//! SIMD-optimized Galois Field multiplication for Reed-Solomon operations
//!
//! Uses AVX2/SSSE3 PSHUFB instructions for parallel GF(2^16) multiplication via table lookups.
//! Based on the "Screaming Fast Galois Field Arithmetic" paper and reed-solomon-erasure crate.
//!
//! The technique splits bytes into low/high nibbles and uses PSHUFB for parallel lookups.

use super::reedsolomon::SplitMulTable;

/// Runtime detection of CPU SIMD features
pub fn detect_simd_support() -> SimdLevel {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("ssse3") {
            return SimdLevel::Avx2;
        }
        if is_x86_feature_detected!("ssse3") {
            return SimdLevel::Ssse3;
        }
    }
    SimdLevel::None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimdLevel {
    None,
    Ssse3,
    Avx2,
}

/// Aggressive AVX2 implementation with 32-word unrolling
///
/// # Safety
/// - Requires AVX2 CPU support. Caller must ensure the CPU supports AVX2 before calling.
/// - `input` and `output` slices must be aligned to 2 bytes (suitable for `u16` access).
/// - The lengths of `input` and `output` must be even (multiples of 2), as the function processes data as `u16` words.
/// - `input` and `output` must each be at least as long as the number of bytes to be processed.
/// - `output` must be mutable and must not alias `input`.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
pub unsafe fn process_slice_multiply_add_avx2_unrolled(
    input: &[u8],
    output: &mut [u8],
    tables: &SplitMulTable,
) {
    let len = input.len().min(output.len());
    let num_words = len / 2;

    if num_words == 0 {
        return;
    }

    // SAFETY: Reinterpret byte slices as u16 slices for performance.
    // - x86-64 supports unaligned loads/stores
    // - We've checked we have at least num_words * 2 bytes available
    let in_words = std::slice::from_raw_parts(input.as_ptr() as *const u16, num_words);
    let out_words = std::slice::from_raw_parts_mut(output.as_mut_ptr() as *mut u16, num_words);
    let low = &tables.low[..];
    let high = &tables.high[..];

    // Process 32 words at a time (64 bytes) for maximum AVX2 utilization
    let avx_words = (num_words / 32) * 32;
    let mut idx = 0;

    // Hyper-aggressive unrolling: 32 words per iteration
    while idx < avx_words {
        // Load 32 input words in batches of 16
        let i0 = in_words[idx];
        let i1 = in_words[idx + 1];
        let i2 = in_words[idx + 2];
        let i3 = in_words[idx + 3];
        let i4 = in_words[idx + 4];
        let i5 = in_words[idx + 5];
        let i6 = in_words[idx + 6];
        let i7 = in_words[idx + 7];
        let i8 = in_words[idx + 8];
        let i9 = in_words[idx + 9];
        let i10 = in_words[idx + 10];
        let i11 = in_words[idx + 11];
        let i12 = in_words[idx + 12];
        let i13 = in_words[idx + 13];
        let i14 = in_words[idx + 14];
        let i15 = in_words[idx + 15];

        let i16 = in_words[idx + 16];
        let i17 = in_words[idx + 17];
        let i18 = in_words[idx + 18];
        let i19 = in_words[idx + 19];
        let i20 = in_words[idx + 20];
        let i21 = in_words[idx + 21];
        let i22 = in_words[idx + 22];
        let i23 = in_words[idx + 23];
        let i24 = in_words[idx + 24];
        let i25 = in_words[idx + 25];
        let i26 = in_words[idx + 26];
        let i27 = in_words[idx + 27];
        let i28 = in_words[idx + 28];
        let i29 = in_words[idx + 29];
        let i30 = in_words[idx + 30];
        let i31 = in_words[idx + 31];

        // Perform lookups and XOR (compiler will pipeline these heavily)
        let r0 = out_words[idx] ^ (low[(i0 & 0xFF) as usize] ^ high[(i0 >> 8) as usize]);
        let r1 = out_words[idx + 1] ^ (low[(i1 & 0xFF) as usize] ^ high[(i1 >> 8) as usize]);
        let r2 = out_words[idx + 2] ^ (low[(i2 & 0xFF) as usize] ^ high[(i2 >> 8) as usize]);
        let r3 = out_words[idx + 3] ^ (low[(i3 & 0xFF) as usize] ^ high[(i3 >> 8) as usize]);
        let r4 = out_words[idx + 4] ^ (low[(i4 & 0xFF) as usize] ^ high[(i4 >> 8) as usize]);
        let r5 = out_words[idx + 5] ^ (low[(i5 & 0xFF) as usize] ^ high[(i5 >> 8) as usize]);
        let r6 = out_words[idx + 6] ^ (low[(i6 & 0xFF) as usize] ^ high[(i6 >> 8) as usize]);
        let r7 = out_words[idx + 7] ^ (low[(i7 & 0xFF) as usize] ^ high[(i7 >> 8) as usize]);
        let r8 = out_words[idx + 8] ^ (low[(i8 & 0xFF) as usize] ^ high[(i8 >> 8) as usize]);
        let r9 = out_words[idx + 9] ^ (low[(i9 & 0xFF) as usize] ^ high[(i9 >> 8) as usize]);
        let r10 = out_words[idx + 10] ^ (low[(i10 & 0xFF) as usize] ^ high[(i10 >> 8) as usize]);
        let r11 = out_words[idx + 11] ^ (low[(i11 & 0xFF) as usize] ^ high[(i11 >> 8) as usize]);
        let r12 = out_words[idx + 12] ^ (low[(i12 & 0xFF) as usize] ^ high[(i12 >> 8) as usize]);
        let r13 = out_words[idx + 13] ^ (low[(i13 & 0xFF) as usize] ^ high[(i13 >> 8) as usize]);
        let r14 = out_words[idx + 14] ^ (low[(i14 & 0xFF) as usize] ^ high[(i14 >> 8) as usize]);
        let r15 = out_words[idx + 15] ^ (low[(i15 & 0xFF) as usize] ^ high[(i15 >> 8) as usize]);

        let r16 = out_words[idx + 16] ^ (low[(i16 & 0xFF) as usize] ^ high[(i16 >> 8) as usize]);
        let r17 = out_words[idx + 17] ^ (low[(i17 & 0xFF) as usize] ^ high[(i17 >> 8) as usize]);
        let r18 = out_words[idx + 18] ^ (low[(i18 & 0xFF) as usize] ^ high[(i18 >> 8) as usize]);
        let r19 = out_words[idx + 19] ^ (low[(i19 & 0xFF) as usize] ^ high[(i19 >> 8) as usize]);
        let r20 = out_words[idx + 20] ^ (low[(i20 & 0xFF) as usize] ^ high[(i20 >> 8) as usize]);
        let r21 = out_words[idx + 21] ^ (low[(i21 & 0xFF) as usize] ^ high[(i21 >> 8) as usize]);
        let r22 = out_words[idx + 22] ^ (low[(i22 & 0xFF) as usize] ^ high[(i22 >> 8) as usize]);
        let r23 = out_words[idx + 23] ^ (low[(i23 & 0xFF) as usize] ^ high[(i23 >> 8) as usize]);
        let r24 = out_words[idx + 24] ^ (low[(i24 & 0xFF) as usize] ^ high[(i24 >> 8) as usize]);
        let r25 = out_words[idx + 25] ^ (low[(i25 & 0xFF) as usize] ^ high[(i25 >> 8) as usize]);
        let r26 = out_words[idx + 26] ^ (low[(i26 & 0xFF) as usize] ^ high[(i26 >> 8) as usize]);
        let r27 = out_words[idx + 27] ^ (low[(i27 & 0xFF) as usize] ^ high[(i27 >> 8) as usize]);
        let r28 = out_words[idx + 28] ^ (low[(i28 & 0xFF) as usize] ^ high[(i28 >> 8) as usize]);
        let r29 = out_words[idx + 29] ^ (low[(i29 & 0xFF) as usize] ^ high[(i29 >> 8) as usize]);
        let r30 = out_words[idx + 30] ^ (low[(i30 & 0xFF) as usize] ^ high[(i30 >> 8) as usize]);
        let r31 = out_words[idx + 31] ^ (low[(i31 & 0xFF) as usize] ^ high[(i31 >> 8) as usize]);

        // Store all results
        out_words[idx] = r0;
        out_words[idx + 1] = r1;
        out_words[idx + 2] = r2;
        out_words[idx + 3] = r3;
        out_words[idx + 4] = r4;
        out_words[idx + 5] = r5;
        out_words[idx + 6] = r6;
        out_words[idx + 7] = r7;
        out_words[idx + 8] = r8;
        out_words[idx + 9] = r9;
        out_words[idx + 10] = r10;
        out_words[idx + 11] = r11;
        out_words[idx + 12] = r12;
        out_words[idx + 13] = r13;
        out_words[idx + 14] = r14;
        out_words[idx + 15] = r15;
        out_words[idx + 16] = r16;
        out_words[idx + 17] = r17;
        out_words[idx + 18] = r18;
        out_words[idx + 19] = r19;
        out_words[idx + 20] = r20;
        out_words[idx + 21] = r21;
        out_words[idx + 22] = r22;
        out_words[idx + 23] = r23;
        out_words[idx + 24] = r24;
        out_words[idx + 25] = r25;
        out_words[idx + 26] = r26;
        out_words[idx + 27] = r27;
        out_words[idx + 28] = r28;
        out_words[idx + 29] = r29;
        out_words[idx + 30] = r30;
        out_words[idx + 31] = r31;

        idx += 32;
    }

    // Handle remaining words with scalar code
    while idx < num_words {
        let in_word = in_words[idx];
        let out_word = out_words[idx];
        let result = low[(in_word & 0xFF) as usize] ^ high[(in_word >> 8) as usize];
        out_words[idx] = out_word ^ result;
        idx += 1;
    }

    // Handle odd trailing byte if any
    if len % 2 == 1 {
        let last_idx = len - 1;
        let in_byte = input[last_idx];
        let out_byte = output[last_idx];
        let result_low = low[in_byte as usize];
        output[last_idx] = out_byte ^ (result_low & 0xFF) as u8;
    }
}

