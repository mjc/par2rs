//! Rolling CRC32 implementation for efficient sliding window block matching
//!
//! This module implements the rolling CRC32 algorithm used by par2cmdline-turbo
//! to efficiently search for blocks in a sliding window. Instead of recomputing
//! the full CRC32 for each window position (O(n) per position), we can update
//! it incrementally in O(1) time.
//!
//! ## Important: RAW CRC Values
//!
//! **This implementation uses RAW CRC32 values (without the standard 0xFFFFFFFF
//! initial and final XOR).** This matches par2cmdline's approach and is necessary
//! for the rolling window algorithm to work correctly.
//!
//! To integrate with standard CRC32 (like `crc32fast`):
//! ```ignore
//! // Convert standard CRC to raw for rolling
//! let raw_crc = standard_crc ^ 0xFFFFFFFF;
//!
//! // Use rolling CRC
//! let new_raw_crc = table.slide(raw_crc, byte_in, byte_out);
//!
//! // Convert back to standard CRC
//! let new_standard_crc = new_raw_crc ^ 0xFFFFFFFF;
//! ```
//!
//! ## Algorithm Overview
//!
//! CRC32 has a mathematical property where:
//! - CRC(A ⊕ B) = CRC(A) ⊕ CRC(B) (linearity over XOR)
//! - We can remove a byte's contribution by XORing with a precomputed value
//!
//! The "window table" contains the CRC contribution of each possible byte value
//! when it's at the start of a window of a given size. When that byte slides
//! out of the window, we XOR with its table entry to remove its contribution,
//! then add the new byte at the end using standard CRC update.
//!
//! ## Performance Impact
//!
//! For a block size of 8MB and window size of 16MB:
//! - **Without rolling CRC**: 16MB worth of CRC32 computations per byte step = ~16 million ops
//! - **With rolling CRC**: 2 XOR operations + 1 table lookup = ~3 ops
//!
//! This is approximately **5,000,000x faster** per step!

// Standard CRC32 lookup table (IEEE polynomial 0xEDB88320)
// We need this for the rolling window algorithm
const fn generate_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

const CRC_TABLE: [u32; 256] = generate_crc_table();

/// Precomputed table for rolling CRC32 window
///
/// Each entry `windowtable[b]` represents the CRC32 contribution of byte value `b`
/// when it occupies position 0 in a window of the specified size.
///
/// This table enables O(1) removal of the oldest byte's contribution when
/// sliding the window forward.
#[derive(Clone)]
pub struct RollingCrcTable {
    table: [u32; 256],
    window_size: usize,
}

impl RollingCrcTable {
    /// Generate a window table for the given window size
    ///
    /// # Arguments
    /// * `window_size` - Size of the sliding window in bytes
    ///
    /// # Algorithm
    /// For each possible byte value (0-255):
    /// 1. Compute CRC of that single byte
    /// 2. Multiply by x^(8*(window_size-1)) in the CRC polynomial field
    ///    This simulates shifting the byte from the end to the beginning of the window
    pub fn new(window_size: usize) -> Self {
        let mut table = [0u32; 256];

        for byte_value in 0..=255u8 {
            table[byte_value as usize] = compute_window_mask(window_size, byte_value);
        }

        RollingCrcTable { table, window_size }
    }

    /// Get the window size this table was generated for
    #[inline]
    pub fn window_size(&self) -> usize {
        self.window_size
    }

    /// Update CRC32 by sliding the window one byte forward
    ///
    /// # Arguments
    /// * `current_crc` - Current CRC value for the window (RAW, no initial/final XOR)
    /// * `byte_in` - New byte entering the window at the end
    /// * `byte_out` - Old byte leaving the window from the start
    ///
    /// # Returns
    /// Updated CRC for the shifted window (RAW, no initial/final XOR)
    ///
    /// # Algorithm (matching par2cmdline exactly)
    /// 1. Remove old byte's contribution: `crc ^= windowtable[byte_out]`
    /// 2. Add new byte's contribution: standard CRC update
    ///
    /// # Important
    /// This works with RAW CRC values (no 0xFFFFFFFF XOR).
    /// To use with standard CRC32:
    /// - Before first call: `raw_crc = standard_crc ^ 0xFFFFFFFF`
    /// - After final call: `standard_crc = raw_crc ^ 0xFFFFFFFF`
    #[inline]
    pub fn slide(&self, current_crc: u32, byte_in: u8, byte_out: u8) -> u32 {
        // Remove the contribution of the outgoing byte
        let crc = current_crc ^ self.table[byte_out as usize];

        // Add the contribution of the incoming byte (standard CRC32 update)
        crc_update_byte_raw(crc, byte_in)
    }
}

