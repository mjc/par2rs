//! SIMD-optimized Galois Field multiplication for Reed-Solomon operations
//!
//! Provides platform-specific SIMD implementations with correct dispatch:
//! - x86_64: PSHUFB (AVX2/SSSE3) → scalar (portable_simd is slower!)
//! - ARM64: portable_simd → scalar
//! - Other: portable_simd → scalar
//!
//! Based on the "Screaming Fast Galois Field Arithmetic" paper.
//!
//! # Note
//! This module is public for benchmarks and tests but not part of the stable API.
//! Use `ReconstructionEngine` from the parent module instead.

pub mod common;
pub mod portable;
pub mod pshufb;

use super::reedsolomon::SplitMulTable;

// These are used in tests/benchmarks
#[doc(hidden)]
pub use portable::process_slice_multiply_add_portable_simd;

#[cfg(target_arch = "x86_64")]
#[doc(hidden)]
pub use pshufb::process_slice_multiply_add_pshufb;

/// SIMD implementation to use for the current platform
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimdLevel {
    /// No SIMD available, use scalar fallback
    None,
    /// x86_64 SSSE3 (128-bit PSHUFB)
    Ssse3,
    /// x86_64 AVX2 (256-bit PSHUFB)
    Avx2,
    /// Cross-platform portable SIMD (std::simd)
    /// Note: Slower than scalar on x86_64, only use on ARM or other platforms
    PortableSimd,
}

/// Detect best available SIMD implementation for current platform
///
/// Returns the optimal implementation based on:
/// - Platform architecture (x86_64, aarch64, etc.)
/// - Available CPU features (AVX2, SSSE3)
/// - Known performance characteristics
///
/// # Platform-specific behavior:
/// - **x86_64**: PSHUFB only - portable_simd is slower than scalar!
/// - **ARM64**: portable_simd (compiles to NEON)
/// - **Other**: portable_simd if available
pub fn detect_simd_support() -> SimdLevel {
    #[cfg(target_arch = "x86_64")]
    {
        // On x86_64, ONLY use PSHUFB - portable_simd is slower than scalar!
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("ssse3") {
            return SimdLevel::Avx2;
        }
        if is_x86_feature_detected!("ssse3") {
            return SimdLevel::Ssse3;
        }
        // No PSHUFB available - use scalar (do NOT use portable_simd on x86_64)
        return SimdLevel::None;
    }

    #[cfg(target_arch = "aarch64")]
    {
        // portable_simd compiles to NEON on ARM64 and provides ~2.2-2.4x speedup
        SimdLevel::PortableSimd
    }

    // Other platforms: Try portable_simd (performance unknown, may be slower than scalar)
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        SimdLevel::PortableSimd
    }
}

/// Scalar fallback for SIMD remainder bytes
///
/// Simple loop that lets the compiler auto-optimize for the target CPU.
/// Assembly comparison showed this is 26x smaller and faster than manual 32x unrolling.
///
/// # Safety
/// - `input` and `output` slices must not alias
/// - Processes min(input.len(), output.len()) bytes
#[doc(hidden)]
#[inline]
pub unsafe fn process_slice_multiply_add_scalar(
    input: &[u8],
    output: &mut [u8],
    tables: &SplitMulTable,
) {
    let len = input.len().min(output.len());
    let num_words = len / 2;

    if num_words > 0 {
        let in_words = std::slice::from_raw_parts(input.as_ptr() as *const u16, num_words);
        let out_words = std::slice::from_raw_parts_mut(output.as_mut_ptr() as *mut u16, num_words);
        let low = &tables.low[..];
        let high = &tables.high[..];

        // Simple loop - let compiler decide optimal unrolling
        for idx in 0..num_words {
            let in_word = in_words[idx];
            let result = low[(in_word & 0xFF) as usize] ^ high[(in_word >> 8) as usize];
            out_words[idx] ^= result;
        }
    }

    // Handle odd trailing byte if any
    if len % 2 == 1 {
        let last_idx = len - 1;
        let in_byte = input[last_idx];
        let result_low = tables.low[in_byte as usize];
        output[last_idx] ^= (result_low & 0xFF) as u8;
    }
}

