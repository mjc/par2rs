//! Tableless XOR multiply kernels for create-side forced JIT modes.
//!
//! This is the executable core used by the `xor-jit-port` and `xor-jit-clean`
//! create methods. The kernels are coefficient-specialized at backend
//! construction by storing a compact typed plan, then the hot path uses AVX2
//! shifts and XORs only. No PSHUFB or scalar lookup table is used here.

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

#[cfg(target_arch = "x86_64")]
mod bitplane;
#[cfg(target_arch = "x86_64")]
mod encoder;
#[cfg(target_arch = "x86_64")]
mod exec_mem;

const GF16_REDUCTION: u16 = 0x100b;

#[derive(Debug, Clone)]
#[cfg(target_arch = "x86_64")]
pub struct XorJitPreparedCoeff {
    coefficient: u16,
    #[allow(dead_code)]
    clean_plan: XorJitCleanPlan,
    bitplane_plan: CleanCoeffPlan,
}

#[derive(Debug, Clone)]
#[cfg(target_arch = "x86_64")]
#[allow(dead_code)]
struct XorJitCleanPlan {
    taps: [u8; 16],
    tap_count: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
struct CleanCoeffPlan {
    output_masks: [u16; 16],
}

#[cfg(target_arch = "x86_64")]
impl XorJitPreparedCoeff {
    #[inline]
    pub fn new(coefficient: u16) -> Self {
        Self {
            coefficient,
            clean_plan: XorJitCleanPlan::new(coefficient),
            bitplane_plan: CleanCoeffPlan::new(coefficient),
        }
    }

