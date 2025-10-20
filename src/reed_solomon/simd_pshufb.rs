//! PSHUFB-based GF(2^16) multiplication for Reed-Solomon error correction
//!
//! ## Performance
//!
//! Parallel reconstruction with PSHUFB SIMD optimizations achieves significant speedups
//! over par2cmdline on x86_64 systems with AVX2/SSSE3 support.
//!
//! See `docs/SIMD_OPTIMIZATION.md` for detailed performance analysis and benchmarks.
//!
//! ## Vandermonde Polynomial
//!
//! PAR2 uses the primitive irreducible polynomial **0x1100B** (x¹⁶ + x¹² + x³ + x + 1)
//! as the generator for GF(2^16) to construct the Vandermonde matrix for Reed-Solomon codes.
//! This specific polynomial is mandated by the PAR2 specification and cannot be changed.
//!
//! ## PSHUFB Technique
//!
//! This implements the "Screaming Fast Galois Field Arithmetic" technique from
//! James Plank's paper: "Screaming Fast Galois Field Arithmetic Using Intel SIMD Instructions"
//! (http://web.eecs.utk.edu/~plank/plank/papers/FAST-2013-GF.html)
//!
//! Implementation inspired by the galois_2p8 crate (https://github.com/djsweet/galois_2p8)
//! which is MIT licensed. This implementation has been adapted for GF(2^16) with AVX2.
//!
//! ### Algorithm Overview
//!
//! **Key insight**: PSHUFB can handle 16-entry (4-bit) lookups. We have 256-entry (8-bit) tables.
//! **Solution**: Split each byte into two nibbles and do two lookups.
//!
//! For GF(2^16) multiplication with 16-bit words:
//! - Input: 16-bit word = [high_byte:low_byte]
//! - tables.low[low_byte] ^ tables.high[high_byte] = result (16 bits)
//!
//! PSHUFB approach:
//! 1. Build 8 nibble tables (each 16 bytes) from 256-entry tables:
//!    - For tables.low[0-255] (produces u16):
//!      - low_input_lo_nibble -> [result_lo_byte, result_hi_byte]
//!      - low_input_hi_nibble -> [result_lo_byte, result_hi_byte]
//!    - For tables.high[0-255] (produces u16):
//!      - high_input_lo_nibble -> [result_lo_byte, result_hi_byte]
//!      - high_input_hi_nibble -> [result_lo_byte, result_hi_byte]
//!
//! 2. Process 32 bytes (16 words) at a time with AVX2:
//!    - Separate even/odd bytes
//!    - Extract nibbles with masks and shifts
//!    - PSHUFB lookups for each nibble
//!    - XOR results together
//!
//! **Memory savings**: 8 tables × 16 bytes = 128 bytes (vs 2 tables × 256 × 2 bytes = 1024 bytes)

#[cfg(target_arch = "x86_64")]
use super::reedsolomon::SplitMulTable;

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

/// Build nibble lookup tables for PSHUFB
///
/// Takes a 256-entry u16 table and splits it into 4 tables of 16 bytes each:
/// - Low nibble (0-15) → result low byte
/// - Low nibble (0-15) → result high byte  
/// - High nibble (0-15) → result low byte
/// - High nibble (0-15) → result high byte
#[cfg(target_arch = "x86_64")]
fn build_pshufb_tables(table: &[u16; 256]) -> ([u8; 16], [u8; 16], [u8; 16], [u8; 16]) {
    let mut lo_nib_lo_byte = [0u8; 16];
    let mut lo_nib_hi_byte = [0u8; 16];
    let mut hi_nib_lo_byte = [0u8; 16];
    let mut hi_nib_hi_byte = [0u8; 16];

    // For each nibble value (0-15)
    for nib in 0..16 {
        // Low nibble: input byte = nib (i.e., 0x0N)
        let result_lo = table[nib];
        lo_nib_lo_byte[nib] = (result_lo & 0xFF) as u8;
        lo_nib_hi_byte[nib] = (result_lo >> 8) as u8;

        // High nibble: input byte = nib << 4 (i.e., 0xN0)
        let result_hi = table[nib << 4];
        hi_nib_lo_byte[nib] = (result_hi & 0xFF) as u8;
        hi_nib_hi_byte[nib] = (result_hi >> 8) as u8;
    }

    (
        lo_nib_lo_byte,
        lo_nib_hi_byte,
        hi_nib_lo_byte,
        hi_nib_hi_byte,
    )
}

