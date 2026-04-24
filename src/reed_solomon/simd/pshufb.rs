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
use super::super::codec::SplitMulTable;
#[cfg(target_arch = "x86_64")]
use super::common::{build_nibble_tables, process_slice_multiply_add_scalar, NibbleTables};

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

/// Prepared AVX2 PSHUFB tables for one GF(2^16) coefficient.
///
/// Building these tables is pure setup work. Create hot paths keep one prepared
/// value per `(recovery output, source input)` coefficient and reuse it for all
/// chunks.
#[derive(Debug, Clone)]
#[cfg(target_arch = "x86_64")]
pub struct Avx2PreparedCoeff {
    low: NibbleTables,
    high: NibbleTables,
}

/// Prepare AVX2 PSHUFB tables for a split multiplication table.
#[cfg(target_arch = "x86_64")]
#[inline]
pub fn prepare_avx2_coeff(tables: &SplitMulTable) -> Avx2PreparedCoeff {
    Avx2PreparedCoeff {
        low: build_nibble_tables(&tables.low),
        high: build_nibble_tables(&tables.high),
    }
}

#[cfg(target_arch = "x86_64")]
struct Avx2CoeffVectors {
    low_lo_nib_lo: __m256i,
    low_lo_nib_hi: __m256i,
    low_hi_nib_lo: __m256i,
    low_hi_nib_hi: __m256i,
    high_lo_nib_lo: __m256i,
    high_lo_nib_hi: __m256i,
    high_hi_nib_lo: __m256i,
    high_hi_nib_hi: __m256i,
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "ssse3")]
unsafe fn load_coeff_vectors(prepared: &Avx2PreparedCoeff) -> Avx2CoeffVectors {
    Avx2CoeffVectors {
        low_lo_nib_lo: _mm256_broadcastsi128_si256(_mm_loadu_si128(
            prepared.low.lo_nib_lo_byte.as_ptr() as *const __m128i,
        )),
        low_lo_nib_hi: _mm256_broadcastsi128_si256(_mm_loadu_si128(
            prepared.low.lo_nib_hi_byte.as_ptr() as *const __m128i,
        )),
        low_hi_nib_lo: _mm256_broadcastsi128_si256(_mm_loadu_si128(
            prepared.low.hi_nib_lo_byte.as_ptr() as *const __m128i,
        )),
        low_hi_nib_hi: _mm256_broadcastsi128_si256(_mm_loadu_si128(
            prepared.low.hi_nib_hi_byte.as_ptr() as *const __m128i,
        )),
        high_lo_nib_lo: _mm256_broadcastsi128_si256(_mm_loadu_si128(
            prepared.high.lo_nib_lo_byte.as_ptr() as *const __m128i,
        )),
        high_lo_nib_hi: _mm256_broadcastsi128_si256(_mm_loadu_si128(
            prepared.high.lo_nib_hi_byte.as_ptr() as *const __m128i,
        )),
        high_hi_nib_lo: _mm256_broadcastsi128_si256(_mm_loadu_si128(
            prepared.high.hi_nib_lo_byte.as_ptr() as *const __m128i,
        )),
        high_hi_nib_hi: _mm256_broadcastsi128_si256(_mm_loadu_si128(
            prepared.high.hi_nib_hi_byte.as_ptr() as *const __m128i,
        )),
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "ssse3")]
unsafe fn multiply_vec_pshufb(
    in_vec: __m256i,
    tables: &Avx2CoeffVectors,
    mask_0x0f: __m256i,
) -> __m256i {
    let low_bytes = _mm256_and_si256(in_vec, _mm256_set1_epi16(0x00FF));
    let high_bytes = _mm256_srli_epi16(in_vec, 8);

    let low_lo_nib = _mm256_and_si256(low_bytes, mask_0x0f);
    let low_hi_nib = _mm256_and_si256(_mm256_srli_epi16(low_bytes, 4), mask_0x0f);

    let low_lo_nib_result_lo = _mm256_shuffle_epi8(tables.low_lo_nib_lo, low_lo_nib);
    let low_lo_nib_result_hi = _mm256_shuffle_epi8(tables.low_lo_nib_hi, low_lo_nib);
    let low_hi_nib_result_lo = _mm256_shuffle_epi8(tables.low_hi_nib_lo, low_hi_nib);
    let low_hi_nib_result_hi = _mm256_shuffle_epi8(tables.low_hi_nib_hi, low_hi_nib);

    let low_byte_result_lo = _mm256_xor_si256(low_lo_nib_result_lo, low_hi_nib_result_lo);
    let low_byte_result_hi = _mm256_xor_si256(low_lo_nib_result_hi, low_hi_nib_result_hi);

    let high_lo_nib = _mm256_and_si256(high_bytes, mask_0x0f);
    let high_hi_nib = _mm256_and_si256(_mm256_srli_epi16(high_bytes, 4), mask_0x0f);

    let high_lo_nib_result_lo = _mm256_shuffle_epi8(tables.high_lo_nib_lo, high_lo_nib);
    let high_lo_nib_result_hi = _mm256_shuffle_epi8(tables.high_lo_nib_hi, high_lo_nib);
    let high_hi_nib_result_lo = _mm256_shuffle_epi8(tables.high_hi_nib_lo, high_hi_nib);
    let high_hi_nib_result_hi = _mm256_shuffle_epi8(tables.high_hi_nib_hi, high_hi_nib);

    let high_byte_result_lo = _mm256_xor_si256(high_lo_nib_result_lo, high_hi_nib_result_lo);
    let high_byte_result_hi = _mm256_xor_si256(high_lo_nib_result_hi, high_hi_nib_result_hi);

    let result_lo = _mm256_xor_si256(low_byte_result_lo, high_byte_result_lo);
    let result_hi = _mm256_xor_si256(low_byte_result_hi, high_byte_result_hi);

    _mm256_or_si256(result_lo, _mm256_slli_epi16(result_hi, 8))
}