/// Portable SIMD implementation using nibble-based table lookups
///
/// Uses the same nibble lookup strategy as PSHUFB/NEON but with portable_simd swizzle operations.
/// The key insight is using swizzle_dyn() for parallel table lookups instead of scalar loops.
///
/// See docs/SIMD_OPTIMIZATION.md for performance benchmarks.
///
/// # Safety
/// - `input` and `output` slices must not alias
/// - Lengths must be compatible (processes min(input.len(), output.len()) bytes)
pub unsafe fn process_slice_multiply_add_portable_simd(
    input: &[u8],
    output: &mut [u8],
    tables: &SplitMulTable,
) {
    use std::simd::{prelude::*, simd_swizzle, u8x16};

    let len = input.len().min(output.len());

    // Build 16-byte nibble lookup tables
    // For each nibble value 0x0-0xF, store the multiplication result bytes
    // We need separate tables for tables.low and tables.high
    let mut lo_nib_lo = [0u8; 16];
    let mut lo_nib_hi = [0u8; 16];
    let mut hi_nib_lo = [0u8; 16];
    let mut hi_nib_hi = [0u8; 16];

    let mut lo_nib_lo_h = [0u8; 16];
    let mut lo_nib_hi_h = [0u8; 16];
    let mut hi_nib_lo_h = [0u8; 16];
    let mut hi_nib_hi_h = [0u8; 16];

    for nibble in 0..16u8 {
        // tables.low - for low nibble and high nibble
        let low_result_lo_nib = tables.low[nibble as usize];
        lo_nib_lo[nibble as usize] = (low_result_lo_nib & 0xFF) as u8;
        lo_nib_hi[nibble as usize] = (low_result_lo_nib >> 8) as u8;

        let low_result_hi_nib = tables.low[(nibble << 4) as usize];
        hi_nib_lo[nibble as usize] = (low_result_hi_nib & 0xFF) as u8;
        hi_nib_hi[nibble as usize] = (low_result_hi_nib >> 8) as u8;

        // tables.high - for low nibble and high nibble
        let high_result_lo_nib = tables.high[nibble as usize];
        lo_nib_lo_h[nibble as usize] = (high_result_lo_nib & 0xFF) as u8;
        lo_nib_hi_h[nibble as usize] = (high_result_lo_nib >> 8) as u8;

        let high_result_hi_nib = tables.high[(nibble << 4) as usize];
        hi_nib_lo_h[nibble as usize] = (high_result_hi_nib & 0xFF) as u8;
        hi_nib_hi_h[nibble as usize] = (high_result_hi_nib >> 8) as u8;
    }

    let tbl_lo_nib_lo = u8x16::from_array(lo_nib_lo);
    let tbl_lo_nib_hi = u8x16::from_array(lo_nib_hi);
    let tbl_hi_nib_lo = u8x16::from_array(hi_nib_lo);
    let tbl_hi_nib_hi = u8x16::from_array(hi_nib_hi);

    let tbl_lo_nib_lo_h = u8x16::from_array(lo_nib_lo_h);
    let tbl_lo_nib_hi_h = u8x16::from_array(lo_nib_hi_h);
    let tbl_hi_nib_lo_h = u8x16::from_array(hi_nib_lo_h);
    let tbl_hi_nib_hi_h = u8x16::from_array(hi_nib_hi_h);

    let mask_0f = u8x16::splat(0x0F);

    // Process 16 bytes at a time
    let simd_bytes = (len / 16) * 16;
    let mut idx = 0;

    while idx < simd_bytes {
        // Load 16 input bytes
        let in_vec = u8x16::from_slice(&input[idx..idx + 16]);
        let out_vec = u8x16::from_slice(&output[idx..idx + 16]);

        // De-interleave into even and odd bytes (same approach as NEON)
        // even_bytes: bytes at positions 0,2,4,6,8,10,12,14 (low bytes of u16 words)
        // odd_bytes: bytes at positions 1,3,5,7,9,11,13,15 (high bytes of u16 words)
        let even_bytes = simd_swizzle!(in_vec, [0, 2, 4, 6, 8, 10, 12, 14, 0, 0, 0, 0, 0, 0, 0, 0]);
        let odd_bytes = simd_swizzle!(in_vec, [1, 3, 5, 7, 9, 11, 13, 15, 0, 0, 0, 0, 0, 0, 0, 0]);

        // Process even bytes with tables.low
        let even_lo_nibbles = even_bytes & mask_0f;
        let even_hi_nibbles = even_bytes >> Simd::splat(4);

        let even_result_low =
            tbl_lo_nib_lo.swizzle_dyn(even_lo_nibbles) ^ tbl_hi_nib_lo.swizzle_dyn(even_hi_nibbles);
        let even_result_high =
            tbl_lo_nib_hi.swizzle_dyn(even_lo_nibbles) ^ tbl_hi_nib_hi.swizzle_dyn(even_hi_nibbles);

        // Process odd bytes with tables.high
        let odd_lo_nibbles = odd_bytes & mask_0f;
        let odd_hi_nibbles = odd_bytes >> Simd::splat(4);

        let odd_result_low = tbl_lo_nib_lo_h.swizzle_dyn(odd_lo_nibbles)
            ^ tbl_hi_nib_lo_h.swizzle_dyn(odd_hi_nibbles);
        let odd_result_high = tbl_lo_nib_hi_h.swizzle_dyn(odd_lo_nibbles)
            ^ tbl_hi_nib_hi_h.swizzle_dyn(odd_hi_nibbles);

        // XOR even and odd results together (combine contributions from low/high bytes)
        let combined_low = even_result_low ^ odd_result_low;
        let combined_high = even_result_high ^ odd_result_high;

        // Interleave low and high bytes back together
        // This matches NEON's vzipq_u8(combined_low, combined_high).0
        // Result should be: [low0, high0, low1, high1, low2, high2, ...]
        let result = simd_swizzle!(
            combined_low,
            combined_high,
            [0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23]
        );

        // XOR with output (accumulate)
        let final_result = out_vec ^ result;

        // Store back
        final_result.copy_to_slice(&mut output[idx..idx + 16]);

        idx += 16;
    }

    // Handle remaining bytes with scalar code
    let in_words =
        std::slice::from_raw_parts(input.as_ptr().add(idx) as *const u16, (len - idx) / 2);
    let out_words =
        std::slice::from_raw_parts_mut(output.as_mut_ptr().add(idx) as *mut u16, (len - idx) / 2);
    let low = &tables.low[..];
    let high = &tables.high[..];

    for i in 0..in_words.len() {
        let in_word = in_words[i];
        let out_word = out_words[i];
        let result = low[(in_word & 0xFF) as usize] ^ high[(in_word >> 8) as usize];
        out_words[i] = out_word ^ result;
    }

    // Handle odd trailing byte
    if len % 2 == 1 {
        let last_idx = len - 1;
        let in_byte = input[last_idx];
        let out_byte = output[last_idx];
        let result_low = low[in_byte as usize];
        output[last_idx] = out_byte ^ (result_low & 0xFF) as u8;
    }
}