/// PSHUFB-accelerated GF(2^16) multiply-add using AVX2
///
/// Processes 32 bytes (16 x 16-bit words) per iteration using parallel nibble lookups.
///
/// # Safety
/// - Requires AVX2 and SSSE3 CPU support. Caller must ensure CPU has these features before calling.
/// - `input` and `output` slices must each be at least 32 bytes long for full processing.
/// - Only the first `min(input.len(), output.len())` bytes are processed; if less than 32, the function returns immediately.
/// - The memory pointed to by `input` and `output` must be valid for reads and writes of the required length.
/// - The function uses unaligned loads/stores, so alignment is not strictly required, but for best performance, 16- or 32-byte alignment is recommended.
/// - `input` and `output` must not alias (i.e., must not overlap in memory).
/// - The `tables` argument must point to valid lookup tables as expected by the function.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "ssse3")]
pub unsafe fn process_slice_multiply_add_pshufb(
    input: &[u8],
    output: &mut [u8],
    tables: &SplitMulTable,
) {
    let len = input.len().min(output.len());

    // Need at least 32 bytes for AVX2
    if len < 32 {
        return;
    }

    // Build PSHUFB lookup tables (8 tables × 16 bytes = 128 bytes total vs 512 bytes for full tables)
    let (low_lo_nib_lo, low_lo_nib_hi, low_hi_nib_lo, low_hi_nib_hi) =
        build_pshufb_tables(&tables.low);
    let (high_lo_nib_lo, high_lo_nib_hi, high_hi_nib_lo, high_hi_nib_hi) =
        build_pshufb_tables(&tables.high);

    // Load tables into AVX2 registers (broadcast 128-bit to 256-bit for both lanes)
    let low_lo_nib_lo_vec =
        _mm256_broadcastsi128_si256(_mm_loadu_si128(low_lo_nib_lo.as_ptr() as *const __m128i));
    let low_lo_nib_hi_vec =
        _mm256_broadcastsi128_si256(_mm_loadu_si128(low_lo_nib_hi.as_ptr() as *const __m128i));
    let low_hi_nib_lo_vec =
        _mm256_broadcastsi128_si256(_mm_loadu_si128(low_hi_nib_lo.as_ptr() as *const __m128i));
    let low_hi_nib_hi_vec =
        _mm256_broadcastsi128_si256(_mm_loadu_si128(low_hi_nib_hi.as_ptr() as *const __m128i));

    let high_lo_nib_lo_vec =
        _mm256_broadcastsi128_si256(_mm_loadu_si128(high_lo_nib_lo.as_ptr() as *const __m128i));
    let high_lo_nib_hi_vec =
        _mm256_broadcastsi128_si256(_mm_loadu_si128(high_lo_nib_hi.as_ptr() as *const __m128i));
    let high_hi_nib_lo_vec =
        _mm256_broadcastsi128_si256(_mm_loadu_si128(high_hi_nib_lo.as_ptr() as *const __m128i));
    let high_hi_nib_hi_vec =
        _mm256_broadcastsi128_si256(_mm_loadu_si128(high_hi_nib_hi.as_ptr() as *const __m128i));

    let mask_0x0f = _mm256_set1_epi8(0x0F);

    // Process 32 bytes at a time
    let mut pos = 0;
    let avx_end = (len / 32) * 32;

    while pos < avx_end {
        // Load 32 bytes of input and output
        let in_vec = _mm256_loadu_si256(input.as_ptr().add(pos) as *const __m256i);
        let out_vec = _mm256_loadu_si256(output.as_ptr().add(pos) as *const __m256i);

        // Separate even bytes (low bytes of u16 words) and odd bytes (high bytes of u16 words)
        // Even bytes: indices 0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 30
        // Odd bytes:  indices 1, 3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27, 29, 31

        // Extract low bytes (even indices) - these are the low bytes of each u16 word
        let low_bytes = _mm256_and_si256(
            in_vec,
            _mm256_set1_epi16(0x00FF), // Mask to keep only low byte of each word
        );

        // Extract high bytes (odd indices) - shift right by 8 to get high bytes
        let high_bytes = _mm256_srli_epi16(in_vec, 8);

        // Process low bytes: split into nibbles and lookup
        let low_lo_nib = _mm256_and_si256(low_bytes, mask_0x0f);
        let low_hi_nib = _mm256_srli_epi16(
            _mm256_and_si256(low_bytes, _mm256_set1_epi8(0xF0u8 as i8)),
            4,
        );

        // PSHUFB lookups for low byte
        let low_lo_nib_result_lo = _mm256_shuffle_epi8(low_lo_nib_lo_vec, low_lo_nib);
        let low_lo_nib_result_hi = _mm256_shuffle_epi8(low_lo_nib_hi_vec, low_lo_nib);
        let low_hi_nib_result_lo = _mm256_shuffle_epi8(low_hi_nib_lo_vec, low_hi_nib);
        let low_hi_nib_result_hi = _mm256_shuffle_epi8(low_hi_nib_hi_vec, low_hi_nib);

        // XOR low nibble and high nibble results for low byte
        let low_byte_result_lo = _mm256_xor_si256(low_lo_nib_result_lo, low_hi_nib_result_lo);
        let low_byte_result_hi = _mm256_xor_si256(low_lo_nib_result_hi, low_hi_nib_result_hi);

        // Process high bytes: split into nibbles and lookup
        let high_lo_nib = _mm256_and_si256(high_bytes, mask_0x0f);
        let high_hi_nib = _mm256_srli_epi16(
            _mm256_and_si256(high_bytes, _mm256_set1_epi8(0xF0u8 as i8)),
            4,
        );

        // PSHUFB lookups for high byte
        let high_lo_nib_result_lo = _mm256_shuffle_epi8(high_lo_nib_lo_vec, high_lo_nib);
        let high_lo_nib_result_hi = _mm256_shuffle_epi8(high_lo_nib_hi_vec, high_lo_nib);
        let high_hi_nib_result_lo = _mm256_shuffle_epi8(high_hi_nib_lo_vec, high_hi_nib);
        let high_hi_nib_result_hi = _mm256_shuffle_epi8(high_hi_nib_hi_vec, high_hi_nib);

        // XOR low nibble and high nibble results for high byte
        let high_byte_result_lo = _mm256_xor_si256(high_lo_nib_result_lo, high_hi_nib_result_lo);
        let high_byte_result_hi = _mm256_xor_si256(high_lo_nib_result_hi, high_hi_nib_result_hi);

        // Combine low_byte_result and high_byte_result into final 16-bit results
        // XOR the contributions from both bytes
        let result_lo = _mm256_xor_si256(low_byte_result_lo, high_byte_result_lo);
        let result_hi = _mm256_xor_si256(low_byte_result_hi, high_byte_result_hi);

        // Combine lo and hi bytes back into 16-bit words
        let result = _mm256_or_si256(result_lo, _mm256_slli_epi16(result_hi, 8));

        // XOR with output (multiply-add operation)
        let final_result = _mm256_xor_si256(out_vec, result);

        // Store result
        _mm256_storeu_si256(output.as_mut_ptr().add(pos) as *mut __m256i, final_result);

        pos += 32;
    }

    // Handle remaining bytes with scalar code (fallback in parent function)
}