/// Build nibble lookup tables for PSHUFB
///
/// Wrapper around common build_nibble_tables() for backwards compatibility.
/// Returns tables as a tuple for use with PSHUFB implementation.
#[cfg(target_arch = "x86_64")]
#[cfg(test)]
fn build_pshufb_tables(table: &[u16; 256]) -> ([u8; 16], [u8; 16], [u8; 16], [u8; 16]) {
    let nibbles = build_nibble_tables(table);
    (
        nibbles.lo_nib_lo_byte,
        nibbles.lo_nib_hi_byte,
        nibbles.hi_nib_lo_byte,
        nibbles.hi_nib_hi_byte,
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
    let prepared = prepare_avx2_coeff(tables);
    process_slice_multiply_add_prepared_avx2(input, output, &prepared, tables);
}

/// PSHUFB-accelerated GF(2^16) multiply-add using precomputed nibble tables.
///
/// # Safety
/// Requires AVX2 and SSSE3 CPU support.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "ssse3")]
pub unsafe fn process_slice_multiply_add_prepared_avx2(
    input: &[u8],
    output: &mut [u8],
    prepared: &Avx2PreparedCoeff,
    scalar_tables: &SplitMulTable,
) {
    let len = input.len().min(output.len());

    // Need at least 32 bytes for AVX2 processing
    if len < 32 {
        // Fall back to scalar for small buffers
        process_slice_multiply_add_scalar(input, output, scalar_tables);
        return;
    }

    let table_vectors = load_coeff_vectors(prepared);
    let mask_0x0f = _mm256_set1_epi8(0x0F);

    // Process 32 bytes at a time
    let mut pos = 0;
    let avx_end = (len / 32) * 32;

    // Check alignment of both input and output pointers
    // Since we now allocate aligned buffers and process full buffers (not sub-slices),
    // alignment should be maintained throughout the Reed-Solomon reconstruction path.
    let input_ptr = input.as_ptr();
    let output_ptr = output.as_ptr();
    let both_aligned =
        (input_ptr as usize).is_multiple_of(32) && (output_ptr as usize).is_multiple_of(32);

    while pos < avx_end {
        // Load 32 bytes of input and output
        // Use aligned loads/stores when both pointers are 32-byte aligned (common case now)
        let in_vec = if both_aligned {
            _mm256_load_si256(input_ptr.add(pos) as *const __m256i)
        } else {
            _mm256_loadu_si256(input_ptr.add(pos) as *const __m256i)
        };
        let out_vec = if both_aligned {
            _mm256_load_si256(output_ptr.add(pos) as *const __m256i)
        } else {
            _mm256_loadu_si256(output_ptr.add(pos) as *const __m256i)
        };

        let result = multiply_vec_pshufb(in_vec, &table_vectors, mask_0x0f);
        let final_result = _mm256_xor_si256(out_vec, result);

        // Store result using aligned store when possible (fast path for reconstruction)
        if both_aligned {
            _mm256_store_si256(output.as_mut_ptr().add(pos) as *mut __m256i, final_result);
        } else {
            _mm256_storeu_si256(output.as_mut_ptr().add(pos) as *mut __m256i, final_result);
        }

        // Debug the final iteration to see if it's corrupting byte 512

        pos += 32;
    }

    // Handle remaining bytes with scalar fallback
    if pos < len {
        let _remaining = len - pos;

        process_slice_multiply_add_scalar(&input[pos..], &mut output[pos..], scalar_tables);
    }
}

/// PSHUFB-accelerated multiply-add for two inputs into one output.
///
/// This is the first packed create kernel: it loads/stores the output once for
/// two staged source inputs.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "ssse3")]
pub unsafe fn process_slices_multiply_add_prepared_avx2_x2(
    input_a: &[u8],
    prepared_a: &Avx2PreparedCoeff,
    scalar_a: &SplitMulTable,
    input_b: &[u8],
    prepared_b: &Avx2PreparedCoeff,
    scalar_b: &SplitMulTable,
    output: &mut [u8],
) {
    let len = input_a.len().min(input_b.len()).min(output.len());
    if len < 32 {
        process_slice_multiply_add_scalar(input_a, output, scalar_a);
        process_slice_multiply_add_scalar(input_b, output, scalar_b);
        return;
    }

    let table_a = load_coeff_vectors(prepared_a);
    let table_b = load_coeff_vectors(prepared_b);
    let mask_0x0f = _mm256_set1_epi8(0x0F);

    let mut pos = 0;
    let avx_end = (len / 32) * 32;
    let input_a_ptr = input_a.as_ptr();
    let input_b_ptr = input_b.as_ptr();
    let output_ptr = output.as_mut_ptr();
    let all_aligned = (input_a_ptr as usize).is_multiple_of(32)
        && (input_b_ptr as usize).is_multiple_of(32)
        && (output_ptr as usize).is_multiple_of(32);

    while pos < avx_end {
        let in_a = if all_aligned {
            _mm256_load_si256(input_a_ptr.add(pos) as *const __m256i)
        } else {
            _mm256_loadu_si256(input_a_ptr.add(pos) as *const __m256i)
        };
        let in_b = if all_aligned {
            _mm256_load_si256(input_b_ptr.add(pos) as *const __m256i)
        } else {
            _mm256_loadu_si256(input_b_ptr.add(pos) as *const __m256i)
        };
        let out_vec = if all_aligned {
            _mm256_load_si256(output_ptr.add(pos) as *const __m256i)
        } else {
            _mm256_loadu_si256(output_ptr.add(pos) as *const __m256i)
        };

        let result_a = multiply_vec_pshufb(in_a, &table_a, mask_0x0f);
        let result_b = multiply_vec_pshufb(in_b, &table_b, mask_0x0f);
        let final_result = _mm256_xor_si256(out_vec, _mm256_xor_si256(result_a, result_b));

        if all_aligned {
            _mm256_store_si256(output_ptr.add(pos) as *mut __m256i, final_result);
        } else {
            _mm256_storeu_si256(output_ptr.add(pos) as *mut __m256i, final_result);
        }

        pos += 32;
    }

    if pos < len {
        process_slice_multiply_add_scalar(&input_a[pos..], &mut output[pos..], scalar_a);
        process_slice_multiply_add_scalar(&input_b[pos..], &mut output[pos..], scalar_b);
    }
}

/// PSHUFB-accelerated multiply-add for four inputs into one output.
///
/// This create kernel keeps a single output load/store for four source inputs,
/// reducing repeated output traversal and giving Rayon coarser work per batch.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "ssse3")]
#[allow(clippy::too_many_arguments)]
pub unsafe fn process_slices_multiply_add_prepared_avx2_x4(
    input_a: &[u8],
    prepared_a: &Avx2PreparedCoeff,
    scalar_a: &SplitMulTable,
    input_b: &[u8],
    prepared_b: &Avx2PreparedCoeff,
    scalar_b: &SplitMulTable,
    input_c: &[u8],
    prepared_c: &Avx2PreparedCoeff,
    scalar_c: &SplitMulTable,
    input_d: &[u8],
    prepared_d: &Avx2PreparedCoeff,
    scalar_d: &SplitMulTable,
    output: &mut [u8],
) {
    let len = input_a
        .len()
        .min(input_b.len())
        .min(input_c.len())
        .min(input_d.len())
        .min(output.len());
    if len < 32 {
        process_slice_multiply_add_scalar(input_a, output, scalar_a);
        process_slice_multiply_add_scalar(input_b, output, scalar_b);
        process_slice_multiply_add_scalar(input_c, output, scalar_c);
        process_slice_multiply_add_scalar(input_d, output, scalar_d);
        return;
    }

    let table_a = load_coeff_vectors(prepared_a);
    let table_b = load_coeff_vectors(prepared_b);
    let table_c = load_coeff_vectors(prepared_c);
    let table_d = load_coeff_vectors(prepared_d);
    let mask_0x0f = _mm256_set1_epi8(0x0F);

    let mut pos = 0;
    let avx_end = (len / 32) * 32;
    let input_a_ptr = input_a.as_ptr();
    let input_b_ptr = input_b.as_ptr();
    let input_c_ptr = input_c.as_ptr();
    let input_d_ptr = input_d.as_ptr();
    let output_ptr = output.as_mut_ptr();
    let all_aligned = (input_a_ptr as usize).is_multiple_of(32)
        && (input_b_ptr as usize).is_multiple_of(32)
        && (input_c_ptr as usize).is_multiple_of(32)
        && (input_d_ptr as usize).is_multiple_of(32)
        && (output_ptr as usize).is_multiple_of(32);

    while pos < avx_end {
        let in_a = if all_aligned {
            _mm256_load_si256(input_a_ptr.add(pos) as *const __m256i)
        } else {
            _mm256_loadu_si256(input_a_ptr.add(pos) as *const __m256i)
        };
        let in_b = if all_aligned {
            _mm256_load_si256(input_b_ptr.add(pos) as *const __m256i)
        } else {
            _mm256_loadu_si256(input_b_ptr.add(pos) as *const __m256i)
        };
        let in_c = if all_aligned {
            _mm256_load_si256(input_c_ptr.add(pos) as *const __m256i)
        } else {
            _mm256_loadu_si256(input_c_ptr.add(pos) as *const __m256i)
        };
        let in_d = if all_aligned {
            _mm256_load_si256(input_d_ptr.add(pos) as *const __m256i)
        } else {
            _mm256_loadu_si256(input_d_ptr.add(pos) as *const __m256i)
        };
        let out_vec = if all_aligned {
            _mm256_load_si256(output_ptr.add(pos) as *const __m256i)
        } else {
            _mm256_loadu_si256(output_ptr.add(pos) as *const __m256i)
        };

        let result_ab = _mm256_xor_si256(
            multiply_vec_pshufb(in_a, &table_a, mask_0x0f),
            multiply_vec_pshufb(in_b, &table_b, mask_0x0f),
        );
        let result_cd = _mm256_xor_si256(
            multiply_vec_pshufb(in_c, &table_c, mask_0x0f),
            multiply_vec_pshufb(in_d, &table_d, mask_0x0f),
        );
        let final_result = _mm256_xor_si256(out_vec, _mm256_xor_si256(result_ab, result_cd));

        if all_aligned {
            _mm256_store_si256(output_ptr.add(pos) as *mut __m256i, final_result);
        } else {
            _mm256_storeu_si256(output_ptr.add(pos) as *mut __m256i, final_result);
        }

        pos += 32;
    }

    if pos < len {
        process_slice_multiply_add_scalar(&input_a[pos..], &mut output[pos..], scalar_a);
        process_slice_multiply_add_scalar(&input_b[pos..], &mut output[pos..], scalar_b);
        process_slice_multiply_add_scalar(&input_c[pos..], &mut output[pos..], scalar_c);
        process_slice_multiply_add_scalar(&input_d[pos..], &mut output[pos..], scalar_d);
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_arch = "x86_64")]
    use super::{
        build_pshufb_tables, prepare_avx2_coeff, process_slice_multiply_add_pshufb,
        process_slices_multiply_add_prepared_avx2_x2, process_slices_multiply_add_prepared_avx2_x4,
    };

    // These are only used in x86_64 tests
    #[cfg(target_arch = "x86_64")]
    use crate::reed_solomon::codec::{build_split_mul_table, process_slice_multiply_add};
    #[cfg(target_arch = "x86_64")]
    use crate::reed_solomon::galois::Galois16;

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

        // Output should be modified by scalar fallback
        assert_ne!(output, original_output);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn process_slices_multiply_add_prepared_avx2_x2_matches_separate_adds() {
        if !is_x86_feature_detected!("avx2") || !is_x86_feature_detected!("ssse3") {
            eprintln!("Skipping PSHUFB x2 test - AVX2/SSSE3 not supported");
            return;
        }

        let input_a = (0..257).map(|idx| (idx * 3) as u8).collect::<Vec<_>>();
        let input_b = (0..257).map(|idx| (idx * 5 + 11) as u8).collect::<Vec<_>>();
        let table_a = build_split_mul_table(Galois16::new(7));
        let table_b = build_split_mul_table(Galois16::new(29));
        let prepared_a = prepare_avx2_coeff(&table_a);
        let prepared_b = prepare_avx2_coeff(&table_b);

        let mut expected = vec![0xA5; input_a.len()];
        process_slice_multiply_add(&input_a, &mut expected, &table_a);
        process_slice_multiply_add(&input_b, &mut expected, &table_b);

        let mut actual = vec![0xA5; input_a.len()];
        unsafe {
            process_slices_multiply_add_prepared_avx2_x2(
                &input_a,
                &prepared_a,
                &table_a,
                &input_b,
                &prepared_b,
                &table_b,
                &mut actual,
            );
        }

        assert_eq!(actual, expected);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn process_slices_multiply_add_prepared_avx2_x4_matches_separate_adds() {
        if !std::is_x86_feature_detected!("avx2") {
            return;
        }

        let inputs = (0..4)
            .map(|source| {
                (0..255)
                    .map(|byte| (byte * 11 + source * 31) as u8)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let coeffs = [3, 5, 7, 11]
            .into_iter()
            .map(|value| {
                let split = build_split_mul_table(Galois16::new(value));
                let prepared = prepare_avx2_coeff(&split);
                (split, prepared)
            })
            .collect::<Vec<_>>();

        let mut expected = vec![0u8; 255];
        inputs
            .iter()
            .zip(coeffs.iter())
            .for_each(|(input, (split, _))| {
                process_slice_multiply_add(input, &mut expected, split)
            });

        let mut actual = vec![0u8; 255];
        unsafe {
            process_slices_multiply_add_prepared_avx2_x4(
                &inputs[0],
                &coeffs[0].1,
                &coeffs[0].0,
                &inputs[1],
                &coeffs[1].1,
                &coeffs[1].0,
                &inputs[2],
                &coeffs[2].1,
                &coeffs[2].0,
                &inputs[3],
                &coeffs[3].1,
                &coeffs[3].0,
                &mut actual,
            );
        }

        assert_eq!(actual, expected);
    }
}