/// Dispatch to the best available SIMD implementation (x86_64)
#[cfg(target_arch = "x86_64")]
pub fn process_slice_multiply_add_simd(
    input: &[u8],
    output: &mut [u8],
    tables: &SplitMulTable,
    simd_level: SimdLevel,
) {
    match simd_level {
        SimdLevel::Avx2 => unsafe {
            let len = input.len().min(output.len());

            // Use PSHUFB for the bulk of the data (multiples of 32 bytes)
            if len >= 32 {
                crate::reed_solomon::simd_pshufb::process_slice_multiply_add_pshufb(
                    input, output, tables,
                );
            }

            // Handle remaining bytes (< 32 bytes) with unrolled version
            let remainder_start = (len / 32) * 32;
            if remainder_start < len {
                process_slice_multiply_add_avx2_unrolled(
                    &input[remainder_start..],
                    &mut output[remainder_start..],
                    tables,
                );
            }
        },
        SimdLevel::Ssse3 => unsafe {
            // SSSE3 has PSHUFB but only 128-bit registers, use unrolled for now
            process_slice_multiply_add_avx2_unrolled(input, output, tables);
        },
        SimdLevel::None => {
            // Caller should use scalar fallback
        }
    }
}

/// Dispatch to the best available SIMD implementation (non-x86_64)
#[cfg(not(target_arch = "x86_64"))]
pub fn process_slice_multiply_add_simd(
    _input: &[u8],
    _output: &mut [u8],
    _tables: &SplitMulTable,
    _simd_level: SimdLevel,
) {
    // SIMD not available on non-x86_64 architectures
    // Caller should use scalar fallback
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reed_solomon::galois::Galois16;
    use crate::reed_solomon::reedsolomon::build_split_mul_table;

    #[test]
    fn detect_simd_support_returns_valid_level() {
        let level = detect_simd_support();

        // Should return one of the valid enum values
        match level {
            SimdLevel::None | SimdLevel::Ssse3 | SimdLevel::Avx2 => {
                // Valid
            }
        }

        // On x86_64, should detect at least SSSE3 on modern CPUs
        #[cfg(target_arch = "x86_64")]
        {
            // Most modern x86_64 CPUs have at least SSSE3
            // But we can't assert this in CI as it depends on the runner
            println!("Detected SIMD level: {:?}", level);
        }
    }

    #[test]
    fn simd_level_enum_equality() {
        assert_eq!(SimdLevel::None, SimdLevel::None);
        assert_eq!(SimdLevel::Ssse3, SimdLevel::Ssse3);
        assert_eq!(SimdLevel::Avx2, SimdLevel::Avx2);

        assert_ne!(SimdLevel::None, SimdLevel::Ssse3);
        assert_ne!(SimdLevel::Ssse3, SimdLevel::Avx2);
    }

    #[test]
    fn process_slice_multiply_add_simd_with_none_does_nothing() {
        let input = vec![1u8, 2, 3, 4];
        let mut output = vec![5u8, 6, 7, 8];
        let tables = build_split_mul_table(Galois16::new(2));
        let original_output = output.clone();

        process_slice_multiply_add_simd(&input, &mut output, &tables, SimdLevel::None);

        // SimdLevel::None should not modify output
        assert_eq!(output, original_output);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn process_slice_multiply_add_simd_avx2_modifies_output() {
        if !is_x86_feature_detected!("avx2") {
            eprintln!("Skipping AVX2 test - not supported on this CPU");
            return;
        }

        let input = vec![0x5Au8; 64];
        let mut output = vec![0xA5u8; 64];
        let tables = build_split_mul_table(Galois16::new(7));
        let original_output = output.clone();

        process_slice_multiply_add_simd(&input, &mut output, &tables, SimdLevel::Avx2);

        // Output should be modified
        assert_ne!(output, original_output);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn process_slice_multiply_add_simd_ssse3_modifies_output() {
        if !is_x86_feature_detected!("ssse3") {
            eprintln!("Skipping SSSE3 test - not supported on this CPU");
            return;
        }

        let input = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut output = vec![10u8, 20, 30, 40, 50, 60, 70, 80];
        let tables = build_split_mul_table(Galois16::new(3));
        let original_output = output.clone();

        process_slice_multiply_add_simd(&input, &mut output, &tables, SimdLevel::Ssse3);

        // Output should be modified
        assert_ne!(output, original_output);
    }

    #[test]
    fn process_slice_multiply_add_simd_empty_buffers() {
        let input: Vec<u8> = vec![];
        let mut output: Vec<u8> = vec![];
        let tables = build_split_mul_table(Galois16::new(1));

        // Should not panic
        process_slice_multiply_add_simd(&input, &mut output, &tables, SimdLevel::None);
    }

    #[test]
    fn process_slice_multiply_add_simd_small_buffer() {
        // Buffer smaller than SIMD threshold (< 32 bytes)
        let input = vec![1u8, 2, 3, 4];
        let mut output = vec![0u8; 4];
        let tables = build_split_mul_table(Galois16::new(2));

        let level = detect_simd_support();

        // Should not panic even with small buffers
        process_slice_multiply_add_simd(&input, &mut output, &tables, level);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn process_slice_multiply_add_avx2_unrolled_basic() {
        if !is_x86_feature_detected!("avx2") {
            eprintln!("Skipping AVX2 unrolled test - not supported");
            return;
        }

        let input = vec![1u8; 64];
        let mut output = vec![0u8; 64];
        let tables = build_split_mul_table(Galois16::new(5));

        unsafe {
            process_slice_multiply_add_avx2_unrolled(&input, &mut output, &tables);
        }

        // Output should be non-zero after processing
        assert!(output.iter().any(|&b| b != 0));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn process_slice_multiply_add_avx2_unrolled_accumulates() {
        if !is_x86_feature_detected!("avx2") {
            eprintln!("Skipping AVX2 accumulate test - not supported");
            return;
        }

        let input = vec![1u8, 0, 2, 0];
        let mut output = vec![3u8, 0, 4, 0];
        let tables = build_split_mul_table(Galois16::new(2));
        let original_output = output.clone();

        unsafe {
            process_slice_multiply_add_avx2_unrolled(&input, &mut output, &tables);
        }

        // Output should have changed (XOR accumulated)
        assert_ne!(output, original_output);
    }
}