#[cfg(test)]
mod tests {
    #[cfg(target_arch = "x86_64")]
    use super::{build_pshufb_tables, process_slice_multiply_add_pshufb};

    // These are only used in x86_64 tests
    #[cfg(target_arch = "x86_64")]
    use crate::reed_solomon::galois::Galois16;
    #[cfg(target_arch = "x86_64")]
    use crate::reed_solomon::reedsolomon::build_split_mul_table;

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn build_pshufb_tables_basic() {
        // Create a simple identity-like table for testing
        let mut table = [0u16; 256];
        for (i, item) in table.iter_mut().enumerate() {
            *item = i as u16;
        }

        let (lo_nib_lo, lo_nib_hi, hi_nib_lo, hi_nib_hi) = build_pshufb_tables(&table);

        // Low nibble 0: table[0] = 0 -> lo=0, hi=0
        assert_eq!(lo_nib_lo[0], 0);
        assert_eq!(lo_nib_hi[0], 0);

        // Low nibble 1: table[1] = 1 -> lo=1, hi=0
        assert_eq!(lo_nib_lo[1], 1);
        assert_eq!(lo_nib_hi[1], 0);

        // High nibble 0: table[0x00] = 0 -> lo=0, hi=0
        assert_eq!(hi_nib_lo[0], 0);
        assert_eq!(hi_nib_hi[0], 0);

        // High nibble 1: table[0x10] = 16 -> lo=16, hi=0
        assert_eq!(hi_nib_lo[1], 16);
        assert_eq!(hi_nib_hi[1], 0);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn build_pshufb_tables_zero_table() {
        let table = [0u16; 256];

        let (lo_nib_lo, lo_nib_hi, hi_nib_lo, hi_nib_hi) = build_pshufb_tables(&table);

        // All tables should be zero
        for &val in &lo_nib_lo {
            assert_eq!(val, 0);
        }
        for &val in &lo_nib_hi {
            assert_eq!(val, 0);
        }
        for &val in &hi_nib_lo {
            assert_eq!(val, 0);
        }
        for &val in &hi_nib_hi {
            assert_eq!(val, 0);
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn build_pshufb_tables_nibble_extraction() {
        let mut table = [0u16; 256];
        table[0x0F] = 0xABCD; // Low nibble 0xF
        table[0xF0] = 0x1234; // High nibble 0xF

        let (lo_nib_lo, lo_nib_hi, hi_nib_lo, hi_nib_hi) = build_pshufb_tables(&table);

        // Low nibble 0xF: table[0x0F] = 0xABCD
        assert_eq!(lo_nib_lo[0xF], 0xCD); // Low byte
        assert_eq!(lo_nib_hi[0xF], 0xAB); // High byte

        // High nibble 0xF: table[0xF0] = 0x1234
        assert_eq!(hi_nib_lo[0xF], 0x34); // Low byte
        assert_eq!(hi_nib_hi[0xF], 0x12); // High byte
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn process_slice_multiply_add_pshufb_requires_avx2() {
        if !is_x86_feature_detected!("avx2") || !is_x86_feature_detected!("ssse3") {
            eprintln!("Skipping PSHUFB test - AVX2/SSSE3 not supported");
            return;
        }

        let input = vec![0x5Au8; 64];
        let mut output = vec![0xA5u8; 64];
        let tables = build_split_mul_table(Galois16::new(7));
        let original_output = output.clone();

        unsafe {
            process_slice_multiply_add_pshufb(&input, &mut output, &tables);
        }

        // Output should be modified
        assert_ne!(output, original_output);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn process_slice_multiply_add_pshufb_small_buffer() {
        if !is_x86_feature_detected!("avx2") || !is_x86_feature_detected!("ssse3") {
            eprintln!("Skipping PSHUFB small buffer test - AVX2/SSSE3 not supported");
            return;
        }

        let input = vec![1u8, 2, 3, 4];
        let mut output = vec![0u8; 4];
        let tables = build_split_mul_table(Galois16::new(2));
        let original_output = output.clone();

        unsafe {
            // Should return immediately for buffers < 32 bytes
            process_slice_multiply_add_pshufb(&input, &mut output, &tables);
        }

        // Output should NOT be modified (buffer too small)
        assert_eq!(output, original_output);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn process_slice_multiply_add_pshufb_exactly_32_bytes() {
        if !is_x86_feature_detected!("avx2") || !is_x86_feature_detected!("ssse3") {
            eprintln!("Skipping PSHUFB exact size test - AVX2/SSSE3 not supported");
            return;
        }

        let input = vec![1u8; 32];
        let mut output = vec![0u8; 32];
        let tables = build_split_mul_table(Galois16::new(3));

        unsafe {
            process_slice_multiply_add_pshufb(&input, &mut output, &tables);
        }

        // Output should be non-zero
        assert!(output.iter().any(|&b| b != 0));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn process_slice_multiply_add_pshufb_large_buffer() {
        if !is_x86_feature_detected!("avx2") || !is_x86_feature_detected!("ssse3") {
            eprintln!("Skipping PSHUFB large buffer test - AVX2/SSSE3 not supported");
            return;
        }

        let input = vec![0xAAu8; 128];
        let mut output = vec![0x55u8; 128];
        let tables = build_split_mul_table(Galois16::new(11));
        let original_output = output.clone();

        unsafe {
            process_slice_multiply_add_pshufb(&input, &mut output, &tables);
        }

        // Output should be significantly different
        assert_ne!(output, original_output);

        // Verify at least some bytes changed
        let changed = output
            .iter()
            .zip(original_output.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(changed > 0, "Expected some bytes to change");
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn process_slice_multiply_add_pshufb_accumulates() {
        if !is_x86_feature_detected!("avx2") || !is_x86_feature_detected!("ssse3") {
            eprintln!("Skipping PSHUFB accumulate test - AVX2/SSSE3 not supported");
            return;
        }

        let input = vec![1u8; 64];
        let mut output = vec![2u8; 64];
        let tables = build_split_mul_table(Galois16::new(5));

        // Run twice to verify it accumulates (XOR)
        unsafe {
            process_slice_multiply_add_pshufb(&input, &mut output, &tables);
            let after_first = output.clone();
            process_slice_multiply_add_pshufb(&input, &mut output, &tables);

            // After two identical operations, should XOR back to original or close
            // (depending on the math, but definitely should be different from after_first)
            assert_ne!(output, after_first, "Second operation should modify output");
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn process_slice_multiply_add_pshufb_empty_buffer() {
        if !is_x86_feature_detected!("avx2") || !is_x86_feature_detected!("ssse3") {
            eprintln!("Skipping PSHUFB empty buffer test - AVX2/SSSE3 not supported");
            return;
        }

        let input: Vec<u8> = vec![];
        let mut output: Vec<u8> = vec![];
        let tables = build_split_mul_table(Galois16::new(1));

        unsafe {
            // Should not panic
            process_slice_multiply_add_pshufb(&input, &mut output, &tables);
        }
    }
}
