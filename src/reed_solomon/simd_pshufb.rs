//! PSHUFB-based GF(2^16) multiplication for Reed-Solomon error correction
//!
//! ## Performance
//!
//! Achieves **2.76x speedup** over scalar code (54.7ns vs 150.9ns per 528-byte block).
//! Real-world: **1.66x faster** than par2cmdline (0.607s vs 1.008s for 100MB file repair).
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
    
    (lo_nib_lo_byte, lo_nib_hi_byte, hi_nib_lo_byte, hi_nib_hi_byte)
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
    let (low_lo_nib_lo, low_lo_nib_hi, low_hi_nib_lo, low_hi_nib_hi) = build_pshufb_tables(&tables.low);
    let (high_lo_nib_lo, high_lo_nib_hi, high_hi_nib_lo, high_hi_nib_hi) = build_pshufb_tables(&tables.high);
    
    // Load tables into AVX2 registers (broadcast 128-bit to 256-bit for both lanes)
    let low_lo_nib_lo_vec = _mm256_broadcastsi128_si256(_mm_loadu_si128(low_lo_nib_lo.as_ptr() as *const __m128i));
    let low_lo_nib_hi_vec = _mm256_broadcastsi128_si256(_mm_loadu_si128(low_lo_nib_hi.as_ptr() as *const __m128i));
    let low_hi_nib_lo_vec = _mm256_broadcastsi128_si256(_mm_loadu_si128(low_hi_nib_lo.as_ptr() as *const __m128i));
    let low_hi_nib_hi_vec = _mm256_broadcastsi128_si256(_mm_loadu_si128(low_hi_nib_hi.as_ptr() as *const __m128i));
    
    let high_lo_nib_lo_vec = _mm256_broadcastsi128_si256(_mm_loadu_si128(high_lo_nib_lo.as_ptr() as *const __m128i));
    let high_lo_nib_hi_vec = _mm256_broadcastsi128_si256(_mm_loadu_si128(high_lo_nib_hi.as_ptr() as *const __m128i));
    let high_hi_nib_lo_vec = _mm256_broadcastsi128_si256(_mm_loadu_si128(high_hi_nib_lo.as_ptr() as *const __m128i));
    let high_hi_nib_hi_vec = _mm256_broadcastsi128_si256(_mm_loadu_si128(high_hi_nib_hi.as_ptr() as *const __m128i));
    
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
            _mm256_set1_epi16(0x00FF)  // Mask to keep only low byte of each word
        );
        
        // Extract high bytes (odd indices) - shift right by 8 to get high bytes
        let high_bytes = _mm256_srli_epi16(in_vec, 8);
        
        // Process low bytes: split into nibbles and lookup
        let low_lo_nib = _mm256_and_si256(low_bytes, mask_0x0f);
        let low_hi_nib = _mm256_srli_epi16(_mm256_and_si256(low_bytes, _mm256_set1_epi8(0xF0u8 as i8)), 4);
        
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
        let high_hi_nib = _mm256_srli_epi16(_mm256_and_si256(high_bytes, _mm256_set1_epi8(0xF0u8 as i8)), 4);
        
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
        let result = _mm256_or_si256(
            result_lo,
            _mm256_slli_epi16(result_hi, 8)
        );
        
        // XOR with output (multiply-add operation)
        let final_result = _mm256_xor_si256(out_vec, result);
        
        // Store result
        _mm256_storeu_si256(output.as_mut_ptr().add(pos) as *mut __m256i, final_result);
        
        pos += 32;
    }
    
    // Handle remaining bytes with scalar code (fallback in parent function)
}