    fn bitplane_plan(&self) -> CleanCoeffPlan {
        self.bitplane_plan.clone()
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
#[cfg_attr(not(test), allow(dead_code))]
impl CleanCoeffPlan {
    fn new(coefficient: u16) -> Self {
        let output_masks =
            std::array::from_fn(|output_bit| input_dependency_mask(coefficient, output_bit));

        Self { output_masks }
    }

    fn input_mask_for_output_bit(&self, output_bit: usize) -> u16 {
        debug_assert!(output_bit < 16);
        self.output_masks[output_bit]
    }

    fn output_bit_depends_on(&self, output_bit: usize, input_bit: usize) -> bool {
        debug_assert!(input_bit < 16);
        self.input_mask_for_output_bit(output_bit) & (1 << input_bit) != 0
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn input_dependency_mask(coefficient: u16, output_bit: usize) -> u16 {
    (0..16)
        .filter(|&input_bit| multiply_word(1 << input_bit, coefficient) & (1 << output_bit) != 0)
        .fold(0u16, |mask, input_bit| mask | (1 << input_bit))
}

#[cfg(target_arch = "x86_64")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XorJitFlavor {
    Port,
    Clean,
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
type LaneKernelFn = unsafe extern "sysv64" fn(*const u8, *mut u8);

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
struct CleanLaneKernel {
    code: exec_mem::ExecutableBuffer,
    function: LaneKernelFn,
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
struct CleanBitplaneKernel {
    code: exec_mem::ExecutableBuffer,
    function: LaneKernelFn,
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
pub struct XorJitBitplaneKernel {
    kernel: CleanBitplaneKernel,
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
impl CleanLaneKernel {
    fn identity() -> std::io::Result<Self> {
        Self::from_program(identity_lane_program())
    }

    fn from_program(program: encoder::Program) -> std::io::Result<Self> {
        let (code, function) = compile_lane_program(program)?;
        Ok(Self { code, function })
    }

    unsafe fn run(&self, input: *const u8, output: *mut u8) {
        debug_assert!(!self.code.is_empty());
        (self.function)(input, output);
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
impl CleanBitplaneKernel {
    fn new(coefficient: u16) -> std::io::Result<Self> {
        Self::from_plan(CleanCoeffPlan::new(coefficient))
    }

    fn from_plan(plan: CleanCoeffPlan) -> std::io::Result<Self> {
        Self::from_program(bitplane_multiply_add_program(plan))
    }

    fn from_program(program: encoder::Program) -> std::io::Result<Self> {
        let (code, function) = compile_lane_program(program)?;
        Ok(Self { code, function })
    }

    unsafe fn multiply_add(&self, input: *const u8, output: *mut u8) {
        debug_assert!(!self.code.is_empty());
        (self.function)(input, output);
    }

    fn multiply_add_chunks(&self, input: &[u8], output: &mut [u8]) {
        assert_prepared_chunk_shape(input, output);

        for (input_block, output_block) in input
            .chunks_exact(bitplane::AVX2_BLOCK_BYTES)
            .zip(output.chunks_exact_mut(bitplane::AVX2_BLOCK_BYTES))
        {
            unsafe {
                self.multiply_add(input_block.as_ptr(), output_block.as_mut_ptr());
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
impl XorJitBitplaneKernel {
    pub fn new(prepared: &XorJitPreparedCoeff) -> std::io::Result<Self> {
        Ok(Self {
            kernel: CleanBitplaneKernel::from_plan(prepared.bitplane_plan())?,
        })
    }

    pub fn multiply_add_chunks(&self, input: &[u8], output: &mut [u8]) {
        self.kernel.multiply_add_chunks(input, output);
    }
}

#[cfg(target_arch = "x86_64")]
pub fn prepare_xor_jit_bitplane_chunks(dst: &mut [u8], src: &[u8]) -> usize {
    bitplane::prepare_avx2(dst, src)
}

#[cfg(target_arch = "x86_64")]
pub fn finish_xor_jit_bitplane_chunks(dst: &mut [u8], prepared: &[u8]) {
    assert_eq!(prepared.len() % bitplane::AVX2_BLOCK_BYTES, 0);
    assert!(prepared.len() >= dst.len().next_multiple_of(bitplane::AVX2_BLOCK_BYTES));

    let mut finished_block = [0u8; bitplane::AVX2_BLOCK_BYTES];
    for (prepared_block, output_block) in prepared
        .chunks_exact(bitplane::AVX2_BLOCK_BYTES)
        .zip(dst.chunks_mut(bitplane::AVX2_BLOCK_BYTES))
    {
        bitplane::finish_avx2_block(&mut finished_block, prepared_block.try_into().unwrap());
        output_block.copy_from_slice(&finished_block[..output_block.len()]);
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn assert_prepared_chunk_shape(input: &[u8], output: &[u8]) {
    assert_eq!(input.len(), output.len());
    assert_eq!(input.len() % bitplane::AVX2_BLOCK_BYTES, 0);
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn compile_lane_program(
    program: encoder::Program,
) -> std::io::Result<(exec_mem::ExecutableBuffer, LaneKernelFn)> {
    let generated = program.finish();
    let mut code = exec_mem::ExecutableBuffer::new(generated.len())?;
    code.write(&generated)?;
    let function = unsafe { code.function() };

    Ok((code, function))
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn identity_lane_program() -> encoder::Program {
    encoder::Program::new()
        .vmovdqu_ymm0_from_rdi()
        .vmovdqu_ymm1_from_rsi()
        .vpxor_ymm0_ymm0_ymm1()
        .vmovdqu_rsi_from_ymm0()
        .vzeroupper()
        .ret()
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn bitplane_multiply_add_program(plan: CleanCoeffPlan) -> encoder::Program {
    (0..16)
        .filter_map(|output_bit| {
            let input_mask = plan.input_mask_for_output_bit(output_bit);
            (input_mask != 0).then_some((output_bit, input_mask))
        })
        .fold(encoder::Program::new(), emit_output_bitplane)
        .vzeroupper()
        .ret()
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn emit_output_bitplane(
    program: encoder::Program,
    (output_bit, input_mask): (usize, u16),
) -> encoder::Program {
    input_bits(input_mask)
        .fold(
            program.vmovdqu_ymm0_from_rsi_offset(bitplane_vector_offset(output_bit)),
            |program, input_bit| {
                program
                    .vmovdqu_ymm1_from_rdi_offset(bitplane_vector_offset(input_bit))
                    .vpxor_ymm0_ymm0_ymm1()
            },
        )
        .vmovdqu_rsi_offset_from_ymm0(bitplane_vector_offset(output_bit))
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn input_bits(mask: u16) -> impl Iterator<Item = usize> {
    (0..16).filter(move |input_bit| mask & (1 << input_bit) != 0)
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn bitplane_vector_offset(word_bit: usize) -> i32 {
    debug_assert!(word_bit < 16);
    let half = if word_bit < 8 {
        bitplane::ByteHalf::Low
    } else {
        bitplane::ByteHalf::High
    };
    let bit_from_msb = 7 - (word_bit & 7);

    bitplane::mask_offset(half, bit_from_msb, 0) as i32
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
#[target_feature(enable = "avx2", enable = "vpclmulqdq")]
unsafe fn clmul_256<const CONTROL: i32>(left: __m256i, right: __m256i) -> __m256i {
    _mm256_clmulepi64_epi128::<CONTROL>(left, right)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "vpclmulqdq")]
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
#[target_feature(enable = "avx2", enable = "vpclmulqdq")]
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
#[target_feature(enable = "avx2", enable = "vpclmulqdq")]
unsafe fn load_vec(ptr: *const u8, pos: usize) -> __m256i {
    _mm256_loadu_si256(ptr.add(pos) as *const __m256i)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "vpclmulqdq")]
unsafe fn store_vec(ptr: *mut u8, pos: usize, value: __m256i) {
    _mm256_storeu_si256(ptr.add(pos) as *mut __m256i, value);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "vpclmulqdq")]
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

    let mut pos = 0;
    while pos + 128 <= avx_end {
        let in0 = load_vec(input_ptr, pos);
        let out0 = load_vec(output_ptr, pos);
        store_vec(
            output_ptr,
            pos,
            _mm256_xor_si256(out0, multiply_vec(in0, &coeff, &constants, flavor)),
        );
        pos += 32;

        let in1 = load_vec(input_ptr, pos);
        let out1 = load_vec(output_ptr, pos);
        store_vec(
            output_ptr,
            pos,
            _mm256_xor_si256(out1, multiply_vec(in1, &coeff, &constants, flavor)),
        );
        pos += 32;

        let in2 = load_vec(input_ptr, pos);
        let out2 = load_vec(output_ptr, pos);
        store_vec(
            output_ptr,
            pos,
            _mm256_xor_si256(out2, multiply_vec(in2, &coeff, &constants, flavor)),
        );
        pos += 32;

        let in3 = load_vec(input_ptr, pos);
        let out3 = load_vec(output_ptr, pos);
        store_vec(
            output_ptr,
            pos,
            _mm256_xor_si256(out3, multiply_vec(in3, &coeff, &constants, flavor)),
        );
        pos += 32;
    }

    while pos < avx_end {
        let input_vec = load_vec(input_ptr, pos);
        let output_vec = load_vec(output_ptr, pos);
        store_vec(
            output_ptr,
            pos,
            _mm256_xor_si256(
                output_vec,
                multiply_vec(input_vec, &coeff, &constants, flavor),
            ),
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
#[target_feature(enable = "avx2", enable = "vpclmulqdq")]
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

    let mut pos = 0;
    while pos < avx_end {
        let result = _mm256_xor_si256(
            multiply_vec(load_vec(input_a_ptr, pos), &coeff_a, &constants, flavor),
            multiply_vec(load_vec(input_b_ptr, pos), &coeff_b, &constants, flavor),
        );
        let output_vec = load_vec(output_ptr, pos);
        store_vec(output_ptr, pos, _mm256_xor_si256(output_vec, result));
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
#[target_feature(enable = "avx2", enable = "vpclmulqdq")]
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

    let mut pos = 0;
    while pos < avx_end {
        let ab = _mm256_xor_si256(
            multiply_vec(load_vec(input_a_ptr, pos), &coeff_a, &constants, flavor),
            multiply_vec(load_vec(input_b_ptr, pos), &coeff_b, &constants, flavor),
        );
        let cd = _mm256_xor_si256(
            multiply_vec(load_vec(input_c_ptr, pos), &coeff_c, &constants, flavor),
            multiply_vec(load_vec(input_d_ptr, pos), &coeff_d, &constants, flavor),
        );
        let output_vec = load_vec(output_ptr, pos);
        store_vec(
            output_ptr,
            pos,
            _mm256_xor_si256(output_vec, _mm256_xor_si256(ab, cd)),
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

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "vpclmulqdq")]
#[allow(clippy::too_many_arguments)]
pub unsafe fn process_slices_multiply_add_xor_jit_x4_inputs_x2_outputs(
    input_a: &[u8],
    input_b: &[u8],
    input_c: &[u8],
    input_d: &[u8],
    coeff_a0: &XorJitPreparedCoeff,
    coeff_b0: &XorJitPreparedCoeff,
    coeff_c0: &XorJitPreparedCoeff,
    coeff_d0: &XorJitPreparedCoeff,
    output_0: &mut [u8],
    coeff_a1: &XorJitPreparedCoeff,
    coeff_b1: &XorJitPreparedCoeff,
    coeff_c1: &XorJitPreparedCoeff,
    coeff_d1: &XorJitPreparedCoeff,
    output_1: &mut [u8],
    flavor: XorJitFlavor,
) {
    let constants = kernel_constants();
    let coeff_a0_vec = coeff_vectors(coeff_a0, &constants);
    let coeff_b0_vec = coeff_vectors(coeff_b0, &constants);
    let coeff_c0_vec = coeff_vectors(coeff_c0, &constants);
    let coeff_d0_vec = coeff_vectors(coeff_d0, &constants);
    let coeff_a1_vec = coeff_vectors(coeff_a1, &constants);
    let coeff_b1_vec = coeff_vectors(coeff_b1, &constants);
    let coeff_c1_vec = coeff_vectors(coeff_c1, &constants);
    let coeff_d1_vec = coeff_vectors(coeff_d1, &constants);
    let len = input_a
        .len()
        .min(input_b.len())
        .min(input_c.len())
        .min(input_d.len())
        .min(output_0.len())
        .min(output_1.len());
    let avx_end = len / 32 * 32;
    let input_a_ptr = input_a.as_ptr();
    let input_b_ptr = input_b.as_ptr();
    let input_c_ptr = input_c.as_ptr();
    let input_d_ptr = input_d.as_ptr();
    let output_0_ptr = output_0.as_mut_ptr();
    let output_1_ptr = output_1.as_mut_ptr();

    macro_rules! process_vector {
        ($offset:expr) => {{
            let offset = $offset;
            let in_a = load_vec(input_a_ptr, offset);
            let in_b = load_vec(input_b_ptr, offset);
            let in_c = load_vec(input_c_ptr, offset);
            let in_d = load_vec(input_d_ptr, offset);

            let result_0_ab = _mm256_xor_si256(
                multiply_vec(in_a, &coeff_a0_vec, &constants, flavor),
                multiply_vec(in_b, &coeff_b0_vec, &constants, flavor),
            );
            let result_0_cd = _mm256_xor_si256(
                multiply_vec(in_c, &coeff_c0_vec, &constants, flavor),
                multiply_vec(in_d, &coeff_d0_vec, &constants, flavor),
            );
            let output_0_vec = load_vec(output_0_ptr, offset);
            store_vec(
                output_0_ptr,
                offset,
                _mm256_xor_si256(output_0_vec, _mm256_xor_si256(result_0_ab, result_0_cd)),
            );

            let result_1_ab = _mm256_xor_si256(
                multiply_vec(in_a, &coeff_a1_vec, &constants, flavor),
                multiply_vec(in_b, &coeff_b1_vec, &constants, flavor),
            );
            let result_1_cd = _mm256_xor_si256(
                multiply_vec(in_c, &coeff_c1_vec, &constants, flavor),
                multiply_vec(in_d, &coeff_d1_vec, &constants, flavor),
            );
            let output_1_vec = load_vec(output_1_ptr, offset);
            store_vec(
                output_1_ptr,
                offset,
                _mm256_xor_si256(output_1_vec, _mm256_xor_si256(result_1_ab, result_1_cd)),
            );
        }};
    }

    let mut pos = 0;
    while pos + 128 <= avx_end {
        process_vector!(pos);
        process_vector!(pos + 32);
        process_vector!(pos + 64);
        process_vector!(pos + 96);
        pos += 128;
    }

    while pos < avx_end {
        process_vector!(pos);
        pos += 32;
    }

    if pos < len {
        multiply_add_tail(
            &input_a[pos..len],
            &mut output_0[pos..len],
            coeff_a0.coefficient,
        );
        multiply_add_tail(
            &input_b[pos..len],
            &mut output_0[pos..len],
            coeff_b0.coefficient,
        );
        multiply_add_tail(
            &input_c[pos..len],
            &mut output_0[pos..len],
            coeff_c0.coefficient,
        );
        multiply_add_tail(
            &input_d[pos..len],
            &mut output_0[pos..len],
            coeff_d0.coefficient,
        );
        multiply_add_tail(
            &input_a[pos..len],
            &mut output_1[pos..len],
            coeff_a1.coefficient,
        );
        multiply_add_tail(
            &input_b[pos..len],
            &mut output_1[pos..len],
            coeff_b1.coefficient,
        );
        multiply_add_tail(
            &input_c[pos..len],
            &mut output_1[pos..len],
            coeff_c1.coefficient,
        );
        multiply_add_tail(
            &input_d[pos..len],
            &mut output_1[pos..len],
            coeff_d1.coefficient,
        );
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "vpclmulqdq")]
#[allow(clippy::too_many_arguments)]
pub unsafe fn process_slices_multiply_add_xor_jit_x4_inputs_x4_outputs(
    input_a: &[u8],
    input_b: &[u8],
    input_c: &[u8],
    input_d: &[u8],
    coeff_a0: &XorJitPreparedCoeff,
    coeff_b0: &XorJitPreparedCoeff,
    coeff_c0: &XorJitPreparedCoeff,
    coeff_d0: &XorJitPreparedCoeff,
    output_0: &mut [u8],
    coeff_a1: &XorJitPreparedCoeff,
    coeff_b1: &XorJitPreparedCoeff,
    coeff_c1: &XorJitPreparedCoeff,
    coeff_d1: &XorJitPreparedCoeff,
    output_1: &mut [u8],
    coeff_a2: &XorJitPreparedCoeff,
    coeff_b2: &XorJitPreparedCoeff,
    coeff_c2: &XorJitPreparedCoeff,
    coeff_d2: &XorJitPreparedCoeff,
    output_2: &mut [u8],
    coeff_a3: &XorJitPreparedCoeff,
    coeff_b3: &XorJitPreparedCoeff,
    coeff_c3: &XorJitPreparedCoeff,
    coeff_d3: &XorJitPreparedCoeff,
    output_3: &mut [u8],
    flavor: XorJitFlavor,
) {
    let constants = kernel_constants();
    let coeff_a0_vec = coeff_vectors(coeff_a0, &constants);
    let coeff_b0_vec = coeff_vectors(coeff_b0, &constants);
    let coeff_c0_vec = coeff_vectors(coeff_c0, &constants);
    let coeff_d0_vec = coeff_vectors(coeff_d0, &constants);
    let coeff_a1_vec = coeff_vectors(coeff_a1, &constants);
    let coeff_b1_vec = coeff_vectors(coeff_b1, &constants);
    let coeff_c1_vec = coeff_vectors(coeff_c1, &constants);
    let coeff_d1_vec = coeff_vectors(coeff_d1, &constants);
    let coeff_a2_vec = coeff_vectors(coeff_a2, &constants);
    let coeff_b2_vec = coeff_vectors(coeff_b2, &constants);
    let coeff_c2_vec = coeff_vectors(coeff_c2, &constants);
    let coeff_d2_vec = coeff_vectors(coeff_d2, &constants);
    let coeff_a3_vec = coeff_vectors(coeff_a3, &constants);
    let coeff_b3_vec = coeff_vectors(coeff_b3, &constants);
    let coeff_c3_vec = coeff_vectors(coeff_c3, &constants);
    let coeff_d3_vec = coeff_vectors(coeff_d3, &constants);
    let len = input_a
        .len()
        .min(input_b.len())
        .min(input_c.len())
        .min(input_d.len())
        .min(output_0.len())
        .min(output_1.len())
        .min(output_2.len())
        .min(output_3.len());
    let avx_end = len / 32 * 32;
    let input_a_ptr = input_a.as_ptr();
    let input_b_ptr = input_b.as_ptr();
    let input_c_ptr = input_c.as_ptr();
    let input_d_ptr = input_d.as_ptr();
    let output_0_ptr = output_0.as_mut_ptr();
    let output_1_ptr = output_1.as_mut_ptr();
    let output_2_ptr = output_2.as_mut_ptr();
    let output_3_ptr = output_3.as_mut_ptr();

    macro_rules! accumulate_output {
        ($offset:expr, $in_a:expr, $in_b:expr, $in_c:expr, $in_d:expr, $out_ptr:expr, $ca:expr, $cb:expr, $cc:expr, $cd:expr) => {{
            let result_ab = _mm256_xor_si256(
                multiply_vec($in_a, $ca, &constants, flavor),
                multiply_vec($in_b, $cb, &constants, flavor),
            );
            let result_cd = _mm256_xor_si256(
                multiply_vec($in_c, $cc, &constants, flavor),
                multiply_vec($in_d, $cd, &constants, flavor),
            );
            let output_vec = load_vec($out_ptr, $offset);
            store_vec(
                $out_ptr,
                $offset,
                _mm256_xor_si256(output_vec, _mm256_xor_si256(result_ab, result_cd)),
            );
        }};
    }

    macro_rules! process_vector {
        ($offset:expr) => {{
            let offset = $offset;
            let in_a = load_vec(input_a_ptr, offset);
            let in_b = load_vec(input_b_ptr, offset);
            let in_c = load_vec(input_c_ptr, offset);
            let in_d = load_vec(input_d_ptr, offset);

            accumulate_output!(
                offset,
                in_a,
                in_b,
                in_c,
                in_d,
                output_0_ptr,
                &coeff_a0_vec,
                &coeff_b0_vec,
                &coeff_c0_vec,
                &coeff_d0_vec
            );
            accumulate_output!(
                offset,
                in_a,
                in_b,
                in_c,
                in_d,
                output_1_ptr,
                &coeff_a1_vec,
                &coeff_b1_vec,
                &coeff_c1_vec,
                &coeff_d1_vec
            );
            accumulate_output!(
                offset,
                in_a,
                in_b,
                in_c,
                in_d,
                output_2_ptr,
                &coeff_a2_vec,
                &coeff_b2_vec,
                &coeff_c2_vec,
                &coeff_d2_vec
            );
            accumulate_output!(
                offset,
                in_a,
                in_b,
                in_c,
                in_d,
                output_3_ptr,
                &coeff_a3_vec,
                &coeff_b3_vec,
                &coeff_c3_vec,
                &coeff_d3_vec
            );
        }};
    }

    let mut pos = 0;
    while pos + 64 <= avx_end {
        process_vector!(pos);
        process_vector!(pos + 32);
        pos += 64;
    }

    while pos < avx_end {
        process_vector!(pos);
        pos += 32;
    }

    if pos < len {
        for (output, coeff_a, coeff_b, coeff_c, coeff_d) in [
            (
                &mut output_0[pos..len],
                coeff_a0.coefficient,
                coeff_b0.coefficient,
                coeff_c0.coefficient,
                coeff_d0.coefficient,
            ),
            (
                &mut output_1[pos..len],
                coeff_a1.coefficient,
                coeff_b1.coefficient,
                coeff_c1.coefficient,
                coeff_d1.coefficient,
            ),
            (
                &mut output_2[pos..len],
                coeff_a2.coefficient,
                coeff_b2.coefficient,
                coeff_c2.coefficient,
                coeff_d2.coefficient,
            ),
            (
                &mut output_3[pos..len],
                coeff_a3.coefficient,
                coeff_b3.coefficient,
                coeff_c3.coefficient,
                coeff_d3.coefficient,
            ),
        ] {
            multiply_add_tail(&input_a[pos..len], output, coeff_a);
            multiply_add_tail(&input_b[pos..len], output, coeff_b);
            multiply_add_tail(&input_c[pos..len], output, coeff_c);
            multiply_add_tail(&input_d[pos..len], output, coeff_d);
        }
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;
    use crate::reed_solomon::codec::{build_split_mul_table, process_slice_multiply_add};
    use crate::reed_solomon::galois::Galois16;

    #[test]
    fn executable_buffer_runs_constant_function() {
        let generated = encoder::Program::new().mov_eax_imm32(7).ret().finish();
        assert_eq!(generated, [0xb8, 0x07, 0x00, 0x00, 0x00, 0xc3]);

        let mut code = exec_mem::ExecutableBuffer::new(16).expect("executable buffer");
        code.write(&generated).expect("write generated code");
        let function: extern "sysv64" fn() -> u32 = unsafe { code.function() };

        assert_eq!(function(), 7);
    }

    #[test]
    fn executable_buffer_can_be_reused_for_new_code() {
        let mut code = exec_mem::ExecutableBuffer::new(16).expect("executable buffer");

        code.write(&encoder::Program::new().mov_eax_imm32(7).ret().finish())
            .expect("write first generated code");
        let function: extern "sysv64" fn() -> u32 = unsafe { code.function() };
        assert_eq!(function(), 7);

        code.write(&encoder::Program::new().mov_eax_imm32(11).ret().finish())
            .expect("rewrite generated code");
        let function: extern "sysv64" fn() -> u32 = unsafe { code.function() };
        assert_eq!(function(), 11);
    }

    #[test]
    fn generated_avx2_lane_xor_updates_destination() {
        if !is_x86_feature_detected!("avx2") {
            return;
        }

        let generated = encoder::Program::new()
            .vmovdqu_ymm0_from_rdi()
            .vmovdqu_ymm1_from_rsi()
            .vpxor_ymm0_ymm0_ymm1()
            .vmovdqu_rsi_from_ymm0()
            .vzeroupper()
            .ret()
            .finish();
        assert_eq!(
            generated,
            [
                0xc5, 0xfe, 0x6f, 0x07, 0xc5, 0xfe, 0x6f, 0x0e, 0xc5, 0xfd, 0xef, 0xc1, 0xc5, 0xfe,
                0x7f, 0x06, 0xc5, 0xf8, 0x77, 0xc3
            ]
        );

        let input = [0xa5u8; 32];
        let mut output = [0x5au8; 32];
        let mut code = exec_mem::ExecutableBuffer::new(32).expect("executable buffer");
        code.write(&generated).expect("write generated code");
        let function: extern "sysv64" fn(*const u8, *mut u8) = unsafe { code.function() };

        function(input.as_ptr(), output.as_mut_ptr());

        assert_eq!(output, [0xffu8; 32]);
    }

    #[test]
    fn generated_avx2_lane_xor_updates_destination_offset() {
        if !is_x86_feature_detected!("avx2") {
            return;
        }

        let mut input = [0u8; 64];
        let mut output = [0x33u8; 64];
        input[32..].fill(0xcc);

        let program = encoder::Program::new()
            .vmovdqu_ymm0_from_rdi_offset(32)
            .vmovdqu_ymm1_from_rsi_offset(32)
            .vpxor_ymm0_ymm0_ymm1()
            .vmovdqu_rsi_offset_from_ymm0(32)
            .vzeroupper()
            .ret();
        let generated = program.finish();
        let mut code = exec_mem::ExecutableBuffer::new(generated.len()).expect("executable code");
        code.write(&generated).expect("write generated code");
        let function: extern "sysv64" fn(*const u8, *mut u8) = unsafe { code.function() };

        function(input.as_ptr(), output.as_mut_ptr());

        assert_eq!(&output[..32], &[0x33; 32]);
        assert_eq!(&output[32..], &[0xff; 32]);
    }

    #[test]
    fn clean_identity_lane_kernel_matches_table_executor() {
        if !is_x86_feature_detected!("avx2") {
            return;
        }

        let input = (0..32).map(|value| value as u8).collect::<Vec<_>>();
        let mut expected = vec![0x33; 32];
        let mut actual = expected.clone();
        let tables = build_split_mul_table(Galois16::new(1));
        process_slice_multiply_add(&input, &mut expected, &tables);

        let kernel = CleanLaneKernel::identity().expect("identity lane kernel");
        unsafe {
            kernel.run(input.as_ptr(), actual.as_mut_ptr());
        }

        assert_eq!(actual, expected);
    }

    #[test]
    fn clean_coeff_plan_matches_basis_multiplication() {
        for coefficient in [0, 1, 2, 3, 5, 0x100b, 0xffff] {
            let plan = CleanCoeffPlan::new(coefficient);

            for input_bit in 0..16 {
                let product = multiply_word(1 << input_bit, coefficient);
                for output_bit in 0..16 {
                    assert_eq!(
                        plan.output_bit_depends_on(output_bit, input_bit),
                        product & (1 << output_bit) != 0,
                        "coefficient={coefficient:#06x} output_bit={output_bit} input_bit={input_bit}"
                    );
                }
            }
        }
    }

    #[test]
    fn avx2_bitplane_prepare_matches_turbo_block_layout() {
        let mut input = [0u8; bitplane::AVX2_BLOCK_BYTES];
        input[0] = 0b1000_0001;
        input[1] = 0b0100_0010;
        input[62] = 0xff;
        input[63] = 0x80;
        input[64] = 0x01;

        let mut prepared = [0u8; bitplane::AVX2_BLOCK_BYTES];
        bitplane::prepare_avx2_block(&mut prepared, &input);

        assert_eq!(prepared_mask(&prepared, bitplane::ByteHalf::High, 1, 0), 1);
        assert_eq!(prepared_mask(&prepared, bitplane::ByteHalf::High, 6, 0), 1);
        assert_eq!(
            prepared_mask(&prepared, bitplane::ByteHalf::High, 0, 0),
            1 << 31
        );
        assert_eq!(
            prepared_mask(&prepared, bitplane::ByteHalf::Low, 0, 0),
            1 | (1 << 31)
        );
        assert_eq!(
            prepared_mask(&prepared, bitplane::ByteHalf::Low, 7, 0),
            1 | (1 << 31)
        );
        assert_eq!(prepared_mask(&prepared, bitplane::ByteHalf::Low, 7, 1), 1);
        assert_eq!(prepared_mask(&prepared, bitplane::ByteHalf::High, 0, 1), 0);
    }

    #[test]
    fn avx2_bitplane_prepare_zero_pads_partial_final_block() {
        let input = vec![0xffu8; 33];
        let mut prepared = vec![0x55u8; bitplane::AVX2_BLOCK_BYTES * 2];

        let prepared_len = bitplane::prepare_avx2(&mut prepared, &input);

        assert_eq!(prepared_len, bitplane::AVX2_BLOCK_BYTES);
        for bit_from_msb in 0..8 {
            assert_eq!(
                prepared_mask_slice(&prepared, bitplane::ByteHalf::Low, bit_from_msb, 0),
                (1 << 17) - 1
            );
            assert_eq!(
                prepared_mask_slice(&prepared, bitplane::ByteHalf::High, bit_from_msb, 0),
                (1 << 16) - 1
            );
            assert_eq!(
                prepared_mask_slice(&prepared, bitplane::ByteHalf::High, bit_from_msb, 1),
                0
            );
        }
        assert!(prepared[prepared_len..].iter().all(|&byte| byte == 0x55));
    }

    #[test]
    fn prepared_bitplane_multiply_add_matches_table_executor() {
        let input = (0..bitplane::AVX2_BLOCK_BYTES)
            .map(|value| (value * 37) as u8)
            .collect::<Vec<_>>();
        let mut prepared = vec![0u8; bitplane::AVX2_BLOCK_BYTES];
        bitplane::prepare_avx2(&mut prepared, &input);

        for coefficient in [0, 1, 2, 3, 5, 0x100b, 0xffff] {
            let tables = build_split_mul_table(Galois16::new(coefficient));
            let mut expected = vec![0xa5; bitplane::AVX2_BLOCK_BYTES];
            let mut actual = expected.clone();

            process_slice_multiply_add(&input, &mut expected, &tables);
            bitplane::multiply_add_prepared_avx2_block(&prepared, coefficient, &mut actual);

            assert_eq!(actual, expected, "coefficient={coefficient:#06x}");
        }
    }

    #[test]
    fn avx2_bitplane_finish_roundtrips_prepared_block() {
        let input = core::array::from_fn(|idx| (idx * 37 + 11) as u8);
        let mut prepared = [0u8; bitplane::AVX2_BLOCK_BYTES];
        let mut actual = [0u8; bitplane::AVX2_BLOCK_BYTES];

        bitplane::prepare_avx2_block(&mut prepared, &input);
        bitplane::finish_avx2_block(&mut actual, &prepared);

        assert_eq!(actual, input);
    }

    #[test]
    fn prepared_bitplane_multiply_add_to_prepared_matches_table_executor() {
        let input = (0..bitplane::AVX2_BLOCK_BYTES)
            .map(|value| (value * 37 + 11) as u8)
            .collect::<Vec<_>>();
        let initial_output = (0..bitplane::AVX2_BLOCK_BYTES)
            .map(|value| (value * 17 + 3) as u8)
            .collect::<Vec<_>>();
        let mut prepared_input = [0u8; bitplane::AVX2_BLOCK_BYTES];
        let mut prepared_output = [0u8; bitplane::AVX2_BLOCK_BYTES];

        bitplane::prepare_avx2_block(&mut prepared_input, input.as_slice().try_into().unwrap());
        bitplane::prepare_avx2_block(
            &mut prepared_output,
            initial_output.as_slice().try_into().unwrap(),
        );

        for coefficient in [0, 1, 2, 3, 5, 0x100b, 0xffff] {
            let tables = build_split_mul_table(Galois16::new(coefficient));
            let mut expected = initial_output.clone();
            let mut actual = [0u8; bitplane::AVX2_BLOCK_BYTES];
            let mut output_block = prepared_output;

            process_slice_multiply_add(&input, &mut expected, &tables);
            bitplane::multiply_add_prepared_avx2_block_to_prepared(
                &prepared_input,
                coefficient,
                &mut output_block,
            );
            bitplane::finish_avx2_block(&mut actual, &output_block);

            assert_eq!(
                actual.as_slice(),
                expected,
                "coefficient={coefficient:#06x}"
            );
        }
    }

    #[test]
    fn generated_bitplane_multiply_add_matches_reference() {
        if !is_x86_feature_detected!("avx2") {
            return;
        }

        let input = core::array::from_fn(|idx| (idx * 37 + 11) as u8);
        let initial_output = core::array::from_fn(|idx| (idx * 17 + 3) as u8);
        let mut prepared_input = [0u8; bitplane::AVX2_BLOCK_BYTES];
        let mut prepared_output = [0u8; bitplane::AVX2_BLOCK_BYTES];

        bitplane::prepare_avx2_block(&mut prepared_input, &input);
        bitplane::prepare_avx2_block(&mut prepared_output, &initial_output);

        for coefficient in [0, 1, 2, 3, 5, 0x100b, 0xffff] {
            let mut expected = prepared_output;
            let mut actual = prepared_output;
            let kernel = CleanBitplaneKernel::new(coefficient).expect("bitplane kernel");

            bitplane::multiply_add_prepared_avx2_block_to_prepared(
                &prepared_input,
                coefficient,
                &mut expected,
            );
            unsafe {
                kernel.multiply_add(prepared_input.as_ptr(), actual.as_mut_ptr());
            }

            assert_eq!(actual, expected, "coefficient={coefficient:#06x}");
        }
    }

    #[test]
    fn generated_bitplane_multiply_add_processes_prepared_chunks() {
        if !is_x86_feature_detected!("avx2") {
            return;
        }

        let input = (0..bitplane::AVX2_BLOCK_BYTES * 3)
            .map(|idx| (idx * 37 + 11) as u8)
            .collect::<Vec<_>>();
        let initial_output = (0..bitplane::AVX2_BLOCK_BYTES * 3)
            .map(|idx| (idx * 17 + 3) as u8)
            .collect::<Vec<_>>();
        let mut prepared_input = vec![0u8; input.len()];
        let mut expected = vec![0u8; input.len()];
        let mut actual = vec![0u8; input.len()];
        let coefficient = 0x100b;

        bitplane::prepare_avx2(&mut prepared_input, &input);
        bitplane::prepare_avx2(&mut expected, &initial_output);
        bitplane::prepare_avx2(&mut actual, &initial_output);

        for (input_block, output_block) in prepared_input
            .chunks_exact(bitplane::AVX2_BLOCK_BYTES)
            .zip(expected.chunks_exact_mut(bitplane::AVX2_BLOCK_BYTES))
        {
            bitplane::multiply_add_prepared_avx2_block_to_prepared(
                input_block.try_into().unwrap(),
                coefficient,
                output_block.try_into().unwrap(),
            );
        }

        let kernel = CleanBitplaneKernel::new(coefficient).expect("bitplane kernel");
        kernel.multiply_add_chunks(&prepared_input, &mut actual);

        assert_eq!(actual, expected);
    }

    #[test]
    fn public_bitplane_kernel_uses_prepared_coefficient_metadata() {
        if !is_x86_feature_detected!("avx2") {
            return;
        }

        let input = (0..bitplane::AVX2_BLOCK_BYTES * 2)
            .map(|idx| (idx * 13 + 7) as u8)
            .collect::<Vec<_>>();
        let initial_output = (0..bitplane::AVX2_BLOCK_BYTES * 2)
            .map(|idx| (idx * 19 + 5) as u8)
            .collect::<Vec<_>>();
        let mut prepared_input = vec![0u8; input.len()];
        let mut expected = vec![0u8; input.len()];
        let mut actual = vec![0u8; input.len()];
        let coefficient = 0xbeef;
        let prepared = XorJitPreparedCoeff::new(coefficient);

        bitplane::prepare_avx2(&mut prepared_input, &input);
        bitplane::prepare_avx2(&mut expected, &initial_output);
        bitplane::prepare_avx2(&mut actual, &initial_output);

        for (input_block, output_block) in prepared_input
            .chunks_exact(bitplane::AVX2_BLOCK_BYTES)
            .zip(expected.chunks_exact_mut(bitplane::AVX2_BLOCK_BYTES))
        {
            bitplane::multiply_add_prepared_avx2_block_to_prepared(
                input_block.try_into().unwrap(),
                coefficient,
                output_block.try_into().unwrap(),
            );
        }

        let kernel = XorJitBitplaneKernel::new(&prepared).expect("bitplane kernel");
        kernel.multiply_add_chunks(&prepared_input, &mut actual);

        assert_eq!(actual, expected);
    }

    #[test]
    fn public_bitplane_prepare_finish_roundtrips_partial_chunks() {
        let input = (0..bitplane::AVX2_BLOCK_BYTES * 2 + 37)
            .map(|idx| (idx * 29 + 17) as u8)
            .collect::<Vec<_>>();
        let mut prepared = vec![0u8; input.len().next_multiple_of(bitplane::AVX2_BLOCK_BYTES)];
        let mut actual = vec![0u8; input.len()];

        let prepared_len = prepare_xor_jit_bitplane_chunks(&mut prepared, &input);
        finish_xor_jit_bitplane_chunks(&mut actual, &prepared[..prepared_len]);

        assert_eq!(actual, input);
    }

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

    fn prepared_mask(
        prepared: &[u8; bitplane::AVX2_BLOCK_BYTES],
        half: bitplane::ByteHalf,
        bit_from_msb: usize,
        group: usize,
    ) -> u32 {
        let offset = bitplane::mask_offset(half, bit_from_msb, group);
        u32::from_le_bytes(prepared[offset..offset + 4].try_into().unwrap())
    }

    fn prepared_mask_slice(
        prepared: &[u8],
        half: bitplane::ByteHalf,
        bit_from_msb: usize,
        group: usize,
    ) -> u32 {
        let offset = bitplane::mask_offset(half, bit_from_msb, group);
        u32::from_le_bytes(prepared[offset..offset + 4].try_into().unwrap())
    }

    #[test]
    fn xor_jit_avx2_matches_table_executor() {
        if !is_x86_feature_detected!("avx2") || !is_x86_feature_detected!("vpclmulqdq") {
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
