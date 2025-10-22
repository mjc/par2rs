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
/// # use par2rs::reed_solomon::galois::Galois16;
/// # use par2rs::reed_solomon::reedsolomon::build_split_mul_table;
/// # use par2rs::reed_solomon::simd::common::build_nibble_tables;
/// let coeff = Galois16::new(42);
/// let tables = build_split_mul_table(coeff);
/// let nibbles_low = build_nibble_tables(&tables.low);
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
/// Specifies how to combine the multiplication result with the output buffer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOp {
    /// Direct write: output = coefficient * input (replaces contents)
    Direct,
    /// Accumulate: output = output XOR (coefficient * input)
    Add,
}

/// Scalar implementation of Galois Field multiply-accumulate
///
/// Processes input bytes, multiplies by coefficient (via lookup tables), and either:
/// - Directly writes result (WriteOp::Direct)
/// - XOR-accumulates into existing output (WriteOp::Add)
///
/// # Compiler Auto-Optimization
///
/// This simple loop allows the compiler to auto-unroll optimally for each architecture:
/// - ARM64: 4-way unrolling (6.2x smaller code than manual 16x unrolling)
/// - x86-64: 2-way unrolling (10.3x smaller code than manual 16x unrolling)
///
/// # Performance
/// This is the baseline implementation. SIMD implementations aim to be 2-3x faster.
///
/// # Safety
/// - Uses unsafe pointer casts to reinterpret bytes as u16 words
/// - Safe on x86_64/ARM64 which support unaligned loads
/// - Correctly handles odd trailing bytes
#[inline]
pub fn process_slice_multiply_mode(
    input: &[u8],
    output: &mut [u8],
    tables: &SplitMulTable,
    mode: WriteOp,
) {
    let min_len = input.len().min(output.len());
    let num_words = min_len / 2;

    // SAFETY: We're reinterpreting bytes as u16 words. This is safe because:
    // - x86_64/ARM64 support unaligned loads/stores
    // - We've validated we have num_words complete words
    // - Odd trailing byte is handled separately below
    if num_words > 0 {
        unsafe {
            let in_words = std::slice::from_raw_parts(input.as_ptr() as *const u16, num_words);
            let out_words =
                std::slice::from_raw_parts_mut(output.as_mut_ptr() as *mut u16, num_words);
            let low = &tables.low[..];
            let high = &tables.high[..];

            // Simple loop - compiler auto-unrolls optimally for each architecture
            for idx in 0..num_words {
                let in_word = in_words[idx];
                let result = low[(in_word & 0xFF) as usize] ^ high[(in_word >> 8) as usize];
                match mode {
                    WriteOp::Direct => {
                        out_words[idx] = result;
                    }
                    WriteOp::Add => {
                        out_words[idx] ^= result;
                    }
                }
            }
        }
    }

    // Handle odd trailing byte
    if min_len % 2 == 1 {
        let last_idx = num_words * 2;
        let in_byte = input[last_idx];
        let low_byte = tables.low[in_byte as usize].to_le_bytes()[0];
        match mode {
            WriteOp::Direct => {
                output[last_idx] = low_byte;
            }
            WriteOp::Add => {
                output[last_idx] ^= low_byte;
            }
        }
    }
}

/// Convenience wrapper for XOR-accumulate mode
///
/// This is the most common operation in Reed-Solomon reconstruction.
#[inline]
pub fn process_slice_multiply_add_scalar(input: &[u8], output: &mut [u8], tables: &SplitMulTable) {
    process_slice_multiply_mode(input, output, tables, WriteOp::Add);
}
