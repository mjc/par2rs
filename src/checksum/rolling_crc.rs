//! Rolling CRC32 implementation for efficient sliding window block matching
//!
//! This module implements the rolling CRC32 algorithm used by par2cmdline-turbo
//! to efficiently search for blocks in a sliding window. Instead of recomputing
//! the full CRC32 for each window position (O(n) per position), we can update
//! it incrementally in O(1) time.
//!
//! ## Usage (matching par2cmdline-turbo)
//!
//! ```ignore
//! // Compute initial CRC in standard form
//! let mut crc = compute_crc32(&buffer[0..window_size]);
//!
//! // Slide the window by one byte
//! crc = rolling_table.slide(crc.as_u32(), byte_in, byte_out);
//! ```
//!
//! The `slide()` function takes and returns STANDARD CRC32 values (with the
//! 0xFFFFFFFF XOR applied). Internally it converts to RAW form, performs the
//! rolling operation, then converts back - exactly matching par2cmdline-turbo's
//! CRCSlideChar implementation.
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

// Polynomial for CRC32 (IEEE 802.3)
const CRC_POLYNOMIAL: u32 = 0xEDB88320;

/// GF(2^32) multiplication for CRC polynomial field
/// This matches par2cmdline-turbo's GF32Multiply function exactly
fn gf32_multiply(mut a: u32, mut b: u32, polynomial: u32) -> u32 {
    let mut product = 0u32;
    for _ in 0..31 {
        // NEGATE32(b >> 31) - if high bit set, all 1s, else all 0s
        let mask = (b as i32 >> 31) as u32; // Sign extension
        product ^= mask & a;

        // Update a: shift right and XOR with polynomial if low bit was set
        let low_bit_mask = (a as i32 & 1).wrapping_neg() as u32;
        a = (a >> 1) ^ (polynomial & low_bit_mask);

        b <<= 1;
    }
    // Final iteration
    let mask = (b as i32 >> 31) as u32;
    product ^= mask & a;
    product
}

/// Compute 2^(8n) in CRC's Galois Field
/// This matches par2cmdline-turbo's CRCExp8 function
fn crc_exp8(mut n: u64) -> u32 {
    // Build power table (matching crc32table::power[] generation)
    let mut power = [0u32; 32];
    let mut k = 0x80000000u32 >> 1;
    for i in 0u32..32 {
        power[((i.wrapping_sub(3)) & 31) as usize] = k;
        k = gf32_multiply(k, k, CRC_POLYNOMIAL);
    }

    let mut result = 0x80000000u32;
    let mut power_idx = 0;
    n %= 0xffffffff;
    while n != 0 {
        if n & 1 != 0 {
            result = gf32_multiply(result, power[power_idx], CRC_POLYNOMIAL);
        }
        n >>= 1;
        power_idx = (power_idx + 1) & 31;
    }
    result
}

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
    /// * `current_crc` - Current CRC value for the window (STANDARD form with XOR)
    /// * `byte_in` - New byte entering the window at the end
    /// * `byte_out` - Old byte leaving the window from the start
    ///
    /// # Returns
    /// Updated CRC for the shifted window (STANDARD form with XOR)
    ///
    /// # Algorithm (matching par2cmdline-turbo's CRCSlideChar exactly)
    /// ```cpp
    /// u32 CRCSlideChar(u32 crc, u8 chNew, u8 chOld, const u32 (&windowtable)[256])
    /// {
    ///   crc ^= ~0;  // Convert to RAW
    ///   return ((crc >> 8) & 0x00ffffffL) ^ ccitttable.table[(u8)crc ^ chNew] ^ windowtable[chOld];
    /// }
    /// ```
    #[inline]
    pub fn slide(&self, current_crc: u32, byte_in: u8, byte_out: u8) -> u32 {
        // Convert from STANDARD to RAW (matching par2cmdline's crc ^= ~0)
        let crc = current_crc ^ 0xFFFFFFFF;

        // Update CRC: shift right, add new byte, remove old byte
        ((crc >> 8) & 0x00ffffff)
            ^ CRC_TABLE[((crc ^ byte_in as u32) & 0xFF) as usize]
            ^ self.table[byte_out as usize]
    }

    /// Compute CRC at a specific buffer position
    /// Returns None if there's not enough data for a full block
    pub fn compute_crc_at_position(
        &self,
        buffer: &[u8],
        pos: usize,
        block_size: usize,
        bytes_in_buffer: usize,
    ) -> Option<u32> {
        use crate::checksum::compute_crc32;

        if pos + block_size <= bytes_in_buffer {
            let block = &buffer[pos..pos + block_size];
            Some(compute_crc32(block).as_u32())
        } else {
            None
        }
    }

    /// Slide CRC forward by one byte using the rolling window algorithm
    /// Returns None if there's not enough data for a full block at the current position
    pub fn slide_crc_forward(
        &self,
        current_crc: u32,
        buffer: &[u8],
        pos: usize,
        block_size: usize,
        bytes_in_buffer: usize,
    ) -> Option<u32> {
        if pos + block_size <= bytes_in_buffer {
            let byte_out = buffer[pos - 1];
            let byte_in = buffer[pos + block_size - 1];
            Some(self.slide(current_crc, byte_in, byte_out))
        } else {
            None
        }
    }
}

