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
/// Requires AVX2 CPU support. Caller must ensure CPU has AVX2 before calling.
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

    let in_ptr = input.as_ptr() as *const u16;
    let out_ptr = output.as_mut_ptr() as *mut u16;
    let low_ptr = tables.low.as_ptr();
    let high_ptr = tables.high.as_ptr();

    // Process 32 words at a time (64 bytes) for maximum AVX2 utilization
    let avx_words = (num_words / 32) * 32;
    let mut idx = 0;

    // Hyper-aggressive unrolling: 32 words per iteration
    while idx < avx_words {
        // Load 32 input words in batches of 16
        let i0 = *in_ptr.add(idx); let i1 = *in_ptr.add(idx + 1);
        let i2 = *in_ptr.add(idx + 2); let i3 = *in_ptr.add(idx + 3);
        let i4 = *in_ptr.add(idx + 4); let i5 = *in_ptr.add(idx + 5);
        let i6 = *in_ptr.add(idx + 6); let i7 = *in_ptr.add(idx + 7);
        let i8 = *in_ptr.add(idx + 8); let i9 = *in_ptr.add(idx + 9);
        let i10 = *in_ptr.add(idx + 10); let i11 = *in_ptr.add(idx + 11);
        let i12 = *in_ptr.add(idx + 12); let i13 = *in_ptr.add(idx + 13);
        let i14 = *in_ptr.add(idx + 14); let i15 = *in_ptr.add(idx + 15);
        
        let i16 = *in_ptr.add(idx + 16); let i17 = *in_ptr.add(idx + 17);
        let i18 = *in_ptr.add(idx + 18); let i19 = *in_ptr.add(idx + 19);
        let i20 = *in_ptr.add(idx + 20); let i21 = *in_ptr.add(idx + 21);
        let i22 = *in_ptr.add(idx + 22); let i23 = *in_ptr.add(idx + 23);
        let i24 = *in_ptr.add(idx + 24); let i25 = *in_ptr.add(idx + 25);
        let i26 = *in_ptr.add(idx + 26); let i27 = *in_ptr.add(idx + 27);
        let i28 = *in_ptr.add(idx + 28); let i29 = *in_ptr.add(idx + 29);
        let i30 = *in_ptr.add(idx + 30); let i31 = *in_ptr.add(idx + 31);

        // Perform lookups and XOR (compiler will pipeline these heavily)
        let r0 = *out_ptr.add(idx) ^ (*low_ptr.add((i0 & 0xFF) as usize) ^ *high_ptr.add((i0 >> 8) as usize));
        let r1 = *out_ptr.add(idx + 1) ^ (*low_ptr.add((i1 & 0xFF) as usize) ^ *high_ptr.add((i1 >> 8) as usize));
        let r2 = *out_ptr.add(idx + 2) ^ (*low_ptr.add((i2 & 0xFF) as usize) ^ *high_ptr.add((i2 >> 8) as usize));
        let r3 = *out_ptr.add(idx + 3) ^ (*low_ptr.add((i3 & 0xFF) as usize) ^ *high_ptr.add((i3 >> 8) as usize));
        let r4 = *out_ptr.add(idx + 4) ^ (*low_ptr.add((i4 & 0xFF) as usize) ^ *high_ptr.add((i4 >> 8) as usize));
        let r5 = *out_ptr.add(idx + 5) ^ (*low_ptr.add((i5 & 0xFF) as usize) ^ *high_ptr.add((i5 >> 8) as usize));
        let r6 = *out_ptr.add(idx + 6) ^ (*low_ptr.add((i6 & 0xFF) as usize) ^ *high_ptr.add((i6 >> 8) as usize));
        let r7 = *out_ptr.add(idx + 7) ^ (*low_ptr.add((i7 & 0xFF) as usize) ^ *high_ptr.add((i7 >> 8) as usize));
        let r8 = *out_ptr.add(idx + 8) ^ (*low_ptr.add((i8 & 0xFF) as usize) ^ *high_ptr.add((i8 >> 8) as usize));
        let r9 = *out_ptr.add(idx + 9) ^ (*low_ptr.add((i9 & 0xFF) as usize) ^ *high_ptr.add((i9 >> 8) as usize));
        let r10 = *out_ptr.add(idx + 10) ^ (*low_ptr.add((i10 & 0xFF) as usize) ^ *high_ptr.add((i10 >> 8) as usize));
        let r11 = *out_ptr.add(idx + 11) ^ (*low_ptr.add((i11 & 0xFF) as usize) ^ *high_ptr.add((i11 >> 8) as usize));
        let r12 = *out_ptr.add(idx + 12) ^ (*low_ptr.add((i12 & 0xFF) as usize) ^ *high_ptr.add((i12 >> 8) as usize));
        let r13 = *out_ptr.add(idx + 13) ^ (*low_ptr.add((i13 & 0xFF) as usize) ^ *high_ptr.add((i13 >> 8) as usize));
        let r14 = *out_ptr.add(idx + 14) ^ (*low_ptr.add((i14 & 0xFF) as usize) ^ *high_ptr.add((i14 >> 8) as usize));
        let r15 = *out_ptr.add(idx + 15) ^ (*low_ptr.add((i15 & 0xFF) as usize) ^ *high_ptr.add((i15 >> 8) as usize));
        
        let r16 = *out_ptr.add(idx + 16) ^ (*low_ptr.add((i16 & 0xFF) as usize) ^ *high_ptr.add((i16 >> 8) as usize));
        let r17 = *out_ptr.add(idx + 17) ^ (*low_ptr.add((i17 & 0xFF) as usize) ^ *high_ptr.add((i17 >> 8) as usize));
        let r18 = *out_ptr.add(idx + 18) ^ (*low_ptr.add((i18 & 0xFF) as usize) ^ *high_ptr.add((i18 >> 8) as usize));
        let r19 = *out_ptr.add(idx + 19) ^ (*low_ptr.add((i19 & 0xFF) as usize) ^ *high_ptr.add((i19 >> 8) as usize));
        let r20 = *out_ptr.add(idx + 20) ^ (*low_ptr.add((i20 & 0xFF) as usize) ^ *high_ptr.add((i20 >> 8) as usize));
        let r21 = *out_ptr.add(idx + 21) ^ (*low_ptr.add((i21 & 0xFF) as usize) ^ *high_ptr.add((i21 >> 8) as usize));
        let r22 = *out_ptr.add(idx + 22) ^ (*low_ptr.add((i22 & 0xFF) as usize) ^ *high_ptr.add((i22 >> 8) as usize));
        let r23 = *out_ptr.add(idx + 23) ^ (*low_ptr.add((i23 & 0xFF) as usize) ^ *high_ptr.add((i23 >> 8) as usize));
        let r24 = *out_ptr.add(idx + 24) ^ (*low_ptr.add((i24 & 0xFF) as usize) ^ *high_ptr.add((i24 >> 8) as usize));
        let r25 = *out_ptr.add(idx + 25) ^ (*low_ptr.add((i25 & 0xFF) as usize) ^ *high_ptr.add((i25 >> 8) as usize));
        let r26 = *out_ptr.add(idx + 26) ^ (*low_ptr.add((i26 & 0xFF) as usize) ^ *high_ptr.add((i26 >> 8) as usize));
        let r27 = *out_ptr.add(idx + 27) ^ (*low_ptr.add((i27 & 0xFF) as usize) ^ *high_ptr.add((i27 >> 8) as usize));
        let r28 = *out_ptr.add(idx + 28) ^ (*low_ptr.add((i28 & 0xFF) as usize) ^ *high_ptr.add((i28 >> 8) as usize));
        let r29 = *out_ptr.add(idx + 29) ^ (*low_ptr.add((i29 & 0xFF) as usize) ^ *high_ptr.add((i29 >> 8) as usize));
        let r30 = *out_ptr.add(idx + 30) ^ (*low_ptr.add((i30 & 0xFF) as usize) ^ *high_ptr.add((i30 >> 8) as usize));
        let r31 = *out_ptr.add(idx + 31) ^ (*low_ptr.add((i31 & 0xFF) as usize) ^ *high_ptr.add((i31 >> 8) as usize));

        // Store all results
        *out_ptr.add(idx) = r0; *out_ptr.add(idx + 1) = r1;
        *out_ptr.add(idx + 2) = r2; *out_ptr.add(idx + 3) = r3;
        *out_ptr.add(idx + 4) = r4; *out_ptr.add(idx + 5) = r5;
        *out_ptr.add(idx + 6) = r6; *out_ptr.add(idx + 7) = r7;
        *out_ptr.add(idx + 8) = r8; *out_ptr.add(idx + 9) = r9;
        *out_ptr.add(idx + 10) = r10; *out_ptr.add(idx + 11) = r11;
        *out_ptr.add(idx + 12) = r12; *out_ptr.add(idx + 13) = r13;
        *out_ptr.add(idx + 14) = r14; *out_ptr.add(idx + 15) = r15;
        *out_ptr.add(idx + 16) = r16; *out_ptr.add(idx + 17) = r17;
        *out_ptr.add(idx + 18) = r18; *out_ptr.add(idx + 19) = r19;
        *out_ptr.add(idx + 20) = r20; *out_ptr.add(idx + 21) = r21;
        *out_ptr.add(idx + 22) = r22; *out_ptr.add(idx + 23) = r23;
        *out_ptr.add(idx + 24) = r24; *out_ptr.add(idx + 25) = r25;
        *out_ptr.add(idx + 26) = r26; *out_ptr.add(idx + 27) = r27;
        *out_ptr.add(idx + 28) = r28; *out_ptr.add(idx + 29) = r29;
        *out_ptr.add(idx + 30) = r30; *out_ptr.add(idx + 31) = r31;

        idx += 32;
    }

    // Handle remaining words with scalar code
    while idx < num_words {
        let in_word = *in_ptr.add(idx);
        let out_word = *out_ptr.add(idx);
        let result = *low_ptr.add((in_word & 0xFF) as usize) 
                   ^ *high_ptr.add((in_word >> 8) as usize);
        *out_ptr.add(idx) = out_word ^ result;
        idx += 1;
    }

    // Handle odd trailing byte if any
    if len % 2 == 1 {
        let last_idx = len - 1;
        let in_byte = *input.get_unchecked(last_idx);
        let out_byte = *output.get_unchecked(last_idx);
        let result_low = *low_ptr.add(in_byte as usize);
        *output.get_unchecked_mut(last_idx) = out_byte ^ (result_low & 0xFF) as u8;
    }
}

/// Dispatch to the best available SIMD implementation
pub fn process_slice_multiply_add_simd(
    input: &[u8],
    output: &mut [u8],
    tables: &SplitMulTable,
    simd_level: SimdLevel,
) {
    match simd_level {
        #[cfg(target_arch = "x86_64")]
        SimdLevel::Avx2 => unsafe {
            let len = input.len().min(output.len());
            
            // Use PSHUFB for the bulk of the data (multiples of 32 bytes)
            if len >= 32 {
                crate::reed_solomon::simd_pshufb::process_slice_multiply_add_pshufb(
                    input, output, tables
                );
            }
            
            // Handle remaining bytes (< 32 bytes) with unrolled version
            let remainder_start = (len / 32) * 32;
            if remainder_start < len {
                process_slice_multiply_add_avx2_unrolled(
                    &input[remainder_start..],
                    &mut output[remainder_start..],
                    tables
                );
            }
        },
        #[cfg(target_arch = "x86_64")]
        SimdLevel::Ssse3 => unsafe {
            // SSSE3 has PSHUFB but only 128-bit registers, use unrolled for now
            process_slice_multiply_add_avx2_unrolled(input, output, tables);
        },
        SimdLevel::None => {
            // Caller should use scalar fallback
        }
    }
}
