//! Common SIMD utilities shared across all implementations
//!
//! Provides shared data structures and fallback implementations used by all SIMD variants.

use super::super::reedsolomon::SplitMulTable;

/// Nibble lookup tables for SIMD operations
///
/// Stores the 4 16-byte tables needed for nibble-based GF(2^16) multiplication.
/// This is the core data structure for PSHUFB, NEON vtbl, and portable_simd table lookups.
///
/// # Memory Layout
/// Each table maps a 4-bit nibble (0-15) to the result bytes of GF(2^16) multiplication.
///
/// # Performance
/// Total size: 64 bytes (vs 512 bytes for full byte tables)
/// This 8x reduction fits in L1 cache and enables fast SIMD lookups.
#[derive(Debug, Clone)]
pub struct NibbleTables {
    /// Low nibble (0x0N) → result low byte
    pub lo_nib_lo_byte: [u8; 16],
    /// Low nibble (0x0N) → result high byte
    pub lo_nib_hi_byte: [u8; 16],
    /// High nibble (0xN0) → result low byte
    pub hi_nib_lo_byte: [u8; 16],
    /// High nibble (0xN0) → result high byte
    pub hi_nib_hi_byte: [u8; 16],
}

/// Build nibble lookup tables from a 256-entry GF(2^16) multiplication table
///
/// Splits each byte into low/high nibbles (4 bits each) for SIMD table lookups.
/// This reduces table size from 512 bytes to 64 bytes (8x reduction).
///
/// # Algorithm
/// For each nibble value (0-15):
/// - Low nibble: Look up table[nibble] (e.g., 0x05 → table[0x05])
/// - High nibble: Look up table[nibble << 4] (e.g., 0x05 → table[0x50])
/// - Split each 16-bit result into low/high bytes
///
/// # Usage
/// Used by all SIMD implementations:
/// - PSHUFB (x86_64): Load into __m128i/_m256i registers
/// - NEON (ARM64): Load into uint8x16_t registers  
/// - portable_simd: Load into u8x16 vectors
///
/// # Example
/// ```rust
/// # use par2rs::reed_solomon::{build_split_mul_table, Galois16};
/// let coeff = Galois16::new(42);
/// let tables = build_split_mul_table(coeff);
/// let nibbles_low = super::build_nibble_tables(&tables.low);
/// ```
pub fn build_nibble_tables(table: &[u16; 256]) -> NibbleTables {
    let mut lo_nib_lo_byte = [0u8; 16];
    let mut lo_nib_hi_byte = [0u8; 16];
    let mut hi_nib_lo_byte = [0u8; 16];
    let mut hi_nib_hi_byte = [0u8; 16];

    // For each nibble value (0-15)
    for nib in 0..16 {
        // Low nibble: input byte = nib (0x0N)
        let result_lo = table[nib];
        lo_nib_lo_byte[nib] = (result_lo & 0xFF) as u8;
        lo_nib_hi_byte[nib] = (result_lo >> 8) as u8;

        // High nibble: input byte = nib << 4 (0xN0)
        let result_hi = table[nib << 4];
        hi_nib_lo_byte[nib] = (result_hi & 0xFF) as u8;
        hi_nib_hi_byte[nib] = (result_hi >> 8) as u8;
    }

    NibbleTables {
        lo_nib_lo_byte,
        lo_nib_hi_byte,
        hi_nib_lo_byte,
        hi_nib_hi_byte,
    }
}

/// Scalar GF(2^16) multiply-add fallback
///
/// Processes input word-by-word (16 bits at a time) using lookup tables.
/// Used as fallback for:
/// - Small buffers (< minimum SIMD size)
/// - Remainder bytes after SIMD processing
/// - Platforms without SIMD support
///
/// # Performance
/// This is the baseline implementation. SIMD implementations aim to be 2-3x faster.
///
/// # Safety
/// - Uses unsafe pointer casts to reinterpret bytes as u16 words
/// - Safe on x86_64/ARM64 which support unaligned loads
/// - Correctly handles odd trailing bytes
pub fn process_slice_multiply_add_scalar(input: &[u8], output: &mut [u8], tables: &SplitMulTable) {
    let len = input.len().min(output.len());

    // SAFETY: We're reinterpreting bytes as u16 words. This is safe because:
    // - x86_64/ARM64 support unaligned loads/stores
    // - We've validated we have len / 2 complete words
    // - Odd trailing byte is handled separately below
    let in_words = unsafe { std::slice::from_raw_parts(input.as_ptr() as *const u16, len / 2) };
    let out_words =
        unsafe { std::slice::from_raw_parts_mut(output.as_mut_ptr() as *mut u16, len / 2) };

    let low = &tables.low[..];
    let high = &tables.high[..];

    // Process complete 16-bit words
    for i in 0..in_words.len() {
        let in_word = in_words[i];
        let out_word = out_words[i];
        // GF(2^16) multiply: result = low[low_byte] ^ high[high_byte]
        let result = low[(in_word & 0xFF) as usize] ^ high[(in_word >> 8) as usize];
        // XOR accumulate into output
        out_words[i] = out_word ^ result;
    }

    // Handle odd trailing byte (if len is odd)
    if len % 2 == 1 {
        let last_idx = len - 1;
        let in_byte = input[last_idx];
        let out_byte = output[last_idx];
        // For odd byte, only use low table (high byte is implicitly 0)
        let result_low = low[in_byte as usize];
        output[last_idx] = out_byte ^ (result_low & 0xFF) as u8;
    }
}
