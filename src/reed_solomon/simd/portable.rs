//! Portable SIMD implementation using `std::simd` for cross-platform support
//!
//! Uses the same nibble-based table lookup strategy as PSHUFB and NEON,
//! but with portable_simd swizzle operations for platform independence.
//!
//! # Performance Note
//! On x86_64, this implementation is typically slower than the scalar fallback.
//! On ARM64, it achieves performance comparable to NEON.

use super::super::reedsolomon::SplitMulTable;
use super::common::{build_nibble_tables, process_slice_multiply_add_scalar};

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

    // Build nibble lookup tables using common function
    let nibbles_low = build_nibble_tables(&tables.low);
    let nibbles_high = build_nibble_tables(&tables.high);

    // Load into SIMD vectors
    let tbl_lo_nib_lo = u8x16::from_array(nibbles_low.lo_nib_lo_byte);
    let tbl_lo_nib_hi = u8x16::from_array(nibbles_low.lo_nib_hi_byte);
    let tbl_hi_nib_lo = u8x16::from_array(nibbles_low.hi_nib_lo_byte);
    let tbl_hi_nib_hi = u8x16::from_array(nibbles_low.hi_nib_hi_byte);

    let tbl_lo_nib_lo_h = u8x16::from_array(nibbles_high.lo_nib_lo_byte);
    let tbl_lo_nib_hi_h = u8x16::from_array(nibbles_high.lo_nib_hi_byte);
    let tbl_hi_nib_lo_h = u8x16::from_array(nibbles_high.hi_nib_lo_byte);
    let tbl_hi_nib_hi_h = u8x16::from_array(nibbles_high.hi_nib_hi_byte);

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

    // Handle remaining bytes with scalar fallback
    if idx < len {
        process_slice_multiply_add_scalar(&input[idx..], &mut output[idx..], tables);
    }
}