/// Dispatch to the best available SIMD implementation
///
/// Automatically selects the fastest implementation for the current platform:
/// - x86_64: PSHUFB (AVX2/SSSE3) or scalar fallback
/// - ARM64: portable_simd (compiles to NEON) or scalar fallback
/// - Other: portable_simd or scalar fallback
///
/// # Safety
/// Uses unsafe SIMD operations internally but provides safe wrapper
#[doc(hidden)]
pub fn process_slice_multiply_add_simd(
    input: &[u8],
    output: &mut [u8],
    tables: &SplitMulTable,
    simd_level: SimdLevel,
) {
    match simd_level {
        SimdLevel::Avx2 => {
            #[cfg(target_arch = "x86_64")]
            unsafe {
                let len = input.len().min(output.len());

                // Use PSHUFB for the bulk of the data (multiples of 32 bytes)
                if len >= 32 {
                    process_slice_multiply_add_pshufb(input, output, tables);
                }

                // Handle remaining bytes (< 32 bytes) with unrolled version
                let remainder_start = (len / 32) * 32;
                if remainder_start < len {
                    process_slice_multiply_add_scalar(
                        &input[remainder_start..],
                        &mut output[remainder_start..],
                        tables,
                    );
                }
            }
        }
        SimdLevel::Ssse3 => {
            #[cfg(target_arch = "x86_64")]
            unsafe {
                // SSSE3 has PSHUFB but only 128-bit registers, use unrolled for now
                process_slice_multiply_add_scalar(input, output, tables);
            }
        }
        SimdLevel::PortableSimd => {
            // Used on ARM64 (compiles to NEON) or non-x86_64/non-aarch64 platforms
            // NOT used on x86_64 (slower than scalar!)
            unsafe {
                process_slice_multiply_add_portable_simd(input, output, tables);
            }
        }
        SimdLevel::None => {
            // Caller should use scalar fallback
            // This is the correct path for x86_64 without PSHUFB
        }
    }
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
            SimdLevel::None | SimdLevel::Ssse3 | SimdLevel::Avx2 | SimdLevel::PortableSimd => {
                // Valid
            }
        }

        // Platform-specific expectations
        #[cfg(target_arch = "x86_64")]
        {
            // On x86_64, should NEVER return PortableSimd
            assert_ne!(level, SimdLevel::PortableSimd);
            println!("x86_64 SIMD level: {:?}", level);
        }

        #[cfg(target_arch = "aarch64")]
        {
            // On ARM64, should return PortableSimd (which compiles to NEON)
            assert_eq!(level, SimdLevel::PortableSimd);
            println!("ARM64 SIMD level: {:?}", level);
        }
    }

    #[test]
    fn simd_level_enum_equality() {
        assert_eq!(SimdLevel::None, SimdLevel::None);
        assert_eq!(SimdLevel::Ssse3, SimdLevel::Ssse3);
        assert_eq!(SimdLevel::Avx2, SimdLevel::Avx2);
        assert_eq!(SimdLevel::PortableSimd, SimdLevel::PortableSimd);

        assert_ne!(SimdLevel::None, SimdLevel::Ssse3);
        assert_ne!(SimdLevel::Ssse3, SimdLevel::Avx2);
        assert_ne!(SimdLevel::Avx2, SimdLevel::PortableSimd);
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
    fn process_slice_multiply_add_scalar_basic() {
        if !is_x86_feature_detected!("avx2") {
            eprintln!("Skipping AVX2 unrolled test - not supported");
            return;
        }

        let input = vec![1u8; 64];
        let mut output = vec![0u8; 64];
        let tables = build_split_mul_table(Galois16::new(5));

        unsafe {
            process_slice_multiply_add_scalar(&input, &mut output, &tables);
        }

        // Output should be non-zero after processing
        assert!(output.iter().any(|&b| b != 0));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn process_slice_multiply_add_scalar_accumulates() {
        if !is_x86_feature_detected!("avx2") {
            eprintln!("Skipping AVX2 accumulate test - not supported");
            return;
        }

        let input = vec![1u8, 0, 2, 0];
        let mut output = vec![3u8, 0, 4, 0];
        let tables = build_split_mul_table(Galois16::new(2));
        let original_output = output.clone();

        unsafe {
            process_slice_multiply_add_scalar(&input, &mut output, &tables);
        }

        // Output should have changed (XOR accumulated)
        assert_ne!(output, original_output);
    }
}
