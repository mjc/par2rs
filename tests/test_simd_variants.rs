//! Tests for different SIMD implementation variants
//!
//! Verifies correctness of:
//! - portable_simd implementation
//! - ARM NEON implementation
//! - All implementations produce identical results to scalar reference

use par2rs::reed_solomon::simd::process_slice_multiply_add_portable_simd;
use par2rs::reed_solomon::{build_split_mul_table, Galois16, SplitMulTable};

/// Generate test multiplication tables for a non-zero coefficient
fn make_test_tables(coef: u16) -> SplitMulTable {
    build_split_mul_table(Galois16::new(coef))
}

/// Reference scalar implementation for testing
fn process_slice_multiply_add_scalar(input: &[u8], output: &mut [u8], tables: &SplitMulTable) {
    let len = input.len().min(output.len());
    let in_words = unsafe { std::slice::from_raw_parts(input.as_ptr() as *const u16, len / 2) };
    let out_words =
        unsafe { std::slice::from_raw_parts_mut(output.as_mut_ptr() as *mut u16, len / 2) };

    for i in 0..in_words.len() {
        let in_word = in_words[i];
        let out_word = out_words[i];
        let result = tables.low[(in_word & 0xFF) as usize] ^ tables.high[(in_word >> 8) as usize];
        out_words[i] = out_word ^ result;
    }

    // Handle odd trailing byte
    if len % 2 == 1 {
        let last_idx = len - 1;
        let in_byte = input[last_idx];
        let out_byte = output[last_idx];
        let result_low = tables.low[in_byte as usize];
        output[last_idx] = out_byte ^ (result_low & 0xFF) as u8;
    }
}

#[test]
fn test_portable_simd_correctness() {
    // Test various sizes: aligned (16, 32, 64) and unaligned (17, 31, 65)
    let sizes = vec![16, 17, 31, 32, 63, 64, 100, 127, 128, 256];
    let coef = 0x1234u16; // Arbitrary non-zero coefficient
    let tables = make_test_tables(coef);

    for size in sizes {
        // Create test input data
        let mut input = vec![0u8; size];
        for (i, byte) in input.iter_mut().enumerate() {
            *byte = (i * 37 + 13) as u8; // Pseudo-random pattern
        }

        // Process with scalar reference
        let mut output_scalar = vec![0u8; size];
        for (i, byte) in output_scalar.iter_mut().enumerate() {
            *byte = (i * 53 + 7) as u8; // Different initial pattern
        }
        process_slice_multiply_add_scalar(&input, &mut output_scalar, &tables);

        // Process with portable_simd
        let mut output_simd = vec![0u8; size];
        for (i, byte) in output_simd.iter_mut().enumerate() {
            *byte = (i * 53 + 7) as u8; // Same initial pattern
        }
        unsafe {
            process_slice_multiply_add_portable_simd(&input, &mut output_simd, &tables);
        }

        // Compare results
        assert_eq!(
            output_scalar,
            output_simd,
            "portable_simd mismatch at size {}: expected {:?}, got {:?}",
            size,
            &output_scalar[..],
            &output_simd[..]
        );
    }
}

#[test]
fn test_portable_simd_accumulation() {
    // Test that XOR accumulation works correctly across multiple calls
    let size = 128;
    let input1 = vec![0xAAu8; size];
    let input2 = vec![0x55u8; size];
    let mut output = vec![0u8; size];

    let coef1 = 0x1234u16;
    let coef2 = 0x5678u16;
    let tables1 = make_test_tables(coef1);
    let tables2 = make_test_tables(coef2);

    // Apply first operation
    unsafe {
        process_slice_multiply_add_portable_simd(&input1, &mut output, &tables1);
    }

    // Apply second operation (accumulate)
    unsafe {
        process_slice_multiply_add_portable_simd(&input2, &mut output, &tables2);
    }

    // Verify against scalar reference
    let mut output_scalar = vec![0u8; size];
    process_slice_multiply_add_scalar(&input1, &mut output_scalar, &tables1);
    process_slice_multiply_add_scalar(&input2, &mut output_scalar, &tables2);

    assert_eq!(output, output_scalar, "Accumulation mismatch");
}

#[test]
fn test_portable_simd_zero_coefficient() {
    // When coefficient is zero, output should remain unchanged
    let size = 64;
    let input = vec![0xFFu8; size];
    let mut output = vec![0xAAu8; size];
    let expected = output.clone();

    let tables = make_test_tables(0); // Zero coefficient

    unsafe {
        process_slice_multiply_add_portable_simd(&input, &mut output, &tables);
    }

    assert_eq!(output, expected, "Output changed with zero coefficient");
}

#[test]
fn test_portable_simd_mismatched_lengths() {
    // Test that function handles different input/output lengths correctly
    let input = vec![0x12u8; 100];
    let mut output = vec![0x34u8; 50]; // Shorter output
    let tables = make_test_tables(0x1234);

    let mut output_scalar = output.clone();
    process_slice_multiply_add_scalar(&input, &mut output_scalar, &tables);

    unsafe {
        process_slice_multiply_add_portable_simd(&input, &mut output, &tables);
    }

    assert_eq!(output, output_scalar, "Mismatched length handling differs");
}

// ARM NEON tests (only compiled on ARM64)
#[cfg(target_arch = "aarch64")]
mod neon_tests {
    use super::*;
    use par2rs::reed_solomon::simd_neon::process_slice_multiply_add_neon;

    #[test]
    fn test_neon_correctness() {
        let sizes = vec![16, 17, 31, 32, 63, 64, 100, 127, 128, 256];
        let coef = 0x1234u16;
        let tables = make_test_tables(coef);

        for size in sizes {
            let mut input = vec![0u8; size];
            for (i, byte) in input.iter_mut().enumerate() {
                *byte = (i * 37 + 13) as u8;
            }

            // Scalar reference
            let mut output_scalar = vec![0u8; size];
            for (i, byte) in output_scalar.iter_mut().enumerate() {
                *byte = (i * 53 + 7) as u8;
            }
            process_slice_multiply_add_scalar(&input, &mut output_scalar, &tables);

            // NEON implementation
            let mut output_neon = vec![0u8; size];
            for (i, byte) in output_neon.iter_mut().enumerate() {
                *byte = (i * 53 + 7) as u8;
            }
            unsafe {
                process_slice_multiply_add_neon(&input, &mut output_neon, &tables);
            }

            assert_eq!(output_scalar, output_neon, "NEON mismatch at size {}", size);
        }
    }

    #[test]
    fn test_neon_accumulation() {
        let size = 128;
        let input1 = vec![0xAAu8; size];
        let input2 = vec![0x55u8; size];
        let mut output = vec![0u8; size];

        let coef1 = 0x1234u16;
        let coef2 = 0x5678u16;
        let tables1 = make_test_tables(coef1);
        let tables2 = make_test_tables(coef2);

        unsafe {
            process_slice_multiply_add_neon(&input1, &mut output, &tables1);
            process_slice_multiply_add_neon(&input2, &mut output, &tables2);
        }

        let mut output_scalar = vec![0u8; size];
        process_slice_multiply_add_scalar(&input1, &mut output_scalar, &tables1);
        process_slice_multiply_add_scalar(&input2, &mut output_scalar, &tables2);

        assert_eq!(output, output_scalar, "NEON accumulation mismatch");
    }
}
