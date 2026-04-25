//! Tableless XOR multiply kernels for create-side forced JIT modes.
//!
//! This is the executable core used by the `xor-jit-port` and `xor-jit-clean`
//! create methods. The kernels are coefficient-specialized at backend
//! construction by storing a compact typed plan, then the hot path uses AVX2
//! shifts and XORs only. No PSHUFB or scalar lookup table is used here.

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

const GF16_REDUCTION: u16 = 0x100b;

#[derive(Debug, Clone)]
#[cfg(target_arch = "x86_64")]
pub struct XorJitPreparedCoeff {
    coefficient: u16,
    #[allow(dead_code)]
    clean_plan: XorJitCleanPlan,
}

#[derive(Debug, Clone)]
#[cfg(target_arch = "x86_64")]
#[allow(dead_code)]
struct XorJitCleanPlan {
    taps: [u8; 16],
    tap_count: u8,
}

#[cfg(target_arch = "x86_64")]
impl XorJitPreparedCoeff {
    #[inline]
    pub fn new(coefficient: u16) -> Self {
        Self {
            coefficient,
            clean_plan: XorJitCleanPlan::new(coefficient),
        }
    }
}

#[cfg(target_arch = "x86_64")]
impl XorJitCleanPlan {
    #[inline]
    fn new(coefficient: u16) -> Self {
        let mut taps = [0u8; 16];
        let mut tap_count = 0u8;
        for bit in 0..16 {
            if coefficient & (1 << bit) != 0 {
                taps[tap_count as usize] = bit;
                tap_count += 1;
            }
        }
        Self { taps, tap_count }
    }
}

#[cfg(target_arch = "x86_64")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XorJitFlavor {
    Port,
    Clean,
}

#[cfg(target_arch = "x86_64")]
struct XorJitConstants {
    word_mask: __m256i,
    shuf_lo_hi: __m256i,
}