/// Compute the window mask for a single byte value
///
/// This matches par2cmdline-turbo's GenerateWindowTable exactly:
/// ```cpp
/// void GenerateWindowTable(u64 window, u32 (&target)[256])
/// {
///   u32 coeff = CRCExp8(window);
///   u32 mask = GF32Multiply(~0, coeff, ccitttable.polynom);
///   mask = GF32Multiply(mask, 0x80800000, ccitttable.polynom);
///   mask ^= ~0;
///   
///   for (i16 i=0; i<=255; i++)
///   {
///     target[i] = GF32Multiply(ccitttable.table[i], coeff, ccitttable.polynom) ^ mask;
///   }
/// }
/// ```
fn compute_window_mask(window_size: usize, byte_value: u8) -> u32 {
    // Window coefficient: x^(8*window_size) in GF(2^32)
    let coeff = crc_exp8(window_size as u64);

    // Extend initial CRC (~0) by window_size bytes
    let mut mask = gf32_multiply(0xFFFFFFFF, coeff, CRC_POLYNOMIAL);

    // Multiply by magic constant
    mask = gf32_multiply(mask, 0x80800000, CRC_POLYNOMIAL);

    // Invert to save doing it later
    mask ^= 0xFFFFFFFF;

    // Compute table entry for this byte value
    gf32_multiply(CRC_TABLE[byte_value as usize], coeff, CRC_POLYNOMIAL) ^ mask
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to compute CRC32 in STANDARD form (matching crc32fast)
    fn compute_crc_standard(data: &[u8]) -> u32 {
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(data);
        hasher.finalize()
    }

    #[test]
    fn test_rolling_crc_correctness() {
        // Test that rolling CRC produces same result as recomputing
        let window_size = 1024;
        let table = RollingCrcTable::new(window_size);

        // Create test data: 2 windows worth
        let data: Vec<u8> = (0..window_size * 2).map(|i| (i % 256) as u8).collect();

        // Compute CRC of first window in STANDARD form
        let mut current_crc = compute_crc_standard(&data[0..window_size]);

        // Slide forward and verify each step
        for i in 0..(window_size - 10) {
            // Test first few positions
            // Slide the window using STANDARD CRC
            let byte_out = data[i];
            let byte_in = data[i + window_size];
            current_crc = table.slide(current_crc, byte_in, byte_out);

            // Compute expected CRC directly
            let expected_crc = compute_crc_standard(&data[(i + 1)..(i + 1 + window_size)]);

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

        // Start with window [0..256] in STANDARD form
        let mut rolling_crc = compute_crc_standard(&data[0..window_size]);

        // Slide to [1..257], [2..258], etc.
        for i in 0..100 {
            rolling_crc = table.slide(rolling_crc, data[i + window_size], data[i]);
            let expected = compute_crc_standard(&data[(i + 1)..(i + 1 + window_size)]);
            assert_eq!(rolling_crc, expected, "Mismatch at step {}", i + 1);
        }
    }

    #[test]
    fn test_different_window_sizes() {
        for &size in &[64, 128, 512, 1024, 4096] {
            let table = RollingCrcTable::new(size);
            let data: Vec<u8> = (0..(size * 2)).map(|i| (i % 251) as u8).collect();

            let mut crc = compute_crc_standard(&data[0..size]);
            for i in 0..size {
                crc = table.slide(crc, data[i + size], data[i]);
                let expected = compute_crc_standard(&data[(i + 1)..(i + 1 + size)]);
                assert_eq!(crc, expected, "Size {}, step {}", size, i + 1);
            }
        }
    }
}
