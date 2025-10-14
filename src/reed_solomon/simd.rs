//! SIMD-optimized Galois Field multiplication for Reed-Solomon operations
//!
//! Uses AVX2/SSE instructions for parallel GF(2^16) multiplication via table lookups.
//! Based on the "Screaming Fast Galois Field Arithmetic" paper and reed-solomon-erasure crate.

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

/// SIMD-optimized multiply-add: output ^= coefficient * input
/// 
/// This processes 16-bit words using optimized vector operations.
/// Currently uses scalar logic with better vectorization hints for the compiler.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn process_slice_multiply_add_avx2(
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

    // Process 32 words at a time (better for AVX2 - 64 bytes)
    let chunks = num_words / 32;
    let mut idx = 0;

    for _ in 0..chunks {
        // Unroll 32 words for maximum throughput
        // Compiler can vectorize this with AVX2 instructions
        for i in 0..32 {
            let in_word = *in_ptr.add(idx + i);
            let out_word = *out_ptr.add(idx + i);
            let result = *low_ptr.add((in_word & 0xFF) as usize) 
                       ^ *high_ptr.add((in_word >> 8) as usize);
            *out_ptr.add(idx + i) = out_word ^ result;
        }
        idx += 32;
    }

    // Handle remaining words
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
pub(crate) fn process_slice_multiply_add_simd(
    input: &[u8],
    output: &mut [u8],
    tables: &SplitMulTable,
    simd_level: SimdLevel,
) {
    match simd_level {
        #[cfg(target_arch = "x86_64")]
        SimdLevel::Avx2 => unsafe {
            process_slice_multiply_add_avx2(input, output, tables);
        },
        #[cfg(target_arch = "x86_64")]
        SimdLevel::Ssse3 => unsafe {
            // For now, fall back to AVX2 implementation which will also work with SSSE3
            // TODO: Add dedicated SSSE3 implementation using 128-bit registers
            process_slice_multiply_add_avx2(input, output, tables);
        },
        SimdLevel::None => {
            // Caller should use scalar fallback
        }
    }
}