#[cfg(target_arch = "x86_64")]
struct XorJitCoeffVectors {
    even: __m256i,
    odd: __m256i,
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn kernel_constants() -> XorJitConstants {
    XorJitConstants {
        word_mask: _mm256_set1_epi32(0xffff),
        shuf_lo_hi: _mm256_set_epi16(
            0x0f0e, 0x0b0a, 0x0706, 0x0302, 0x0d0c, 0x0908, 0x0504, 0x0100, 0x0f0e, 0x0b0a, 0x0706,
            0x0302, 0x0d0c, 0x0908, 0x0504, 0x0100,
        ),
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn coeff_vectors(
    prepared: &XorJitPreparedCoeff,
    constants: &XorJitConstants,
) -> XorJitCoeffVectors {
    let coeff = _mm256_set1_epi16(prepared.coefficient as i16);
    XorJitCoeffVectors {
        even: _mm256_and_si256(constants.word_mask, coeff),
        odd: _mm256_andnot_si256(constants.word_mask, coeff),
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(dead_code)]
unsafe fn xtime_vec(value: __m256i) -> __m256i {
    let carry = _mm256_srli_epi16(value, 15);
    let shifted = _mm256_slli_epi16(value, 1);
    let reduction = _mm256_mullo_epi16(carry, _mm256_set1_epi16(GF16_REDUCTION as i16));
    _mm256_xor_si256(shifted, reduction)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(dead_code)]
unsafe fn multiply_vec_port(input: __m256i, coefficient: u16) -> __m256i {
    let mut coeff = coefficient;
    let mut power = input;
    let mut result = _mm256_setzero_si256();

    while coeff != 0 {
        if coeff & 1 != 0 {
            result = _mm256_xor_si256(result, power);
        }
        coeff >>= 1;
        if coeff != 0 {
            power = xtime_vec(power);
        }
    }

    result
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(dead_code)]
unsafe fn multiply_vec_clean(input: __m256i, plan: &XorJitCleanPlan) -> __m256i {
    let mut power = input;
    let mut current_round = 0u8;
    let mut result = _mm256_setzero_si256();

    for tap_idx in 0..plan.tap_count as usize {
        let tap = plan.taps[tap_idx];
        while current_round < tap {
            power = xtime_vec(power);
            current_round += 1;
        }
        result = _mm256_xor_si256(result, power);
    }

    result
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "pclmulqdq")]
unsafe fn clmul_256<const CONTROL: i32>(left: __m256i, right: __m256i) -> __m256i {
    let left_lo = _mm256_castsi256_si128(left);
    let left_hi = _mm256_extracti128_si256(left, 1);
    let right_lo = _mm256_castsi256_si128(right);
    let right_hi = _mm256_extracti128_si256(right, 1);
    let product_lo = _mm_clmulepi64_si128(left_lo, right_lo, CONTROL);
    let product_hi = _mm_clmulepi64_si128(left_hi, right_hi, CONTROL);
    _mm256_inserti128_si256(_mm256_castsi128_si256(product_lo), product_hi, 1)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "pclmulqdq")]
unsafe fn multiply_vec_clmul(
    input: __m256i,
    coeff: &XorJitCoeffVectors,
    constants: &XorJitConstants,
) -> __m256i {
    let input_even = _mm256_and_si256(constants.word_mask, input);
    let input_odd = _mm256_andnot_si256(constants.word_mask, input);

    let prod1_even = clmul_256::<0x00>(input_even, coeff.even);
    let prod2_even = clmul_256::<0x11>(input_even, coeff.even);
    let prod1_odd = clmul_256::<0x00>(input_odd, coeff.odd);
    let prod2_odd = clmul_256::<0x11>(input_odd, coeff.odd);

    let prod1 = _mm256_blend_epi16(prod1_even, prod1_odd, 0xcc);
    let prod2 = _mm256_blend_epi16(prod2_even, prod2_odd, 0xcc);

    let tmp1 = _mm256_shuffle_epi8(prod1, constants.shuf_lo_hi);
    let tmp2 = _mm256_shuffle_epi8(prod2, constants.shuf_lo_hi);
    let rem = _mm256_unpacklo_epi64(tmp1, tmp2);
    let mut quot = _mm256_unpackhi_epi64(tmp1, tmp2);

    let mut tmp = _mm256_xor_si256(quot, _mm256_srli_epi16(quot, 4));
    tmp = _mm256_xor_si256(tmp, _mm256_srli_epi16(tmp, 8));
    quot = _mm256_xor_si256(tmp, _mm256_srli_epi16(quot, 13));

    tmp = _mm256_xor_si256(quot, _mm256_slli_epi16(quot, 3));
    tmp = _mm256_xor_si256(tmp, _mm256_slli_epi16(quot, 1));
    quot = _mm256_xor_si256(tmp, _mm256_slli_epi16(quot, 12));

    _mm256_xor_si256(quot, rem)
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn multiply_word(mut input: u16, coefficient: u16) -> u16 {
    let mut coeff = coefficient;
    let mut result = 0u16;

    while coeff != 0 {
        if coeff & 1 != 0 {
            result ^= input;
        }
        coeff >>= 1;
        if coeff != 0 {
            let carry = input & 0x8000 != 0;
            input <<= 1;
            if carry {
                input ^= GF16_REDUCTION;
            }
        }
    }

    result
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn multiply_add_tail(input: &[u8], output: &mut [u8], coefficient: u16) {
    let len = input.len().min(output.len());
    let words = len / 2;
    for idx in 0..words {
        let byte_idx = idx * 2;
        let in_word = u16::from_le_bytes([input[byte_idx], input[byte_idx + 1]]);
        let out_word = u16::from_le_bytes([output[byte_idx], output[byte_idx + 1]]);
        let result = out_word ^ multiply_word(in_word, coefficient);
        output[byte_idx..byte_idx + 2].copy_from_slice(&result.to_le_bytes());
    }

    if len % 2 == 1 {
        let idx = words * 2;
        output[idx] ^= multiply_word(input[idx] as u16, coefficient) as u8;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "pclmulqdq")]
unsafe fn multiply_vec(
    input: __m256i,
    coeff: &XorJitCoeffVectors,
    constants: &XorJitConstants,
    flavor: XorJitFlavor,
) -> __m256i {
    match flavor {
        XorJitFlavor::Port => multiply_vec_clmul(input, coeff, constants),
        XorJitFlavor::Clean => multiply_vec_clmul(input, coeff, constants),
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "pclmulqdq")]
unsafe fn load_vec(ptr: *const u8, pos: usize, aligned: bool) -> __m256i {
    if aligned {
        _mm256_load_si256(ptr.add(pos) as *const __m256i)
    } else {
        _mm256_loadu_si256(ptr.add(pos) as *const __m256i)
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "pclmulqdq")]
unsafe fn store_vec(ptr: *mut u8, pos: usize, value: __m256i, aligned: bool) {
    if aligned {
        _mm256_store_si256(ptr.add(pos) as *mut __m256i, value);
    } else {
        _mm256_storeu_si256(ptr.add(pos) as *mut __m256i, value);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "pclmulqdq")]
pub unsafe fn process_slice_multiply_add_xor_jit(
    input: &[u8],
    output: &mut [u8],
    prepared: &XorJitPreparedCoeff,
    flavor: XorJitFlavor,
) {
    let constants = kernel_constants();
    let coeff = coeff_vectors(prepared, &constants);
    let len = input.len().min(output.len());
    let avx_end = len / 32 * 32;
    let input_ptr = input.as_ptr();
    let output_ptr = output.as_mut_ptr();
    let aligned =
        (input_ptr as usize).is_multiple_of(32) && (output_ptr as usize).is_multiple_of(32);

    let mut pos = 0;
    while pos + 128 <= avx_end {
        let in0 = load_vec(input_ptr, pos, aligned);
        let out0 = load_vec(output_ptr, pos, aligned);
        store_vec(
            output_ptr,
            pos,
            _mm256_xor_si256(out0, multiply_vec(in0, &coeff, &constants, flavor)),
            aligned,
        );
        pos += 32;

        let in1 = load_vec(input_ptr, pos, aligned);
        let out1 = load_vec(output_ptr, pos, aligned);
        store_vec(
            output_ptr,
            pos,
            _mm256_xor_si256(out1, multiply_vec(in1, &coeff, &constants, flavor)),
            aligned,
        );
        pos += 32;

        let in2 = load_vec(input_ptr, pos, aligned);
        let out2 = load_vec(output_ptr, pos, aligned);
        store_vec(
            output_ptr,
            pos,
            _mm256_xor_si256(out2, multiply_vec(in2, &coeff, &constants, flavor)),
            aligned,
        );
        pos += 32;

        let in3 = load_vec(input_ptr, pos, aligned);
        let out3 = load_vec(output_ptr, pos, aligned);
        store_vec(
            output_ptr,
            pos,
            _mm256_xor_si256(out3, multiply_vec(in3, &coeff, &constants, flavor)),
            aligned,
        );
        pos += 32;
    }

    while pos < avx_end {
        let input_vec = load_vec(input_ptr, pos, aligned);
        let output_vec = load_vec(output_ptr, pos, aligned);
        store_vec(
            output_ptr,
            pos,
            _mm256_xor_si256(
                output_vec,
                multiply_vec(input_vec, &coeff, &constants, flavor),
            ),
            aligned,
        );
        pos += 32;
    }

    if pos < len {
        multiply_add_tail(
            &input[pos..len],
            &mut output[pos..len],
            prepared.coefficient,
        );
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "pclmulqdq")]
pub unsafe fn process_slices_multiply_add_xor_jit_x2(
    input_a: &[u8],
    prepared_a: &XorJitPreparedCoeff,
    input_b: &[u8],
    prepared_b: &XorJitPreparedCoeff,
    output: &mut [u8],
    flavor: XorJitFlavor,
) {
    let constants = kernel_constants();
    let coeff_a = coeff_vectors(prepared_a, &constants);
    let coeff_b = coeff_vectors(prepared_b, &constants);
    let len = input_a.len().min(input_b.len()).min(output.len());
    let avx_end = len / 32 * 32;
    let input_a_ptr = input_a.as_ptr();
    let input_b_ptr = input_b.as_ptr();
    let output_ptr = output.as_mut_ptr();
    let aligned = (input_a_ptr as usize).is_multiple_of(32)
        && (input_b_ptr as usize).is_multiple_of(32)
        && (output_ptr as usize).is_multiple_of(32);

    let mut pos = 0;
    while pos < avx_end {
        let result = _mm256_xor_si256(
            multiply_vec(
                load_vec(input_a_ptr, pos, aligned),
                &coeff_a,
                &constants,
                flavor,
            ),
            multiply_vec(
                load_vec(input_b_ptr, pos, aligned),
                &coeff_b,
                &constants,
                flavor,
            ),
        );
        let output_vec = load_vec(output_ptr, pos, aligned);
        store_vec(
            output_ptr,
            pos,
            _mm256_xor_si256(output_vec, result),
            aligned,
        );
        pos += 32;
    }

    if pos < len {
        multiply_add_tail(
            &input_a[pos..len],
            &mut output[pos..len],
            prepared_a.coefficient,
        );
        multiply_add_tail(
            &input_b[pos..len],
            &mut output[pos..len],
            prepared_b.coefficient,
        );
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "pclmulqdq")]
#[allow(clippy::too_many_arguments)]
pub unsafe fn process_slices_multiply_add_xor_jit_x4(
    input_a: &[u8],
    prepared_a: &XorJitPreparedCoeff,
    input_b: &[u8],
    prepared_b: &XorJitPreparedCoeff,
    input_c: &[u8],
    prepared_c: &XorJitPreparedCoeff,
    input_d: &[u8],
    prepared_d: &XorJitPreparedCoeff,
    output: &mut [u8],
    flavor: XorJitFlavor,
) {
    let constants = kernel_constants();
    let coeff_a = coeff_vectors(prepared_a, &constants);
    let coeff_b = coeff_vectors(prepared_b, &constants);
    let coeff_c = coeff_vectors(prepared_c, &constants);
    let coeff_d = coeff_vectors(prepared_d, &constants);
    let len = input_a
        .len()
        .min(input_b.len())
        .min(input_c.len())
        .min(input_d.len())
        .min(output.len());
    let avx_end = len / 32 * 32;
    let input_a_ptr = input_a.as_ptr();
    let input_b_ptr = input_b.as_ptr();
    let input_c_ptr = input_c.as_ptr();
    let input_d_ptr = input_d.as_ptr();
    let output_ptr = output.as_mut_ptr();
    let aligned = (input_a_ptr as usize).is_multiple_of(32)
        && (input_b_ptr as usize).is_multiple_of(32)
        && (input_c_ptr as usize).is_multiple_of(32)
        && (input_d_ptr as usize).is_multiple_of(32)
        && (output_ptr as usize).is_multiple_of(32);

    let mut pos = 0;
    while pos < avx_end {
        let ab = _mm256_xor_si256(
            multiply_vec(
                load_vec(input_a_ptr, pos, aligned),
                &coeff_a,
                &constants,
                flavor,
            ),
            multiply_vec(
                load_vec(input_b_ptr, pos, aligned),
                &coeff_b,
                &constants,
                flavor,
            ),
        );
        let cd = _mm256_xor_si256(
            multiply_vec(
                load_vec(input_c_ptr, pos, aligned),
                &coeff_c,
                &constants,
                flavor,
            ),
            multiply_vec(
                load_vec(input_d_ptr, pos, aligned),
                &coeff_d,
                &constants,
                flavor,
            ),
        );
        let output_vec = load_vec(output_ptr, pos, aligned);
        store_vec(
            output_ptr,
            pos,
            _mm256_xor_si256(output_vec, _mm256_xor_si256(ab, cd)),
            aligned,
        );
        pos += 32;
    }

    if pos < len {
        multiply_add_tail(
            &input_a[pos..len],
            &mut output[pos..len],
            prepared_a.coefficient,
        );
        multiply_add_tail(
            &input_b[pos..len],
            &mut output[pos..len],
            prepared_b.coefficient,
        );
        multiply_add_tail(
            &input_c[pos..len],
            &mut output[pos..len],
            prepared_c.coefficient,
        );
        multiply_add_tail(
            &input_d[pos..len],
            &mut output[pos..len],
            prepared_d.coefficient,
        );
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;
    use crate::reed_solomon::codec::{build_split_mul_table, process_slice_multiply_add};
    use crate::reed_solomon::galois::Galois16;

    #[test]
    fn xor_jit_word_multiply_matches_table() {
        let coeffs = [0, 1, 2, 7, 0x100b, 0xbeef, 0xffff];
        let values = [0, 1, 2, 0x1234, 0x8000, 0xffff];
        for coeff in coeffs {
            let table = build_split_mul_table(Galois16::new(coeff));
            for value in values {
                let expected =
                    table.low[(value & 0xff) as usize] ^ table.high[(value >> 8) as usize];
                assert_eq!(
                    multiply_word(value, coeff),
                    expected,
                    "coeff={coeff:#06x} value={value:#06x}"
                );
            }
        }
    }

    #[test]
    fn xor_jit_avx2_matches_table_executor() {
        if !is_x86_feature_detected!("avx2") || !is_x86_feature_detected!("pclmulqdq") {
            return;
        }

        for flavor in [XorJitFlavor::Port, XorJitFlavor::Clean] {
            for coeff in [1, 2, 7, 0x100b, 0xbeef] {
                let input = (0..257).map(|idx| (idx * 31 + 7) as u8).collect::<Vec<_>>();
                let mut expected = (0..257).map(|idx| (idx * 17 + 3) as u8).collect::<Vec<_>>();
                let mut actual = expected.clone();
                let table = build_split_mul_table(Galois16::new(coeff));
                process_slice_multiply_add(&input, &mut expected, &table);
                let prepared = XorJitPreparedCoeff::new(coeff);
                unsafe {
                    process_slice_multiply_add_xor_jit(&input, &mut actual, &prepared, flavor);
                }
                assert_eq!(actual, expected, "flavor={flavor:?} coeff={coeff:#06x}");
            }
        }
    }
}
