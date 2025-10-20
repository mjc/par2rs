//! ARM NEON SIMD optimizations for Galois Field multiplication
//!
//! Uses ARM NEON table lookup instructions (vqtbl1q_u8) for parallel GF(2^16)
//! multiplication. Similar technique to x86 PSHUFB but using ARM intrinsics.
//!
//! Performance on Apple M1: vqtbl1q_u8 has ~1 cycle latency, comparable to PSHUFB.

#[cfg(target_arch = "aarch64")]
use super::reedsolomon::SplitMulTable;

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

/// Build nibble lookup tables for NEON table lookups
///
/// Takes a 256-entry u16 table and splits it into 4 tables of 16 bytes each:
/// - Low nibble (0-15) → result low byte
/// - Low nibble (0-15) → result high byte  
/// - High nibble (0-15) → result low byte
/// - High nibble (0-15) → result high byte
#[cfg(target_arch = "aarch64")]
fn build_neon_tables(table: &[u16; 256]) -> ([u8; 16], [u8; 16], [u8; 16], [u8; 16]) {
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

/// ARM NEON implementation of GF(2^16) multiply-add operation
///
/// Uses NEON vtbl (table lookup) instructions for fast GF(2^16) multiplication.
/// Processes 16 bytes at a time with NEON SIMD operations.
///
/// # Safety
/// - Requires ARM NEON support (all ARM64 CPUs have this)
/// - `input` and `output` slices must not alias
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub unsafe fn process_slice_multiply_add_neon(
    input: &[u8],
    output: &mut [u8],
    tables: &SplitMulTable,
) {
    let len = input.len().min(output.len());

    // Need at least 16 bytes for NEON SIMD
    if len < 16 {
        // Fall back to scalar for small buffers
        process_scalar(input, output, tables);
        return;
    }

    // Build NEON lookup tables
    let (lo_nib_lo, lo_nib_hi, hi_nib_lo, hi_nib_hi) = build_neon_tables(&tables.low);
    let (lo_nib_lo_h, lo_nib_hi_h, hi_nib_lo_h, hi_nib_hi_h) = build_neon_tables(&tables.high);

    // Load lookup tables into NEON registers
    let tbl_lo_nib_lo = vld1q_u8(lo_nib_lo.as_ptr());
    let tbl_lo_nib_hi = vld1q_u8(lo_nib_hi.as_ptr());
    let tbl_hi_nib_lo = vld1q_u8(hi_nib_lo.as_ptr());
    let tbl_hi_nib_hi = vld1q_u8(hi_nib_hi.as_ptr());

    let tbl_lo_nib_lo_h = vld1q_u8(lo_nib_lo_h.as_ptr());
    let tbl_lo_nib_hi_h = vld1q_u8(lo_nib_hi_h.as_ptr());
    let tbl_hi_nib_lo_h = vld1q_u8(hi_nib_lo_h.as_ptr());
    let tbl_hi_nib_hi_h = vld1q_u8(hi_nib_hi_h.as_ptr());

    // Nibble mask (0x0F)
    let mask = vdupq_n_u8(0x0F);

    // Process 16 bytes at a time
    let simd_bytes = (len / 16) * 16;
    let mut idx = 0;

    while idx < simd_bytes {
        // Load 16 input bytes
        let in_vec = vld1q_u8(input.as_ptr().add(idx));
        let out_vec = vld1q_u8(output.as_ptr().add(idx));

        // De-interleave into even and odd bytes
        // even_bytes: bytes at positions 0,2,4,6,8,10,12,14 (low bytes of u16 words)
        // odd_bytes: bytes at positions 1,3,5,7,9,11,13,15 (high bytes of u16 words)
        let deinterleaved = vuzpq_u8(in_vec, in_vec);
        let even_bytes = deinterleaved.0;
        let odd_bytes = deinterleaved.1;

        // Process even bytes with tables.low
        let even_lo_nibbles = vandq_u8(even_bytes, mask);
        let even_hi_nibbles = vshrq_n_u8(even_bytes, 4);

        let even_lo_result = vqtbl1q_u8(tbl_lo_nib_lo, even_lo_nibbles);
        let even_hi_result = vqtbl1q_u8(tbl_hi_nib_lo, even_hi_nibbles);
        let even_result_low = veorq_u8(even_lo_result, even_hi_result);

        let even_lo_result_hi = vqtbl1q_u8(tbl_lo_nib_hi, even_lo_nibbles);
        let even_hi_result_hi = vqtbl1q_u8(tbl_hi_nib_hi, even_hi_nibbles);
        let even_result_high = veorq_u8(even_lo_result_hi, even_hi_result_hi);

        // Process odd bytes with tables.high
        let odd_lo_nibbles = vandq_u8(odd_bytes, mask);
        let odd_hi_nibbles = vshrq_n_u8(odd_bytes, 4);

        let odd_lo_result = vqtbl1q_u8(tbl_lo_nib_lo_h, odd_lo_nibbles);
        let odd_hi_result = vqtbl1q_u8(tbl_hi_nib_lo_h, odd_hi_nibbles);
        let odd_result_low = veorq_u8(odd_lo_result, odd_hi_result);

        let odd_lo_result_hi = vqtbl1q_u8(tbl_lo_nib_hi_h, odd_lo_nibbles);
        let odd_hi_result_hi = vqtbl1q_u8(tbl_hi_nib_hi_h, odd_hi_nibbles);
        let odd_result_high = veorq_u8(odd_lo_result_hi, odd_hi_result_hi);

        // XOR even_result and odd_result together (combine low and high byte contributions for each u16 word)
        let combined_low = veorq_u8(even_result_low, odd_result_low);
        let combined_high = veorq_u8(even_result_high, odd_result_high);

        // Interleave low and high bytes back together
        let interleaved = vzipq_u8(combined_low, combined_high);
        let result = interleaved.0;

        // XOR with output (accumulate)
        let final_result = veorq_u8(out_vec, result);

        // Store result
        vst1q_u8(output.as_mut_ptr().add(idx), final_result);

        idx += 16;
    }

    // Handle remaining bytes with scalar code
    if idx < len {
        process_scalar(&input[idx..], &mut output[idx..], tables);
    }
}

/// Scalar fallback for small buffers or remaining bytes
#[cfg(target_arch = "aarch64")]
unsafe fn process_scalar(input: &[u8], output: &mut [u8], tables: &SplitMulTable) {
    let len = input.len().min(output.len());
    let in_words = std::slice::from_raw_parts(input.as_ptr() as *const u16, len / 2);
    let out_words = std::slice::from_raw_parts_mut(output.as_mut_ptr() as *mut u16, len / 2);
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

#[cfg(test)]
#[cfg(target_arch = "aarch64")]
mod tests {
    use super::*;
    use crate::reed_solomon::galois::Galois16;
    use crate::reed_solomon::reedsolomon::build_split_mul_table;

    #[test]
    fn neon_multiply_add_basic() {
        let input = vec![
            0x12u8, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
            0x77, 0x88,
        ];
        let mut output = vec![0u8; 16];
        let tables = build_split_mul_table(Galois16::new(7));

        unsafe {
            process_slice_multiply_add_neon(&input, &mut output, &tables);
        }

        // Output should be non-zero after processing
        assert!(output.iter().any(|&b| b != 0));
    }
}