/// Compute the window mask for a single byte value
///
/// This computes what CRC contribution a byte has when it's at position 0
/// in a window of the given size.
///
/// # Algorithm (matching par2cmdline)
/// 1. Compute RAW CRC of single byte (starting from 0, not 0xFFFFFFFF)
/// 2. Shift it through (window_size - 1) zero bytes
fn compute_window_mask(window_size: usize, byte_value: u8) -> u32 {
    // Compute CRC of single byte WITHOUT initial XOR (raw polynomial operation)
    let mut crc = crc_update_byte_raw(0, byte_value);

    // Shift the byte's contribution through (window_size - 1) positions
    // Each shift is equivalent to processing a zero byte
    for _ in 1..window_size {
        crc = crc_shift_byte(crc);
    }

    crc
}

/// Update CRC32 with a single byte using raw table lookup
#[inline]
fn crc_update_byte_raw(crc: u32, byte: u8) -> u32 {
    (crc >> 8) ^ CRC_TABLE[((crc ^ byte as u32) & 0xFF) as usize]
}

/// Shift CRC by one byte position (equivalent to appending a zero byte)
#[inline]
fn crc_shift_byte(crc: u32) -> u32 {
    (crc >> 8) ^ CRC_TABLE[(crc & 0xFF) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to compute CRC32 in RAW form (matching par2cmdline)
    /// This does NOT apply the initial 0xFFFFFFFF or final 0xFFFFFFFF XOR
    fn compute_crc_raw(data: &[u8]) -> u32 {
        let mut crc = 0; // Start with 0, not 0xFFFFFFFF
        for &byte in data {
            crc = crc_update_byte_raw(crc, byte);
        }
        crc // No final XOR
    }

    #[test]
    fn test_rolling_crc_correctness() {
        // Test that rolling CRC produces same result as recomputing
        let window_size = 1024;
        let table = RollingCrcTable::new(window_size);

        // Create test data: 2 windows worth
        let data: Vec<u8> = (0..window_size * 2).map(|i| (i % 256) as u8).collect();

        // Compute CRC of first window directly
        let mut current_crc = compute_crc_raw(&data[0..window_size]);

        // Slide forward and verify each step
        for i in 0..(window_size - 10) {
            // Test first few positions
            // Slide the window
            let byte_out = data[i];
            let byte_in = data[i + window_size];
            current_crc = table.slide(current_crc, byte_in, byte_out);

            // Compute expected CRC directly
            let expected_crc = compute_crc_raw(&data[(i + 1)..(i + 1 + window_size)]);

            assert_eq!(
                current_crc,
                expected_crc,
                "Rolling CRC mismatch at offset {}",
                i + 1
            );
        }
    }

    #[test]
    fn test_window_table_generation() {
        let table = RollingCrcTable::new(512);
        assert_eq!(table.window_size(), 512);

        // Verify table is populated (non-zero entries)
        assert!(table.table.iter().any(|&x| x != 0));
    }

    #[test]
    fn test_rolling_multiple_steps() {
        let window_size = 256;
        let table = RollingCrcTable::new(window_size);

        let data: Vec<u8> = (0..512).map(|i| ((i * 7) % 256) as u8).collect();

        // Start with window [0..256]
        let mut rolling_crc = compute_crc_raw(&data[0..window_size]);

        // Slide to [1..257], [2..258], etc.
        for i in 0..100 {
            rolling_crc = table.slide(rolling_crc, data[i + window_size], data[i]);
            let expected = compute_crc_raw(&data[(i + 1)..(i + 1 + window_size)]);
            assert_eq!(rolling_crc, expected, "Mismatch at step {}", i + 1);
        }
    }

    #[test]
    fn test_different_window_sizes() {
        for &size in &[64, 128, 512, 1024, 4096] {
            let table = RollingCrcTable::new(size);
            let data: Vec<u8> = (0..(size * 2)).map(|i| (i % 251) as u8).collect();

            let mut crc = compute_crc_raw(&data[0..size]);
            for i in 0..size {
                crc = table.slide(crc, data[i + size], data[i]);
                let expected = compute_crc_raw(&data[(i + 1)..(i + 1 + size)]);
                assert_eq!(crc, expected, "Size {}, step {}", size, i + 1);
            }
        }
    }
}
