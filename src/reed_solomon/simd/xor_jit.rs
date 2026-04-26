//! Tableless XOR multiply kernels for create-side forced JIT modes.
//!
//! This is the executable core used by the `xor-jit` create method. The
//! kernels are coefficient-specialized at backend construction by storing a
//! compact typed plan, then the hot path uses generated AVX2 XOR code. No
//! PSHUFB or scalar lookup table is used here.

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;
#[cfg(target_arch = "x86_64")]
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, OnceLock,
};

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
    bitplane_plan: Arc<OnceLock<BitplaneCoeffPlan>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
struct BitplaneCoeffPlan {
    coefficient: u16,
    output_masks: [u16; 16],
    turbo_pairs: [TurboOutputPairPlan; 8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
struct TurboOutputPairPlan {
    first_output: usize,
    second_output: usize,
    first_seed: Option<usize>,
    second_seed: Option<usize>,
    first_remaining_mask: u16,
    second_remaining_mask: u16,
    deps: TurboDepPlan,
    common: TurboCommonPlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
struct TurboCommonPlan {
    lowest: Option<usize>,
    highest: Option<usize>,
    eliminated_mask: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
struct TurboDepPlan {
    mem_deps: u8,
    dep1_low: u8,
    dep1_high: u8,
    dep2_low: u8,
    dep2_high: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(target_arch = "x86_64")]
struct TurboMemDepOp {
    target_reg: u8,
    physical_bit: u8,
}

#[derive(Debug, Clone)]
#[cfg(target_arch = "x86_64")]
struct TurboDepTables {
    mem_ops: [[TurboMemDepOp; 3]; 64],
    mem_len: [u8; 64],
    nums: [[u8; 8]; 128],
    rmask: [[u8; 8]; 128],
    mem_bytes: Vec<Box<[u8]>>,
    main_bytes_low: Vec<Box<[u8]>>,
    main_bytes_high: Vec<Box<[u8]>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
struct InputPreloadPlan {
    registers: [Option<u8>; 16],
}

#[cfg(target_arch = "x86_64")]
trait XorJitBitplaneProgram: Sized {
    fn vmovdqa_ymm_from_input_offset(self, reg: u8, offset: i32) -> Self;
    fn vmovdqa_ymm_from_output_offset(self, reg: u8, offset: i32) -> Self;
    fn vmovdqa_ymm(self, dst: u8, src: u8) -> Self;
    fn vpxor_ymm(self, dst: u8, lhs: u8, rhs: u8) -> Self;
    fn vpxor_ymm_input_offset(self, dst: u8, lhs: u8, offset: i32) -> Self;
    fn vpxor_ymm_output_offset(self, dst: u8, lhs: u8, offset: i32) -> Self;
    fn vmovdqa_output_offset_from_ymm(self, offset: i32, reg: u8) -> Self;
}

#[cfg(target_arch = "x86_64")]
impl XorJitBitplaneProgram for encoder::Program {
    fn vmovdqa_ymm_from_input_offset(self, reg: u8, offset: i32) -> Self {
        self.vmovdqa_ymm_from_rax_offset(reg, offset)
    }

    fn vmovdqa_ymm_from_output_offset(self, reg: u8, offset: i32) -> Self {
        self.vmovdqa_ymm_from_rdx_offset(reg, offset)
    }

    fn vmovdqa_ymm(self, dst: u8, src: u8) -> Self {
        self.vmovdqa_ymm(dst, src)
    }

    fn vpxor_ymm(self, dst: u8, lhs: u8, rhs: u8) -> Self {
        self.vpxor_ymm(dst, lhs, rhs)
    }

    fn vpxor_ymm_input_offset(self, dst: u8, lhs: u8, offset: i32) -> Self {
        self.vpxor_ymm_rax_offset(dst, lhs, offset)
    }

    fn vpxor_ymm_output_offset(self, dst: u8, lhs: u8, offset: i32) -> Self {
        self.vpxor_ymm_rdx_offset(dst, lhs, offset)
    }

    fn vmovdqa_output_offset_from_ymm(self, offset: i32, reg: u8) -> Self {
        self.vmovdqa_rdx_offset_from_ymm(offset, reg)
    }
}

#[cfg(target_arch = "x86_64")]
impl<'a, S: encoder::ByteSink> XorJitBitplaneProgram for encoder::ProgramSink<'a, S> {
    fn vmovdqa_ymm_from_input_offset(self, reg: u8, offset: i32) -> Self {
        self.vmovdqa_ymm_from_rax_offset(reg, offset)
    }

    fn vmovdqa_ymm_from_output_offset(self, reg: u8, offset: i32) -> Self {
        self.vmovdqa_ymm_from_rdx_offset(reg, offset)
    }

    fn vmovdqa_ymm(self, dst: u8, src: u8) -> Self {
        self.vmovdqa_ymm(dst, src)
    }

    fn vpxor_ymm(self, dst: u8, lhs: u8, rhs: u8) -> Self {
        self.vpxor_ymm(dst, lhs, rhs)
    }

    fn vpxor_ymm_input_offset(self, dst: u8, lhs: u8, offset: i32) -> Self {
        self.vpxor_ymm_rax_offset(dst, lhs, offset)
    }

    fn vpxor_ymm_output_offset(self, dst: u8, lhs: u8, offset: i32) -> Self {
        self.vpxor_ymm_rdx_offset(dst, lhs, offset)
    }

    fn vmovdqa_output_offset_from_ymm(self, offset: i32, reg: u8) -> Self {
        self.vmovdqa_rdx_offset_from_ymm(offset, reg)
    }
}

#[cfg(target_arch = "x86_64")]
const COMMON_INPUT_REG: u8 = 2;
#[cfg(target_arch = "x86_64")]
const XOR_JIT_PREFETCH_STUB_BIAS_BYTES: usize = 128;
#[cfg(target_arch = "x86_64")]
// Keep memory operands in signed-byte displacement range where possible.
const XOR_JIT_BODY_POINTER_BIAS_BYTES: u32 = 128;
#[cfg(target_arch = "x86_64")]
#[cfg(target_arch = "x86_64")]
const XOR_JIT_TURBO_JIT_SIZE: usize = 4096;
#[cfg(target_arch = "x86_64")]
const XOR_JIT_TURBO_CODE_SIZE: usize = 1280;
#[cfg(target_arch = "x86_64")]
const XOR_JIT_TURBO_STUB_BIAS_BYTES: usize =
    bitplane::AVX2_BLOCK_BYTES - XOR_JIT_BODY_POINTER_BIAS_BYTES as usize;

#[cfg(target_arch = "x86_64")]
impl XorJitPreparedCoeff {
    #[inline]
    pub fn new(coefficient: u16) -> Self {
        Self {
            coefficient,
            bitplane_plan: Arc::new(OnceLock::new()),
        }
    }

    fn bitplane_plan(&self) -> &BitplaneCoeffPlan {
        self.bitplane_plan
            .get_or_init(|| BitplaneCoeffPlan::new(self.coefficient))
    }

    #[inline]
    pub fn coefficient(&self) -> u16 {
        self.coefficient
    }

    pub fn ensure_bitplane_emitted(&self) {
        let _ = self.bitplane_plan();
    }
}

#[cfg(target_arch = "x86_64")]
pub struct XorJitPreparedCoeffCache {
    entries: Vec<Option<XorJitPreparedCoeff>>,
}

#[cfg(target_arch = "x86_64")]
impl XorJitPreparedCoeffCache {
    pub fn new() -> Self {
        Self {
            entries: vec![None; u16::MAX as usize + 1],
        }
    }

    pub fn prepare(&mut self, coefficient: u16) -> XorJitPreparedCoeff {
        let entry = &mut self.entries[coefficient as usize];
        match entry {
            Some(prepared) => prepared.clone(),
            None => {
                let prepared = XorJitPreparedCoeff::new(coefficient);
                *entry = Some(prepared.clone());
                prepared
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
impl Default for XorJitPreparedCoeffCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
impl BitplaneCoeffPlan {
    fn new(coefficient: u16) -> Self {
        let output_masks =
            std::array::from_fn(|output_bit| input_dependency_mask(coefficient, output_bit));
        let turbo_pairs = std::array::from_fn(|physical_pair| {
            turbo_output_pair_plan(&output_masks, physical_pair)
        });

        Self {
            coefficient,
            output_masks,
            turbo_pairs,
        }
    }

    fn coefficient(&self) -> u16 {
        self.coefficient
    }

    fn input_mask_for_output_bit(&self, output_bit: usize) -> u16 {
        debug_assert!(output_bit < 16);
        self.output_masks[output_bit]
    }

    fn output_bit_depends_on(&self, output_bit: usize, input_bit: usize) -> bool {
        debug_assert!(input_bit < 16);
        self.input_mask_for_output_bit(output_bit) & (1 << input_bit) != 0
    }

    fn turbo_pair(&self, physical_pair: usize) -> TurboOutputPairPlan {
        debug_assert!(physical_pair < self.turbo_pairs.len());
        self.turbo_pairs[physical_pair]
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
    Jit,
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
type LaneKernelFn = unsafe extern "sysv64" fn(*const u8, *mut u8);

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
type ChunkKernelFn = unsafe extern "sysv64" fn(*const u8, *mut u8, usize);

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
type ChunkKernelPrefetchFn = unsafe extern "sysv64" fn(*const u8, *mut u8, usize, *const u8);

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
struct XorJitLaneKernel {
    code: exec_mem::ExecutableBuffer,
    function: LaneKernelFn,
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
struct XorJitGeneratedBitplaneKernel {
    code: exec_mem::ExecutableBuffer,
    function: ChunkKernelFn,
    prefetch_code: exec_mem::ExecutableBuffer,
    prefetch_function: ChunkKernelPrefetchFn,
    turbo_stub_bias: bool,
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
pub struct XorJitBitplaneKernel {
    kernel: XorJitGeneratedBitplaneKernel,
}

#[cfg(target_arch = "x86_64")]
pub struct XorJitBitplaneScratch {
    code: exec_mem::MutableExecutableBuffer,
    function: ChunkKernelFn,
    prefetch_function: ChunkKernelPrefetchFn,
    loaded: Option<(u16, usize, bool)>,
    body_static_prefix_loaded: bool,
}

#[cfg(target_arch = "x86_64")]
impl XorJitBitplaneScratch {
    pub fn new() -> std::io::Result<Self> {
        let mut code = exec_mem::MutableExecutableBuffer::new(XOR_JIT_TURBO_JIT_SIZE)?;
        code.overwrite(&[0xc5, 0xf8, 0x77, 0xc3])?;
        register_perf_map_range(
            code.as_ptr(),
            code.capacity(),
            "par2rs_xor_jit_bitplane_scratch_body",
        );
        let function = unsafe { code.function() };
        let prefetch_function = unsafe { code.function() };

        Ok(Self {
            code,
            function,
            prefetch_function,
            loaded: None,
            body_static_prefix_loaded: false,
        })
    }

    pub fn multiply_add_chunks_with_prefetch(
        &mut self,
        prepared: &XorJitPreparedCoeff,
        input: &[u8],
        output: &mut [u8],
        prefetch: Option<*const u8>,
    ) {
        assert_prepared_chunk_shape(input, output);

        if input.is_empty() {
            return;
        }

        let coefficient = prepared.coefficient();
        if coefficient == 0 {
            return;
        }

        if let Some(prefetch_ptr) = prefetch {
            self.load_prefetch(prepared.bitplane_plan())
                .expect("load mutable prefetch xor-jit code");
            unsafe {
                call_turbo_bitplane_jit(
                    self.code.as_ptr(),
                    input.as_ptr(),
                    output.as_mut_ptr(),
                    input.len(),
                    xor_jit_biased_prefetch_ptr(prefetch_ptr),
                );
                xor_jit_zeroupper();
            }
        } else {
            self.load_body(prepared.bitplane_plan())
                .expect("load mutable xor-jit code");
            unsafe {
                call_turbo_bitplane_jit(
                    self.code.as_ptr(),
                    input.as_ptr(),
                    output.as_mut_ptr(),
                    input.len(),
                    std::ptr::null(),
                );
                xor_jit_zeroupper();
            }
        }
    }

    fn load_body(&mut self, plan: &BitplaneCoeffPlan) -> std::io::Result<()> {
        self.load_generated(plan, false)
    }

    fn load_prefetch(&mut self, plan: &BitplaneCoeffPlan) -> std::io::Result<()> {
        self.load_generated(plan, true)
    }

    fn load_generated(&mut self, plan: &BitplaneCoeffPlan, prefetch: bool) -> std::io::Result<()> {
        let coefficient = plan.coefficient();

        if self.code.capacity() < XOR_JIT_TURBO_JIT_SIZE {
            self.code = exec_mem::MutableExecutableBuffer::new(XOR_JIT_TURBO_JIT_SIZE)?;
            self.loaded = None;
            self.body_static_prefix_loaded = false;
            register_perf_map_range(
                self.code.as_ptr(),
                self.code.capacity(),
                "par2rs_xor_jit_bitplane_scratch_body",
            );
        }
        let label = if prefetch { "prefetch" } else { "body" };
        let static_prefix = xor_jit_body_static_prefix();

        if !self.body_static_prefix_loaded {
            self.code.overwrite_at(0, static_prefix)?;
            self.body_static_prefix_loaded = true;
        }
        self.code
            .clear_cacheline_bytes_at(static_prefix.len(), XOR_JIT_TURBO_CODE_SIZE)?;
        self.code.set_len_for_overwrite(static_prefix.len())?;
        let dynamic_len = emit_bitplane_chunk_program_dynamic_into(
            plan,
            prefetch,
            static_prefix.len(),
            &mut self.code,
        );
        let generated_len = static_prefix.len() + dynamic_len;

        if xor_jit_dump_dir().is_some() {
            let bytes = self.code.copy_prefix(generated_len)?;
            dump_scratch_program(label, coefficient, &bytes);
        }
        if perf_map_coefficient_labels_enabled() {
            let symbol = if prefetch {
                format!("par2rs_xor_jit_bitplane_scratch_prefetch_coeff_{coefficient:04x}")
            } else {
                format!("par2rs_xor_jit_bitplane_scratch_body_coeff_{coefficient:04x}")
            };
            register_perf_map_range(self.code.as_ptr(), generated_len, &symbol);
        }
        self.function = unsafe { self.code.function() };
        self.prefetch_function = unsafe { self.code.function() };
        self.loaded = Some((coefficient, generated_len, prefetch));
        Ok(())
    }
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn xor_jit_biased_prefetch_ptr(prefetch: *const u8) -> *const u8 {
    prefetch.wrapping_sub(XOR_JIT_PREFETCH_STUB_BIAS_BYTES)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx")]
unsafe fn xor_jit_zeroupper() {
    _mm256_zeroupper();
}

#[cfg(target_arch = "x86_64")]
unsafe fn call_turbo_bitplane_jit(
    function: *const u8,
    input: *const u8,
    output: *mut u8,
    len: usize,
    prefetch: *const u8,
) {
    let src = input.wrapping_sub(XOR_JIT_TURBO_STUB_BIAS_BYTES) as usize;
    let dest = output.wrapping_sub(XOR_JIT_TURBO_STUB_BIAS_BYTES) as usize;
    let dest_end = output.add(len).wrapping_sub(XOR_JIT_TURBO_STUB_BIAS_BYTES) as usize;
    let pf = prefetch as usize;

    std::arch::asm!(
        "call {function}",
        function = in(reg) function,
        inout("rax") src => _,
        inout("rcx") dest_end => _,
        inout("rdx") dest => _,
        inout("rsi") pf => _,
        clobber_abi("sysv64"),
    );
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
impl XorJitLaneKernel {
    fn identity() -> std::io::Result<Self> {
        Self::from_program(identity_lane_program())
    }

    #[allow(dead_code)]
    fn from_program(program: encoder::Program) -> std::io::Result<Self> {
        let (code, function) = compile_lane_program(program, "lane", None)?;
        Ok(Self { code, function })
    }

    unsafe fn run(&self, input: *const u8, output: *mut u8) {
        debug_assert!(!self.code.is_empty());
        (self.function)(input, output);
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
impl XorJitGeneratedBitplaneKernel {
    fn new(coefficient: u16) -> std::io::Result<Self> {
        Self::from_plan(BitplaneCoeffPlan::new(coefficient))
    }

    fn from_plan(plan: BitplaneCoeffPlan) -> std::io::Result<Self> {
        Self::from_plan_ref(&plan)
    }

    fn from_plan_ref(plan: &BitplaneCoeffPlan) -> std::io::Result<Self> {
        let (code, function) = compile_bitplane_chunk_program(plan, "bitplane", false)?;
        let (prefetch_code, prefetch_function) =
            compile_bitplane_chunk_prefetch_program(plan, "bitplane-pf")?;
        Ok(Self {
            code,
            function,
            prefetch_code,
            prefetch_function,
            turbo_stub_bias: true,
        })
    }

    #[allow(dead_code)]
    fn from_program(program: encoder::Program) -> std::io::Result<Self> {
        let (code, function) = compile_chunk_program(program.clone(), "bitplane", None)?;
        let (prefetch_code, prefetch_function) =
            compile_chunk_prefetch_program(program, "bitplane-pf", None)?;
        Ok(Self {
            code,
            function,
            prefetch_code,
            prefetch_function,
            turbo_stub_bias: false,
        })
    }

    unsafe fn multiply_add(&self, input: *const u8, output: *mut u8, len: usize) {
        debug_assert!(!self.code.is_empty());
        if self.turbo_stub_bias {
            call_turbo_bitplane_jit(self.code.as_ptr(), input, output, len, std::ptr::null());
        } else {
            (self.function)(input, output, len);
        }
        xor_jit_zeroupper();
    }

    unsafe fn multiply_add_prefetch(
        &self,
        input: *const u8,
        output: *mut u8,
        len: usize,
        prefetch: *const u8,
    ) {
        debug_assert!(!self.prefetch_code.is_empty());
        if self.turbo_stub_bias {
            call_turbo_bitplane_jit(
                self.prefetch_code.as_ptr(),
                input,
                output,
                len,
                xor_jit_biased_prefetch_ptr(prefetch),
            );
        } else {
            (self.prefetch_function)(input, output, len, prefetch);
        }
        xor_jit_zeroupper();
    }

    fn multiply_add_chunks(&self, input: &[u8], output: &mut [u8]) {
        assert_prepared_chunk_shape(input, output);

        if input.is_empty() {
            return;
        }

        unsafe {
            self.multiply_add(input.as_ptr(), output.as_mut_ptr(), input.len());
        }
    }

    pub fn multiply_add_chunks_with_prefetch(
        &self,
        input: &[u8],
        output: &mut [u8],
        prefetch: Option<*const u8>,
    ) {
        assert_prepared_chunk_shape(input, output);

        if input.is_empty() {
            return;
        }

        unsafe {
            match prefetch {
                Some(prefetch) => {
                    self.multiply_add_prefetch(
                        input.as_ptr(),
                        output.as_mut_ptr(),
                        input.len(),
                        prefetch,
                    );
                }
                None => self.multiply_add(input.as_ptr(), output.as_mut_ptr(), input.len()),
            }
        }
    }

    pub fn multiply_add_block(&self, input: &[u8], output: &mut [u8]) {
        assert_eq!(input.len(), bitplane::AVX2_BLOCK_BYTES);
        assert_eq!(output.len(), bitplane::AVX2_BLOCK_BYTES);
        unsafe {
            self.multiply_add(
                input.as_ptr(),
                output.as_mut_ptr(),
                bitplane::AVX2_BLOCK_BYTES,
            );
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
impl XorJitBitplaneKernel {
    pub fn new(prepared: &XorJitPreparedCoeff) -> std::io::Result<Self> {
        Ok(Self {
            kernel: XorJitGeneratedBitplaneKernel::from_plan_ref(prepared.bitplane_plan())?,
        })
    }

    pub fn multiply_add_chunks(&self, input: &[u8], output: &mut [u8]) {
        self.kernel.multiply_add_chunks(input, output);
    }

    pub fn multiply_add_chunks_with_prefetch(
        &self,
        input: &[u8],
        output: &mut [u8],
        prefetch: Option<*const u8>,
    ) {
        self.kernel
            .multiply_add_chunks_with_prefetch(input, output, prefetch);
    }

    pub fn multiply_add_block(&self, input: &[u8], output: &mut [u8]) {
        self.kernel.multiply_add_block(input, output);
    }
}

#[cfg(target_arch = "x86_64")]
pub fn prepare_xor_jit_bitplane_chunks(dst: &mut [u8], src: &[u8]) -> usize {
    bitplane::prepare_avx2(dst, src)
}

#[cfg(target_arch = "x86_64")]
pub fn prepare_xor_jit_bitplane_segment(dst: &mut [u8], src: &[u8]) {
    assert_eq!(dst.len() % bitplane::AVX2_BLOCK_BYTES, 0);
    assert!(src.len() <= dst.len());

    dst.fill(0);
    let prepared_len = bitplane::prepare_avx2(dst, src);
    assert!(prepared_len <= dst.len());
}

#[cfg(target_arch = "x86_64")]
pub fn finish_xor_jit_bitplane_chunks(dst: &mut [u8], prepared: &[u8]) {
    assert_eq!(prepared.len() % bitplane::AVX2_BLOCK_BYTES, 0);
    assert!(prepared.len() >= dst.len().next_multiple_of(bitplane::AVX2_BLOCK_BYTES));

    let full_len = dst.len() / bitplane::AVX2_BLOCK_BYTES * bitplane::AVX2_BLOCK_BYTES;
    for (prepared_block, output_block) in prepared[..full_len]
        .chunks_exact(bitplane::AVX2_BLOCK_BYTES)
        .zip(dst[..full_len].chunks_exact_mut(bitplane::AVX2_BLOCK_BYTES))
    {
        bitplane::finish_avx2_block(
            output_block.try_into().expect("full output block"),
            prepared_block.try_into().expect("full prepared block"),
        );
    }

    if full_len < dst.len() {
        let tail_len = dst.len() - full_len;
        let mut finished_block = [0u8; bitplane::AVX2_BLOCK_BYTES];
        bitplane::finish_avx2_block(
            &mut finished_block,
            prepared[full_len..full_len + bitplane::AVX2_BLOCK_BYTES]
                .try_into()
                .expect("partial prepared block"),
        );
        dst[full_len..].copy_from_slice(&finished_block[..tail_len]);
    }
}

#[cfg(target_arch = "x86_64")]
pub fn xor_prepared_bitplane_chunks(
    input: &[u8],
    output: &mut [u8],
    prefetch: Option<(*const u8, BitplaneAddPrefetchKind)>,
) {
    assert_prepared_chunk_shape(input, output);

    if input.is_empty() {
        return;
    }

    unsafe {
        xor_prepared_bitplane_chunks_avx2_one(
            input.as_ptr(),
            output.as_mut_ptr(),
            input.len(),
            prefetch,
        );
        xor_jit_zeroupper();
    }
}

#[cfg(target_arch = "x86_64")]
pub fn xor_prepared_bitplane_multi_chunks(
    inputs: &[*const u8],
    len: usize,
    output: &mut [u8],
    prefetch_in: Option<*const u8>,
    prefetch_out: Option<*const u8>,
) {
    assert_eq!(output.len(), len);
    assert_eq!(len % bitplane::AVX2_BLOCK_BYTES, 0);
    assert_eq!(output.as_ptr() as usize % 32, 0);

    if inputs.is_empty() || len == 0 {
        return;
    }

    unsafe {
        xor_prepared_bitplane_multi_chunks_avx2(
            inputs,
            output.as_mut_ptr(),
            len,
            prefetch_in,
            prefetch_out,
        );
    }
}

#[cfg(target_arch = "x86_64")]
pub fn xor_prepared_bitplane_multi_chunks_v1i6(
    inputs: &[*const u8],
    len: usize,
    output: &mut [u8],
    prefetch_in: Option<*const u8>,
    prefetch_out: Option<*const u8>,
) {
    assert_eq!(output.len(), len);
    assert_eq!(len % bitplane::AVX2_BLOCK_BYTES, 0);
    assert_eq!(output.as_ptr() as usize % 32, 0);

    if inputs.is_empty() || len == 0 {
        return;
    }

    unsafe {
        xor_prepared_bitplane_multi_chunks_v1i6_avx2(
            inputs,
            output.as_mut_ptr(),
            len,
            prefetch_in,
            prefetch_out,
        );
    }
}

#[cfg(target_arch = "x86_64")]
pub fn xor_packed_multi_region_v16i1(
    src: *const u8,
    regions: usize,
    len: usize,
    output: &mut [u8],
    prefetch_in: Option<*const u8>,
    prefetch_out: Option<*const u8>,
) {
    assert_eq!(output.len(), len);
    assert_eq!(len % bitplane::AVX2_BLOCK_BYTES, 0);
    assert_eq!(output.as_ptr() as usize % 32, 0);

    if regions == 0 || len == 0 {
        return;
    }

    assert!(!src.is_null());

    unsafe {
        xor_packed_multi_region_v16i1_avx2(
            src,
            regions,
            output.as_mut_ptr(),
            len,
            prefetch_in,
            prefetch_out,
        );
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn assert_prepared_chunk_shape(input: &[u8], output: &[u8]) {
    assert_eq!(input.len(), output.len());
    assert_eq!(input.len() % bitplane::AVX2_BLOCK_BYTES, 0);
    assert_eq!(
        input.as_ptr() as usize % 32,
        0,
        "prepared input must be 32-byte aligned"
    );
    assert_eq!(
        output.as_ptr() as usize % 32,
        0,
        "prepared output must be 32-byte aligned"
    );
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn compile_lane_program(
    program: encoder::Program,
    label: &str,
    coefficient: Option<u16>,
) -> std::io::Result<(exec_mem::ExecutableBuffer, LaneKernelFn)> {
    let generated = program.finish();
    dump_generated_program(label, coefficient, &generated);
    let mut code = exec_mem::ExecutableBuffer::new(generated.len())?;
    code.write(&generated)?;
    register_perf_map_symbol(&code, label, coefficient);
    let function = unsafe { code.function() };

    Ok((code, function))
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn compile_chunk_program(
    program: encoder::Program,
    label: &str,
    coefficient: Option<u16>,
) -> std::io::Result<(exec_mem::ExecutableBuffer, ChunkKernelFn)> {
    let generated = emit_chunk_program_bytes(program, false);
    dump_generated_program(label, coefficient, &generated);
    let mut code = exec_mem::ExecutableBuffer::new(generated.len())?;
    code.write(&generated)?;
    register_perf_map_symbol(&code, label, coefficient);
    let function = unsafe { code.function() };

    Ok((code, function))
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn compile_bitplane_chunk_program(
    plan: &BitplaneCoeffPlan,
    label: &str,
    prefetch: bool,
) -> std::io::Result<(exec_mem::ExecutableBuffer, ChunkKernelFn)> {
    let generated = emit_bitplane_chunk_program_bytes(plan, prefetch);
    dump_generated_program(label, Some(plan.coefficient()), &generated);
    let mut code = exec_mem::ExecutableBuffer::new(generated.len())?;
    code.write(&generated)?;
    register_perf_map_symbol(&code, label, Some(plan.coefficient()));
    let function = unsafe { code.function() };

    Ok((code, function))
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn compile_chunk_prefetch_program(
    program: encoder::Program,
    label: &str,
    coefficient: Option<u16>,
) -> std::io::Result<(exec_mem::ExecutableBuffer, ChunkKernelPrefetchFn)> {
    let generated = emit_chunk_program_bytes(program, true);
    dump_generated_program(label, coefficient, &generated);
    let mut code = exec_mem::ExecutableBuffer::new(generated.len())?;
    code.write(&generated)?;
    register_perf_map_symbol(&code, label, coefficient);
    let function = unsafe { code.function() };

    Ok((code, function))
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn compile_bitplane_chunk_prefetch_program(
    plan: &BitplaneCoeffPlan,
    label: &str,
) -> std::io::Result<(exec_mem::ExecutableBuffer, ChunkKernelPrefetchFn)> {
    let generated = emit_bitplane_chunk_program_bytes(plan, true);
    dump_generated_program(label, Some(plan.coefficient()), &generated);
    let mut code = exec_mem::ExecutableBuffer::new(generated.len())?;
    code.write(&generated)?;
    register_perf_map_symbol(&code, label, Some(plan.coefficient()));
    let function = unsafe { code.function() };

    Ok((code, function))
}

#[cfg(target_arch = "x86_64")]
fn emit_chunk_program_bytes(program: encoder::Program, prefetch: bool) -> Vec<u8> {
    if prefetch {
        program.finish_block_loop_with_prefetch_and_pointer_bias(
            bitplane::AVX2_BLOCK_BYTES as u32,
            256,
            XOR_JIT_BODY_POINTER_BIAS_BYTES,
        )
    } else {
        program.finish_block_loop_with_pointer_bias(
            bitplane::AVX2_BLOCK_BYTES as u32,
            XOR_JIT_BODY_POINTER_BIAS_BYTES,
        )
    }
}

#[cfg(target_arch = "x86_64")]
fn emit_bitplane_chunk_program_bytes(plan: &BitplaneCoeffPlan, prefetch: bool) -> Vec<u8> {
    let static_prefix = xor_jit_body_static_prefix();
    let mut encoded = Vec::with_capacity(static_prefix.len() + 1024);
    encoded.extend_from_slice(static_prefix);
    let _ =
        emit_bitplane_chunk_program_dynamic_into(plan, prefetch, static_prefix.len(), &mut encoded);
    encoded
}

#[cfg(target_arch = "x86_64")]
fn emit_bitplane_chunk_program_dynamic_into<S: encoder::ByteSink>(
    plan: &BitplaneCoeffPlan,
    prefetch: bool,
    static_prefix_len: usize,
    encoded: &mut S,
) -> usize {
    encoder::encode_block_loop_dynamic_after_static_prefix_no_vzeroupper_into(
        static_prefix_len,
        bitplane::AVX2_BLOCK_BYTES as u32,
        prefetch.then_some(256),
        encoded,
        |program| {
            (0..8).fold(program, |program, bit| {
                emit_output_pair_turbo_sink(program, plan.turbo_pair(bit))
            })
        },
    )
}

#[cfg(target_arch = "x86_64")]
fn xor_jit_body_static_prefix() -> &'static [u8] {
    static BODY_PREFIX: OnceLock<Box<[u8]>> = OnceLock::new();

    BODY_PREFIX.get_or_init(|| {
        bitplane_preload_program()
            .finish_turbo_block_loop_prefix()
            .into_boxed_slice()
    })
}

#[cfg(target_arch = "x86_64")]
fn bitplane_preload_program() -> encoder::Program {
    InputPreloadPlan::fixed().emit_preloads(encoder::Program::new())
}

#[cfg(target_arch = "x86_64")]
fn register_perf_map_symbol(
    code: &exec_mem::ExecutableBuffer,
    label: &str,
    coefficient: Option<u16>,
) {
    if !perf_map_enabled() {
        return;
    }

    let coeff = coefficient
        .map(|value| format!("coeff_{value:04x}"))
        .unwrap_or_else(|| "coeff_none".to_string());
    let name = format!("par2rs_xor_jit_{}_{}", label.replace('-', "_"), coeff);
    register_perf_map_range(code.as_ptr(), code.len(), &name);
}

#[cfg(target_arch = "x86_64")]
fn register_perf_map_range(addr: *const u8, len: usize, name: &str) {
    if !perf_map_enabled() || len == 0 {
        return;
    }

    static PERF_MAP_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = PERF_MAP_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("lock perf map writer");

    let path = std::path::Path::new("/tmp").join(format!("perf-{}.map", std::process::id()));
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let _ = writeln!(file, "{:x} {:x} {name}", addr as usize, len);
    }
}

#[cfg(target_arch = "x86_64")]
fn perf_map_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("PAR2RS_XOR_JIT_PERF_MAP").as_deref() == Ok("1"))
}

#[cfg(target_arch = "x86_64")]
fn perf_map_coefficient_labels_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        perf_map_enabled()
            && std::env::var("PAR2RS_XOR_JIT_PERF_COEFF_LABELS").as_deref() != Ok("0")
    })
}

#[cfg(target_arch = "x86_64")]
fn xor_jit_dump_dir() -> Option<&'static std::path::Path> {
    static DUMP_DIR: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    DUMP_DIR
        .get_or_init(|| std::env::var_os("PAR2RS_XOR_JIT_DUMP_DIR").map(std::path::PathBuf::from))
        .as_deref()
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn dump_generated_program(label: &str, coefficient: Option<u16>, generated: &[u8]) {
    let Some(dir) = xor_jit_dump_dir() else {
        return;
    };

    if std::fs::create_dir_all(dir).is_err() {
        return;
    }

    static DUMP_COUNTER: AtomicUsize = AtomicUsize::new(0);
    let index = DUMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let coeff = coefficient
        .map(|value| format!("coeff-{value:04x}"))
        .unwrap_or_else(|| "coeff-none".to_string());
    let path = dir.join(format!("{index:06}-{label}-{coeff}.bin"));
    let _ = std::fs::write(path, generated);
}

#[cfg(target_arch = "x86_64")]
fn dump_scratch_program(label: &str, coefficient: u16, generated: &[u8]) {
    let Some(dir) = xor_jit_dump_dir() else {
        return;
    };

    if std::fs::create_dir_all(dir).is_err() {
        return;
    }

    static SCRATCH_DUMP_COUNTER: AtomicUsize = AtomicUsize::new(0);
    let index = SCRATCH_DUMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!(
        "{index:06}-scratch-{label}-coeff-{coefficient:04x}.bin"
    ));
    let _ = std::fs::write(path, generated);
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
#[allow(dead_code)]
fn bitplane_multiply_add_program(plan: &BitplaneCoeffPlan) -> encoder::Program {
    bitplane_multiply_add_body_program(plan).vzeroupper().ret()
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn bitplane_multiply_add_body_program(plan: &BitplaneCoeffPlan) -> encoder::Program {
    let preloads = InputPreloadPlan::new(plan);
    (0..8).fold(
        preloads.emit_preloads(encoder::Program::new()),
        |program, bit| emit_output_bitplane_pair(program, plan, &preloads, bit),
    )
}

#[cfg(target_arch = "x86_64")]
#[allow(dead_code)]
fn bitplane_multiply_add_dynamic_program(plan: &BitplaneCoeffPlan) -> encoder::Program {
    let preloads = InputPreloadPlan::new(plan);
    (0..8).fold(encoder::Program::new(), |program, bit| {
        emit_output_bitplane_pair(program, plan, &preloads, bit)
    })
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
impl InputPreloadPlan {
    fn new(_plan: &BitplaneCoeffPlan) -> Self {
        Self::fixed()
    }

    fn fixed() -> Self {
        let mut registers = [None; 16];

        for physical_bit in 3..16 {
            registers[turbo_physical_word_bit(physical_bit)] = Some(physical_bit as u8);
        }

        Self { registers }
    }

    fn emit_preloads<P: XorJitBitplaneProgram>(&self, program: P) -> P {
        let mut preloads = self
            .registers
            .iter()
            .enumerate()
            .filter_map(|(input_bit, &register)| register.map(|register| (input_bit, register)))
            .collect::<Vec<_>>();
        preloads.sort_by_key(|&(_, register)| register);

        preloads
            .into_iter()
            .fold(program, |program, (input_bit, register)| {
                program.vmovdqa_ymm_from_input_offset(register, bitplane_vector_offset(input_bit))
            })
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn emit_output_bitplane_pair<P: XorJitBitplaneProgram>(
    program: P,
    plan: &BitplaneCoeffPlan,
    _preloads: &InputPreloadPlan,
    physical_pair: usize,
) -> P {
    emit_output_pair_turbo_plan(program, plan.turbo_pair(physical_pair))
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn turbo_physical_word_bit(physical_bit: usize) -> usize {
    debug_assert!(physical_bit < 16);
    15 - physical_bit
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn word_mask_to_turbo_physical_mask(mask: u16) -> u16 {
    input_bits(mask).fold(0, |physical_mask, word_bit| {
        physical_mask | (1 << (15 - word_bit))
    })
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn highest_physical_bit(mask: u16) -> Option<usize> {
    (mask != 0).then(|| 15usize - mask.leading_zeros() as usize)
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn lowest_physical_bit(mask: u16) -> Option<usize> {
    (mask != 0).then(|| mask.trailing_zeros() as usize)
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn physical_bit_mask(physical_bit: usize) -> u16 {
    1 << physical_bit
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn physical_bitplane_vector_offset(physical_bit: usize) -> i32 {
    bitplane_vector_offset(turbo_physical_word_bit(physical_bit))
}

#[cfg(target_arch = "x86_64")]
fn turbo_dep_tables() -> &'static TurboDepTables {
    static TABLES: OnceLock<TurboDepTables> = OnceLock::new();
    TABLES.get_or_init(build_turbo_dep_tables)
}

#[cfg(target_arch = "x86_64")]
fn build_turbo_dep_tables() -> TurboDepTables {
    let mut mem_ops = [[TurboMemDepOp {
        target_reg: 0xff,
        physical_bit: 0xff,
    }; 3]; 64];
    let mut mem_len = [0u8; 64];
    for (i, ops) in mem_ops.iter_mut().enumerate() {
        let mut interleaved =
            (i & 1) | ((i & 8) >> 2) | ((i & 2) << 1) | ((i & 16) >> 1) | ((i & 4) << 2) | (i & 32);
        let mut len = 0usize;
        for physical_bit in 0..3u8 {
            let mask = (interleaved & 0b11) as u8;
            if mask != 0 {
                ops[len] = TurboMemDepOp {
                    target_reg: mask - 1,
                    physical_bit,
                };
                len += 1;
            }
            interleaved >>= 2;
        }
        mem_len[i] = len as u8;
    }

    let mut nums = [[0xffu8; 8]; 128];
    let mut rmask = [[0u8; 8]; 128];
    for dep in 0..128usize {
        let mut pos = 0usize;
        for bit in 0..8usize {
            if dep & (1 << bit) != 0 {
                nums[dep][pos] = bit as u8;
                rmask[dep][bit] = (1 << 3) + 1;
                pos += 1;
            }
        }
    }

    let mem_bytes = (0..64usize)
        .map(|dep| {
            turbo_mem_template_bytes(&mem_ops[dep], mem_len[dep] as usize).into_boxed_slice()
        })
        .collect::<Vec<_>>();
    let main_bytes_low = (0..(128usize * 128))
        .map(|key| {
            let dep1 = (key >> 7) as u8;
            let dep2 = (key & 0x7f) as u8;
            turbo_main_template_bytes(&nums, &rmask, dep1, dep2, false).into_boxed_slice()
        })
        .collect::<Vec<_>>();
    let main_bytes_high = (0..(64usize * 64))
        .map(|key| {
            let dep1 = (key >> 6) as u8;
            let dep2 = (key & 0x3f) as u8;
            turbo_main_template_bytes(&nums, &rmask, dep1, dep2, true).into_boxed_slice()
        })
        .collect::<Vec<_>>();

    TurboDepTables {
        mem_ops,
        mem_len,
        nums,
        rmask,
        mem_bytes,
        main_bytes_low,
        main_bytes_high,
    }
}

#[cfg(target_arch = "x86_64")]
fn turbo_mem_template_bytes(ops: &[TurboMemDepOp; 3], len: usize) -> Vec<u8> {
    let mut program = encoder::Program::new();
    for op in &ops[..len] {
        program = program.vpxor_ymm_rax_offset(
            op.target_reg,
            op.target_reg,
            physical_bitplane_vector_offset(op.physical_bit as usize),
        );
    }
    program.finish()
}

#[cfg(target_arch = "x86_64")]
fn turbo_main_template_bytes(
    nums: &[[u8; 8]; 128],
    rmask: &[[u8; 8]; 128],
    dep1: u8,
    dep2: u8,
    high: bool,
) -> Vec<u8> {
    let dep = (dep1 | dep2) as usize;
    let reg_base = if high { 10 } else { 3 };
    nums[dep]
        .iter()
        .copied()
        .take_while(|&bit| bit != 0xff)
        .fold(encoder::Program::new(), |program, bit| {
            let reg_code =
                rmask[dep1 as usize][bit as usize] | (rmask[dep2 as usize][bit as usize] << 1);
            let target_reg = match reg_code {
                9 => 0,
                18 => 1,
                27 => COMMON_INPUT_REG,
                _ => unreachable!("unexpected turbo dep register code {reg_code}"),
            };
            program.vpxor_ymm(target_reg, reg_base + bit, target_reg)
        })
        .finish()
}

#[cfg(target_arch = "x86_64")]
fn turbo_dep_plan(first_remaining_mask: u16, second_remaining_mask: u16) -> TurboDepPlan {
    TurboDepPlan {
        mem_deps: ((first_remaining_mask & 0x7) | ((second_remaining_mask & 0x7) << 3)) as u8,
        dep1_low: ((first_remaining_mask >> 3) & 0x7f) as u8,
        dep1_high: ((first_remaining_mask >> 10) & 0x3f) as u8,
        dep2_low: ((second_remaining_mask >> 3) & 0x7f) as u8,
        dep2_high: ((second_remaining_mask >> 10) & 0x3f) as u8,
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn turbo_output_pair_plan(output_masks: &[u16; 16], physical_pair: usize) -> TurboOutputPairPlan {
    let first_output = turbo_physical_word_bit(physical_pair * 2);
    let second_output = turbo_physical_word_bit(physical_pair * 2 + 1);
    let mut first_mask = word_mask_to_turbo_physical_mask(output_masks[first_output]);
    let mut second_mask = word_mask_to_turbo_physical_mask(output_masks[second_output]);
    let common_mask = first_mask & second_mask;
    let common = turbo_common_plan(common_mask);
    first_mask &= !common.eliminated_mask;
    second_mask &= !common.eliminated_mask;

    let first_seed = highest_physical_bit(first_mask);
    if let Some(seed) = first_seed {
        first_mask &= !physical_bit_mask(seed);
    }

    let second_seed = highest_physical_bit(second_mask);
    if let Some(seed) = second_seed {
        second_mask &= !physical_bit_mask(seed);
    }
    let deps = turbo_dep_plan(first_mask, second_mask);

    TurboOutputPairPlan {
        first_output,
        second_output,
        first_seed,
        second_seed,
        first_remaining_mask: first_mask,
        second_remaining_mask: second_mask,
        deps,
        common,
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn turbo_common_plan(common_mask: u16) -> TurboCommonPlan {
    let Some(lowest) = lowest_physical_bit(common_mask) else {
        return TurboCommonPlan {
            lowest: None,
            highest: None,
            eliminated_mask: 0,
        };
    };
    let lowest_mask = physical_bit_mask(lowest);
    let common_without_lowest = common_mask & !lowest_mask;
    let highest = highest_physical_bit(common_without_lowest);
    let highest_mask = highest.map(physical_bit_mask).unwrap_or(0);

    TurboCommonPlan {
        lowest: Some(lowest),
        highest,
        eliminated_mask: lowest_mask | highest_mask,
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn emit_output_pair_turbo_plan<P: XorJitBitplaneProgram>(
    program: P,
    pair: TurboOutputPairPlan,
) -> P {
    let program = emit_turbo_seeded_output_load(program, 0, pair.first_output, pair.first_seed);
    let program = emit_turbo_seeded_output_load(program, 1, pair.second_output, pair.second_seed);

    let (program, common_reg, common_active) =
        emit_turbo_common_accumulator(program, COMMON_INPUT_REG, pair.common);
    let program = emit_turbo_mem_deps(program, pair.deps.mem_deps);
    let program = emit_turbo_main_deps(program, pair.deps.dep1_low, pair.deps.dep2_low, false);
    let program = emit_turbo_main_deps(program, pair.deps.dep1_high, pair.deps.dep2_high, true);
    let program = if common_active {
        program
            .vpxor_ymm(0, common_reg, 0)
            .vpxor_ymm(1, common_reg, 1)
    } else {
        program
    };

    program
        .vmovdqa_output_offset_from_ymm(bitplane_vector_offset(pair.first_output), 0)
        .vmovdqa_output_offset_from_ymm(bitplane_vector_offset(pair.second_output), 1)
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn emit_output_pair_turbo_sink<'a, S: encoder::ByteSink>(
    program: encoder::ProgramSink<'a, S>,
    pair: TurboOutputPairPlan,
) -> encoder::ProgramSink<'a, S> {
    let program =
        emit_turbo_muladd_output_seed_sink(program, 0, pair.first_output, pair.first_seed);
    let program =
        emit_turbo_muladd_output_seed_sink(program, 1, pair.second_output, pair.second_seed);
    let (program, common_reg, common_active) = emit_turbo_load_part_sink(
        program,
        COMMON_INPUT_REG,
        pair.common.lowest,
        pair.common.highest,
    );
    let tables = turbo_dep_tables();
    let dep_key = ((pair.deps.dep1_low as usize) << 7) | pair.deps.dep2_low as usize;
    let dep_key_high = ((pair.deps.dep1_high as usize) << 6) | pair.deps.dep2_high as usize;
    let program = program
        .emit_bytes(&tables.mem_bytes[pair.deps.mem_deps as usize])
        .emit_bytes(&tables.main_bytes_low[dep_key])
        .emit_bytes(&tables.main_bytes_high[dep_key_high]);
    let program = if common_active {
        program
            .vpxor_ymm(0, common_reg, 0)
            .vpxor_ymm(1, common_reg, 1)
    } else {
        program
    };
    program
        .vmovdqa_output_offset_from_ymm(bitplane_vector_offset(pair.first_output), 0)
        .vmovdqa_output_offset_from_ymm(bitplane_vector_offset(pair.second_output), 1)
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn emit_turbo_muladd_output_seed_sink<'a, S: encoder::ByteSink>(
    program: encoder::ProgramSink<'a, S>,
    output_reg: u8,
    output_bit: usize,
    highest: Option<usize>,
) -> encoder::ProgramSink<'a, S> {
    let output_offset = bitplane_vector_offset(output_bit);
    match highest {
        Some(highest) if highest > 2 => {
            program.vpxor_ymm_output_offset(output_reg, highest as u8, output_offset)
        }
        Some(highest) => program
            .vmovdqa_ymm_from_output_offset(output_reg, output_offset)
            .vpxor_ymm_input_offset(
                output_reg,
                output_reg,
                physical_bitplane_vector_offset(highest),
            ),
        None => program.vmovdqa_ymm_from_output_offset(output_reg, output_offset),
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn emit_turbo_load_part_sink<'a, S: encoder::ByteSink>(
    program: encoder::ProgramSink<'a, S>,
    reg: u8,
    lowest: Option<usize>,
    highest: Option<usize>,
) -> (encoder::ProgramSink<'a, S>, u8, bool) {
    let Some(lowest) = lowest else {
        return (program, reg, false);
    };

    let result = if lowest < 3 {
        match highest {
            Some(highest) if highest > 2 => program.vpxor_ymm_input_offset(
                reg,
                highest as u8,
                physical_bitplane_vector_offset(lowest),
            ),
            Some(highest) => program
                .vmovdqa_ymm_from_input_offset(reg, physical_bitplane_vector_offset(highest))
                .vpxor_ymm_input_offset(reg, reg, physical_bitplane_vector_offset(lowest)),
            None => {
                program.vmovdqa_ymm_from_input_offset(reg, physical_bitplane_vector_offset(lowest))
            }
        }
    } else {
        match highest {
            Some(highest) => program.vpxor_ymm(reg, highest as u8, lowest as u8),
            None => program.vmovdqa_ymm(reg, lowest as u8),
        }
    };

    (result, reg, true)
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn emit_turbo_mem_deps<P: XorJitBitplaneProgram>(mut program: P, mem_deps: u8) -> P {
    let tables = turbo_dep_tables();
    let idx = mem_deps as usize;
    for op in &tables.mem_ops[idx][..tables.mem_len[idx] as usize] {
        program = xor_physical_input_bit(
            program,
            op.target_reg,
            op.target_reg,
            op.physical_bit as usize,
        );
    }
    program
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn emit_turbo_main_deps<P: XorJitBitplaneProgram>(program: P, dep1: u8, dep2: u8, high: bool) -> P {
    let tables = turbo_dep_tables();
    let dep = (dep1 | dep2) as usize;
    let reg_base = if high { 10 } else { 3 };
    tables.nums[dep]
        .iter()
        .copied()
        .take_while(|&bit| bit != 0xff)
        .fold(program, |program, bit| {
            let reg_code = tables.rmask[dep1 as usize][bit as usize]
                | (tables.rmask[dep2 as usize][bit as usize] << 1);
            let target_reg = match reg_code {
                9 => 0,
                18 => 1,
                27 => COMMON_INPUT_REG,
                _ => unreachable!("unexpected turbo dep register code {reg_code}"),
            };
            program.vpxor_ymm(target_reg, reg_base + bit, target_reg)
        })
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn emit_turbo_seeded_output_load<P: XorJitBitplaneProgram>(
    program: P,
    output_reg: u8,
    output_bit: usize,
    seed: Option<usize>,
) -> P {
    let output_offset = bitplane_vector_offset(output_bit);
    let Some(seed) = seed else {
        return program.vmovdqa_ymm_from_output_offset(output_reg, output_offset);
    };

    if seed > 2 {
        program.vpxor_ymm_output_offset(output_reg, seed as u8, output_offset)
    } else {
        program
            .vmovdqa_ymm_from_output_offset(output_reg, output_offset)
            .vpxor_ymm_input_offset(
                output_reg,
                output_reg,
                physical_bitplane_vector_offset(seed),
            )
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn emit_turbo_common_accumulator<P: XorJitBitplaneProgram>(
    program: P,
    accumulator_reg: u8,
    common: TurboCommonPlan,
) -> (P, u8, bool) {
    let Some(lowest) = common.lowest else {
        return (program, accumulator_reg, false);
    };

    match (lowest, common.highest) {
        (0..=2, Some(highest)) if highest > 2 => (
            program.vpxor_ymm_input_offset(
                accumulator_reg,
                highest as u8,
                physical_bitplane_vector_offset(lowest),
            ),
            accumulator_reg,
            true,
        ),
        (0..=2, Some(highest)) => (
            program
                .vmovdqa_ymm_from_input_offset(
                    accumulator_reg,
                    physical_bitplane_vector_offset(highest),
                )
                .vpxor_ymm_input_offset(
                    accumulator_reg,
                    accumulator_reg,
                    physical_bitplane_vector_offset(lowest),
                ),
            accumulator_reg,
            true,
        ),
        (0..=2, None) => (
            program.vmovdqa_ymm_from_input_offset(
                accumulator_reg,
                physical_bitplane_vector_offset(lowest),
            ),
            accumulator_reg,
            true,
        ),
        (_, Some(highest)) => (
            program.vpxor_ymm(accumulator_reg, highest as u8, lowest as u8),
            accumulator_reg,
            true,
        ),
        (_, None) => (
            program.vmovdqa_ymm(accumulator_reg, lowest as u8),
            accumulator_reg,
            true,
        ),
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn xor_physical_input_bit<P: XorJitBitplaneProgram>(
    program: P,
    output_reg: u8,
    lhs_reg: u8,
    physical_bit: usize,
) -> P {
    if physical_bit > 2 {
        program.vpxor_ymm(output_reg, lhs_reg, physical_bit as u8)
    } else {
        program.vpxor_ymm_input_offset(
            output_reg,
            lhs_reg,
            physical_bitplane_vector_offset(physical_bit),
        )
    }
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

    bitplane::mask_offset(half, bit_from_msb, 0) as i32 - XOR_JIT_BODY_POINTER_BIAS_BYTES as i32
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitplaneAddPrefetchKind {
    Output,
    Input,
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
        XorJitFlavor::Jit => multiply_vec_clmul(input, coeff, constants),
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
#[target_feature(enable = "avx2")]
unsafe fn xor_prepared_bitplane_chunks_avx2_one(
    input: *const u8,
    output: *mut u8,
    len: usize,
    prefetch: Option<(*const u8, BitplaneAddPrefetchKind)>,
) {
    use std::arch::x86_64::{
        __m256i, _mm256_load_si256, _mm256_store_si256, _mm256_xor_si256, _mm_prefetch,
        _MM_HINT_ET1, _MM_HINT_T1,
    };

    debug_assert_eq!(len % bitplane::AVX2_BLOCK_BYTES, 0);
    debug_assert_eq!(input as usize % 32, 0);
    debug_assert_eq!(output as usize % 32, 0);

    let mut pos = 0usize;
    while pos < len {
        for vec_idx in 0..16usize {
            let offset = pos + vec_idx * 32;
            let in_vec = _mm256_load_si256(input.add(offset).cast::<__m256i>());
            let out_vec = _mm256_load_si256(output.add(offset).cast::<__m256i>());
            _mm256_store_si256(
                output.add(offset).cast::<__m256i>(),
                _mm256_xor_si256(out_vec, in_vec),
            );
        }

        if let Some((prefetch_ptr, kind)) = prefetch {
            let pf_base = prefetch_ptr.add(pos >> 1).cast::<i8>();
            match kind {
                BitplaneAddPrefetchKind::Output => {
                    _mm_prefetch::<{ _MM_HINT_ET1 }>(pf_base);
                    _mm_prefetch::<{ _MM_HINT_ET1 }>(pf_base.add(64));
                    _mm_prefetch::<{ _MM_HINT_ET1 }>(pf_base.add(128));
                    _mm_prefetch::<{ _MM_HINT_ET1 }>(pf_base.add(192));
                }
                BitplaneAddPrefetchKind::Input => {
                    _mm_prefetch::<{ _MM_HINT_T1 }>(pf_base);
                    _mm_prefetch::<{ _MM_HINT_T1 }>(pf_base.add(64));
                    _mm_prefetch::<{ _MM_HINT_T1 }>(pf_base.add(128));
                    _mm_prefetch::<{ _MM_HINT_T1 }>(pf_base.add(192));
                }
            }
        }

        pos += bitplane::AVX2_BLOCK_BYTES;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn prefetch_prepared_bitplane_add(
    prefetch_ptr: *const u8,
    pos: usize,
    kind: BitplaneAddPrefetchKind,
) {
    use std::arch::x86_64::{_mm_prefetch, _MM_HINT_ET1, _MM_HINT_T1};

    let pf_base = prefetch_ptr.add(pos >> 1).cast::<i8>();
    match kind {
        BitplaneAddPrefetchKind::Output => {
            _mm_prefetch::<{ _MM_HINT_ET1 }>(pf_base);
            _mm_prefetch::<{ _MM_HINT_ET1 }>(pf_base.add(64));
            _mm_prefetch::<{ _MM_HINT_ET1 }>(pf_base.add(128));
            _mm_prefetch::<{ _MM_HINT_ET1 }>(pf_base.add(192));
        }
        BitplaneAddPrefetchKind::Input => {
            _mm_prefetch::<{ _MM_HINT_T1 }>(pf_base);
            _mm_prefetch::<{ _MM_HINT_T1 }>(pf_base.add(64));
            _mm_prefetch::<{ _MM_HINT_T1 }>(pf_base.add(128));
            _mm_prefetch::<{ _MM_HINT_T1 }>(pf_base.add(192));
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn xor_prepared_bitplane_multi_chunks_avx2_core(
    inputs: &[*const u8],
    output: *mut u8,
    len: usize,
    prefetch: Option<(*const u8, BitplaneAddPrefetchKind)>,
) {
    use std::arch::x86_64::{__m256i, _mm256_load_si256, _mm256_store_si256, _mm256_xor_si256};

    const TURBO_ADD_MAX_SRCS: usize = 18;

    debug_assert_eq!(len % bitplane::AVX2_BLOCK_BYTES, 0);
    debug_assert_eq!(output as usize % 32, 0);
    debug_assert!(inputs.len() <= TURBO_ADD_MAX_SRCS);
    for &input in inputs {
        debug_assert!(!input.is_null());
        debug_assert_eq!(input as usize % 32, 0);
    }

    let mut input_ends = [std::ptr::null::<u8>(); TURBO_ADD_MAX_SRCS];
    for (idx, &input) in inputs.iter().enumerate() {
        input_ends[idx] = input.add(len);
    }
    let src_count = inputs.len();
    let output_end = output.add(len);

    let mut ptr = -(len as isize);
    while ptr != 0 {
        let out_block = output_end.offset(ptr);
        for vec_idx in 0..16usize {
            let out_ptr = out_block.add(vec_idx * 32).cast::<__m256i>();
            let mut data = _mm256_load_si256(out_ptr);
            macro_rules! add_input {
                ($idx:expr) => {
                    if src_count >= $idx + 1 {
                        data = _mm256_xor_si256(
                            data,
                            _mm256_load_si256(
                                input_ends[$idx]
                                    .offset(ptr)
                                    .add(vec_idx * 32)
                                    .cast::<__m256i>(),
                            ),
                        );
                    }
                };
            }
            add_input!(0);
            add_input!(1);
            add_input!(2);
            add_input!(3);
            add_input!(4);
            add_input!(5);
            add_input!(6);
            add_input!(7);
            add_input!(8);
            add_input!(9);
            add_input!(10);
            add_input!(11);
            add_input!(12);
            add_input!(13);
            add_input!(14);
            add_input!(15);
            add_input!(16);
            add_input!(17);
            _mm256_store_si256(out_ptr, data);
        }

        if let Some((prefetch_ptr, kind)) = prefetch {
            prefetch_prepared_bitplane_add(prefetch_ptr, (len as isize + ptr) as usize, kind);
        }

        ptr += bitplane::AVX2_BLOCK_BYTES as isize;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn xor_prepared_bitplane_multi_chunks_avx2(
    inputs: &[*const u8],
    output: *mut u8,
    len: usize,
    prefetch_in: Option<*const u8>,
    prefetch_out: Option<*const u8>,
) {
    const TURBO_ADD_INTERLEAVE: usize = 1;
    const TURBO_ADD_REGIONS_PER_CALL: usize = 6;
    const TURBO_ADD_PREFETCH_FACTOR: usize = 1;
    const TURBO_ADD_OUTPUT_PREFETCH_ROUNDS: usize = 1 << TURBO_ADD_PREFETCH_FACTOR;

    let pf_len = len >> TURBO_ADD_PREFETCH_FACTOR;
    let mut region = 0usize;
    let mut output_pf_rounds = TURBO_ADD_OUTPUT_PREFETCH_ROUNDS;
    let mut prefetch_out_ptr = prefetch_out.map(|ptr| ptr.wrapping_add(pf_len));

    while output_pf_rounds > 0 && inputs.len().saturating_sub(region) >= TURBO_ADD_REGIONS_PER_CALL
    {
        xor_prepared_bitplane_multi_chunks_avx2_core(
            &inputs[region..region + TURBO_ADD_REGIONS_PER_CALL],
            output,
            len,
            prefetch_out_ptr.map(|ptr| (ptr, BitplaneAddPrefetchKind::Output)),
        );
        region += TURBO_ADD_REGIONS_PER_CALL;
        output_pf_rounds -= 1;
        prefetch_out_ptr = if output_pf_rounds > 0 {
            prefetch_out_ptr.map(|ptr| ptr.wrapping_add(pf_len))
        } else {
            None
        };
    }

    let remaining = inputs.len().saturating_sub(region);
    if let Some(prefetch_ptr) = prefetch_out_ptr {
        if remaining >= TURBO_ADD_INTERLEAVE {
            xor_prepared_bitplane_multi_chunks_avx2_core(
                &inputs[region..],
                output,
                len,
                Some((prefetch_ptr, BitplaneAddPrefetchKind::Output)),
            );
            xor_jit_zeroupper();
            return;
        }
    }

    if let Some(prefetch_in_ptr) = prefetch_in {
        let mut prefetch_ptr = prefetch_in_ptr.wrapping_add(pf_len);
        while inputs.len().saturating_sub(region) >= TURBO_ADD_REGIONS_PER_CALL {
            xor_prepared_bitplane_multi_chunks_avx2_core(
                &inputs[region..region + TURBO_ADD_REGIONS_PER_CALL],
                output,
                len,
                Some((prefetch_ptr, BitplaneAddPrefetchKind::Input)),
            );
            region += TURBO_ADD_REGIONS_PER_CALL;
            prefetch_ptr = prefetch_ptr.wrapping_add(pf_len);
        }
    } else {
        while inputs.len().saturating_sub(region) >= TURBO_ADD_REGIONS_PER_CALL {
            xor_prepared_bitplane_multi_chunks_avx2_core(
                &inputs[region..region + TURBO_ADD_REGIONS_PER_CALL],
                output,
                len,
                None,
            );
            region += TURBO_ADD_REGIONS_PER_CALL;
        }
    }

    if region < inputs.len() {
        xor_prepared_bitplane_multi_chunks_avx2_core(&inputs[region..], output, len, None);
    }
    xor_jit_zeroupper();
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn xor_prepared_bitplane_multi_chunks_v1i6_core(
    inputs: [*const u8; 6],
    output: *mut u8,
    len: usize,
    prefetch: Option<(*const u8, BitplaneAddPrefetchKind)>,
) {
    use std::arch::x86_64::{__m256i, _mm256_load_si256, _mm256_store_si256, _mm256_xor_si256};

    let [input0, input1, input2, input3, input4, input5] = inputs;
    debug_assert_eq!(len % bitplane::AVX2_BLOCK_BYTES, 0);
    debug_assert_eq!(output as usize % 32, 0);
    for input in inputs {
        debug_assert_eq!(input as usize % 32, 0);
    }

    let mut pos = 0usize;
    while pos < len {
        for vec_idx in 0..16usize {
            let offset = pos + vec_idx * 32;
            let mut data = _mm256_load_si256(output.add(offset).cast::<__m256i>());
            data = _mm256_xor_si256(
                data,
                _mm256_load_si256(input0.add(offset).cast::<__m256i>()),
            );
            data = _mm256_xor_si256(
                data,
                _mm256_load_si256(input1.add(offset).cast::<__m256i>()),
            );
            data = _mm256_xor_si256(
                data,
                _mm256_load_si256(input2.add(offset).cast::<__m256i>()),
            );
            data = _mm256_xor_si256(
                data,
                _mm256_load_si256(input3.add(offset).cast::<__m256i>()),
            );
            data = _mm256_xor_si256(
                data,
                _mm256_load_si256(input4.add(offset).cast::<__m256i>()),
            );
            data = _mm256_xor_si256(
                data,
                _mm256_load_si256(input5.add(offset).cast::<__m256i>()),
            );
            _mm256_store_si256(output.add(offset).cast::<__m256i>(), data);
        }

        if let Some((prefetch_ptr, kind)) = prefetch {
            prefetch_prepared_bitplane_add(prefetch_ptr, pos, kind);
        }

        pos += bitplane::AVX2_BLOCK_BYTES;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn xor_prepared_bitplane_multi_chunks_v1i6_avx2(
    inputs: &[*const u8],
    output: *mut u8,
    len: usize,
    prefetch_in: Option<*const u8>,
    prefetch_out: Option<*const u8>,
) {
    const TURBO_ADD_REGIONS_PER_CALL: usize = 6;

    debug_assert_eq!(len % bitplane::AVX2_BLOCK_BYTES, 0);
    debug_assert_eq!(output as usize % 32, 0);
    for &input in inputs {
        debug_assert!(!input.is_null());
        debug_assert_eq!(input as usize % 32, 0);
    }

    let mut region_base = 0usize;
    while region_base + TURBO_ADD_REGIONS_PER_CALL <= inputs.len() {
        let chunk = [
            inputs[region_base],
            inputs[region_base + 1],
            inputs[region_base + 2],
            inputs[region_base + 3],
            inputs[region_base + 4],
            inputs[region_base + 5],
        ];
        let prefetch = if region_base == 0 {
            prefetch_out.map(|ptr| (ptr, BitplaneAddPrefetchKind::Output))
        } else {
            prefetch_in.map(|ptr| {
                (
                    ptr.wrapping_add((region_base / TURBO_ADD_REGIONS_PER_CALL) * len),
                    BitplaneAddPrefetchKind::Input,
                )
            })
        };
        xor_prepared_bitplane_multi_chunks_v1i6_core(chunk, output, len, prefetch);
        region_base += TURBO_ADD_REGIONS_PER_CALL;
    }

    if region_base < inputs.len() {
        let prefetch = if region_base == 0 {
            prefetch_out.map(|ptr| (ptr, BitplaneAddPrefetchKind::Output))
        } else {
            prefetch_in.map(|ptr| {
                (
                    ptr.wrapping_add((region_base / TURBO_ADD_REGIONS_PER_CALL) * len),
                    BitplaneAddPrefetchKind::Input,
                )
            })
        };
        xor_prepared_bitplane_multi_chunks_avx2_core(&inputs[region_base..], output, len, prefetch);
    }

    xor_jit_zeroupper();
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn xor_packed_multi_region_v16i1_core(
    src_end: *const u8,
    src_count: usize,
    output: *mut u8,
    len: usize,
    prefetch: Option<(*const u8, BitplaneAddPrefetchKind)>,
) {
    use std::arch::x86_64::{__m256i, _mm256_load_si256, _mm256_store_si256, _mm256_xor_si256};

    const TURBO_ADD_MAX_SRCS: usize = 18;

    debug_assert_eq!(len % bitplane::AVX2_BLOCK_BYTES, 0);
    debug_assert_eq!(output as usize % 32, 0);
    debug_assert!(src_count <= TURBO_ADD_MAX_SRCS);

    let output_end = output.add(len);
    let src0 = src_end;
    let src1 = src0.add(len);
    let src2 = src1.add(len);
    let src3 = src2.add(len);
    let src4 = src3.add(len);
    let src5 = src4.add(len);
    let src6 = src5.add(len);
    let src7 = src6.add(len);
    let src8 = src7.add(len);
    let src9 = src8.add(len);
    let src10 = src9.add(len);
    let src11 = src10.add(len);
    let src12 = src11.add(len);
    let src13 = src12.add(len);
    let src14 = src13.add(len);
    let src15 = src14.add(len);
    let src16 = src15.add(len);
    let src17 = src16.add(len);

    let mut ptr = -(len as isize);
    while ptr != 0 {
        let out_block = output_end.offset(ptr);
        for vec_idx in 0..16usize {
            let out_ptr = out_block.add(vec_idx * 32).cast::<__m256i>();
            let mut data = _mm256_load_si256(out_ptr);
            macro_rules! add_src {
                ($count:expr, $src:expr) => {
                    if src_count >= $count {
                        data = _mm256_xor_si256(
                            data,
                            _mm256_load_si256($src.offset(ptr).add(vec_idx * 32).cast::<__m256i>()),
                        );
                    }
                };
            }
            add_src!(1, src0);
            add_src!(2, src1);
            add_src!(3, src2);
            add_src!(4, src3);
            add_src!(5, src4);
            add_src!(6, src5);
            add_src!(7, src6);
            add_src!(8, src7);
            add_src!(9, src8);
            add_src!(10, src9);
            add_src!(11, src10);
            add_src!(12, src11);
            add_src!(13, src12);
            add_src!(14, src13);
            add_src!(15, src14);
            add_src!(16, src15);
            add_src!(17, src16);
            add_src!(18, src17);
            _mm256_store_si256(out_ptr, data);
        }

        if let Some((prefetch_ptr, kind)) = prefetch {
            prefetch_prepared_bitplane_add(prefetch_ptr, (len as isize + ptr) as usize, kind);
        }

        ptr += bitplane::AVX2_BLOCK_BYTES as isize;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn xor_packed_multi_region_v16i1_avx2(
    src: *const u8,
    regions: usize,
    output: *mut u8,
    len: usize,
    prefetch_in: Option<*const u8>,
    prefetch_out: Option<*const u8>,
) {
    const TURBO_ADD_INTERLEAVE: usize = 1;
    const TURBO_ADD_REGIONS_PER_CALL: usize = 6;
    const TURBO_ADD_PREFETCH_FACTOR: usize = 1;
    const TURBO_ADD_OUTPUT_PREFETCH_ROUNDS: usize = 1 << TURBO_ADD_PREFETCH_FACTOR;

    debug_assert_eq!(len % bitplane::AVX2_BLOCK_BYTES, 0);
    debug_assert_eq!(output as usize % 32, 0);

    let pf_len = len >> TURBO_ADD_PREFETCH_FACTOR;
    let mut region = 0usize;
    let mut output_pf_rounds = TURBO_ADD_OUTPUT_PREFETCH_ROUNDS;
    let mut prefetch_out_ptr = prefetch_out.map(|ptr| ptr.wrapping_add(pf_len));

    while output_pf_rounds > 0 && regions.saturating_sub(region) >= TURBO_ADD_REGIONS_PER_CALL {
        let src_end = src.add(region * len + len * TURBO_ADD_INTERLEAVE);
        xor_packed_multi_region_v16i1_core(
            src_end,
            TURBO_ADD_REGIONS_PER_CALL,
            output,
            len,
            prefetch_out_ptr.map(|ptr| (ptr, BitplaneAddPrefetchKind::Output)),
        );
        region += TURBO_ADD_REGIONS_PER_CALL;
        output_pf_rounds -= 1;
        prefetch_out_ptr = if output_pf_rounds > 0 {
            prefetch_out_ptr.map(|ptr| ptr.wrapping_add(pf_len))
        } else {
            None
        };
    }

    let remaining = regions.saturating_sub(region);
    if let Some(prefetch_ptr) = prefetch_out_ptr {
        if remaining >= TURBO_ADD_INTERLEAVE {
            let src_end = src.add(region * len + len * TURBO_ADD_INTERLEAVE);
            xor_packed_multi_region_v16i1_core(
                src_end,
                remaining,
                output,
                len,
                Some((prefetch_ptr, BitplaneAddPrefetchKind::Output)),
            );
            xor_jit_zeroupper();
            return;
        }
    }

    if let Some(prefetch_in_ptr) = prefetch_in {
        let mut prefetch_ptr = prefetch_in_ptr.wrapping_add(pf_len);
        while regions.saturating_sub(region) >= TURBO_ADD_REGIONS_PER_CALL {
            let src_end = src.add(region * len + len * TURBO_ADD_INTERLEAVE);
            xor_packed_multi_region_v16i1_core(
                src_end,
                TURBO_ADD_REGIONS_PER_CALL,
                output,
                len,
                Some((prefetch_ptr, BitplaneAddPrefetchKind::Input)),
            );
            region += TURBO_ADD_REGIONS_PER_CALL;
            prefetch_ptr = prefetch_ptr.wrapping_add(pf_len);
        }
    } else {
        while regions.saturating_sub(region) >= TURBO_ADD_REGIONS_PER_CALL {
            let src_end = src.add(region * len + len * TURBO_ADD_INTERLEAVE);
            xor_packed_multi_region_v16i1_core(
                src_end,
                TURBO_ADD_REGIONS_PER_CALL,
                output,
                len,
                None,
            );
            region += TURBO_ADD_REGIONS_PER_CALL;
        }
    }

    if region < regions {
        let src_end = src.add(region * len + len * TURBO_ADD_INTERLEAVE);
        xor_packed_multi_region_v16i1_core(src_end, regions - region, output, len, None);
    }

    xor_jit_zeroupper();
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
    use crate::reed_solomon::aligned::alloc_aligned_vec;
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
    fn add_rax_imm32_uses_accumulator_encoding() {
        let encoded = encoder::Program::new().finish_turbo_block_loop_prefix();
        assert_eq!(
            &encoded[..13],
            [0x48, 0x05, 0x00, 0x02, 0x00, 0x00, 0x48, 0x81, 0xc2, 0x00, 0x02, 0x00, 0x00]
        );
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
    fn xor_jit_identity_lane_kernel_matches_table_executor() {
        if !is_x86_feature_detected!("avx2") {
            return;
        }

        let input = (0..32).map(|value| value as u8).collect::<Vec<_>>();
        let mut expected = vec![0x33; 32];
        let mut actual = expected.clone();
        let tables = build_split_mul_table(Galois16::new(1));
        process_slice_multiply_add(&input, &mut expected, &tables);

        let kernel = XorJitLaneKernel::identity().expect("identity lane kernel");
        unsafe {
            kernel.run(input.as_ptr(), actual.as_mut_ptr());
        }

        assert_eq!(actual, expected);
    }

    #[test]
    fn bitplane_coeff_plan_matches_basis_multiplication() {
        for coefficient in [0, 1, 2, 3, 5, 0x100b, 0x678b, 0xffff] {
            let plan = BitplaneCoeffPlan::new(coefficient);

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
    fn avx2_bitplane_layout_uses_turbo_physical_bit_order() {
        for physical_bit in 0..16 {
            let word_bit = turbo_physical_word_bit(physical_bit);
            let half = if word_bit < 8 {
                bitplane::ByteHalf::Low
            } else {
                bitplane::ByteHalf::High
            };
            let bit_from_msb = 7 - (word_bit & 7);
            let offset = bitplane::mask_offset(half, bit_from_msb, 0);

            assert_eq!(offset, physical_bit * 32);
            assert_eq!(
                bitplane_vector_offset(word_bit),
                physical_bit as i32 * 32 - 128
            );
            assert_eq!(bitplane::mask_offset(half, bit_from_msb, 7), offset + 28);
        }
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

        for coefficient in [0, 1, 2, 3, 5, 0x100b, 0x678b, 0xffff] {
            let tables = build_split_mul_table(Galois16::new(coefficient));
            let mut expected = vec![0xa5; bitplane::AVX2_BLOCK_BYTES];
            let mut actual = expected.clone();

            process_slice_multiply_add(&input, &mut expected, &tables);
            bitplane::multiply_add_prepared_avx2_block(&prepared, coefficient, &mut actual);

            assert_eq!(actual, expected, "coefficient={coefficient:#06x}");
        }
    }

    #[test]
    fn xor_prepared_bitplane_multi_chunks_v1i6_matches_single_input_xors() {
        let len = bitplane::AVX2_BLOCK_BYTES * 2;

        for input_count in [6usize, 7, 12] {
            let prepared_inputs = (0..input_count)
                .map(|input_idx| {
                    let mut prepared = alloc_aligned_vec(len);
                    for (byte_idx, byte) in prepared.iter_mut().enumerate() {
                        *byte = (byte_idx as u8)
                            .wrapping_mul(17)
                            .wrapping_add((input_idx as u8).wrapping_mul(29))
                            .wrapping_add(3);
                    }
                    prepared
                })
                .collect::<Vec<_>>();
            let input_ptrs = prepared_inputs
                .iter()
                .map(|input| input.as_ptr())
                .collect::<Vec<_>>();
            let mut expected = alloc_aligned_vec(len);
            let mut actual = alloc_aligned_vec(len);

            for (byte_idx, byte) in expected.iter_mut().enumerate() {
                *byte = (byte_idx as u8).wrapping_mul(11).wrapping_add(5);
            }
            actual.copy_from_slice(&expected);

            for input in &prepared_inputs {
                xor_prepared_bitplane_chunks(input, &mut expected, None);
            }

            xor_prepared_bitplane_multi_chunks_v1i6(&input_ptrs, len, &mut actual, None, None);

            assert_eq!(actual, expected, "input_count={input_count}");
        }
    }

    #[test]
    fn xor_prepared_bitplane_multi_chunks_matches_single_input_xors() {
        let len = bitplane::AVX2_BLOCK_BYTES * 2;

        for input_count in [6usize, 7, 12] {
            let prepared_inputs = (0..input_count)
                .map(|input_idx| {
                    let mut prepared = alloc_aligned_vec(len);
                    for (byte_idx, byte) in prepared.iter_mut().enumerate() {
                        *byte = (byte_idx as u8)
                            .wrapping_mul(23)
                            .wrapping_add((input_idx as u8).wrapping_mul(31))
                            .wrapping_add(7);
                    }
                    prepared
                })
                .collect::<Vec<_>>();
            let input_ptrs = prepared_inputs
                .iter()
                .map(|input| input.as_ptr())
                .collect::<Vec<_>>();
            let mut expected = alloc_aligned_vec(len);
            let mut actual = alloc_aligned_vec(len);

            for (byte_idx, byte) in expected.iter_mut().enumerate() {
                *byte = (byte_idx as u8).wrapping_mul(13).wrapping_add(9);
            }
            actual.copy_from_slice(&expected);

            for input in &prepared_inputs {
                xor_prepared_bitplane_chunks(input, &mut expected, None);
            }

            xor_prepared_bitplane_multi_chunks(&input_ptrs, len, &mut actual, None, None);

            assert_eq!(actual, expected, "input_count={input_count}");
        }
    }

    #[test]
    fn xor_packed_multi_region_v16i1_matches_single_input_xors() {
        let len = bitplane::AVX2_BLOCK_BYTES * 2;

        for input_count in [6usize, 7, 12] {
            let mut packed_inputs = alloc_aligned_vec(len * input_count);
            for input_idx in 0..input_count {
                let start = input_idx * len;
                for (byte_idx, byte) in packed_inputs[start..start + len].iter_mut().enumerate() {
                    *byte = (byte_idx as u8)
                        .wrapping_mul(7)
                        .wrapping_add((input_idx as u8).wrapping_mul(17))
                        .wrapping_add(11);
                }
            }

            let mut expected = alloc_aligned_vec(len);
            let mut actual = alloc_aligned_vec(len);

            for (byte_idx, byte) in expected.iter_mut().enumerate() {
                *byte = (byte_idx as u8).wrapping_mul(5).wrapping_add(13);
            }
            actual.copy_from_slice(&expected);

            for input_idx in 0..input_count {
                let start = input_idx * len;
                xor_prepared_bitplane_chunks(
                    &packed_inputs[start..start + len],
                    &mut expected,
                    None,
                );
            }

            xor_packed_multi_region_v16i1(
                packed_inputs.as_ptr(),
                input_count,
                len,
                &mut actual,
                None,
                None,
            );

            assert_eq!(actual, expected, "input_count={input_count}");
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

        for coefficient in [0, 1, 2, 3, 5, 0x100b, 0x678b, 0xffff] {
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
        let mut prepared_input = alloc_aligned_vec(bitplane::AVX2_BLOCK_BYTES);
        let mut prepared_output = alloc_aligned_vec(bitplane::AVX2_BLOCK_BYTES);

        bitplane::prepare_avx2_block(prepared_input.as_mut_slice().try_into().unwrap(), &input);
        bitplane::prepare_avx2_block(
            prepared_output.as_mut_slice().try_into().unwrap(),
            &initial_output,
        );

        for coefficient in [0, 1, 2, 3, 5, 0x100b, 0x678b, 0xffff] {
            let mut expected = aligned_copy(&prepared_output);
            let mut actual = aligned_copy(&prepared_output);
            let kernel = XorJitGeneratedBitplaneKernel::new(coefficient).expect("bitplane kernel");

            bitplane::multiply_add_prepared_avx2_block_to_prepared(
                prepared_input.as_slice().try_into().unwrap(),
                coefficient,
                expected.as_mut_slice().try_into().unwrap(),
            );
            unsafe {
                kernel.multiply_add(
                    prepared_input.as_ptr(),
                    actual.as_mut_ptr(),
                    bitplane::AVX2_BLOCK_BYTES,
                );
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
        let mut prepared_input = alloc_aligned_vec(input.len());
        let mut expected = vec![0u8; input.len()];
        let mut actual = alloc_aligned_vec(input.len());
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

        let kernel = XorJitGeneratedBitplaneKernel::new(coefficient).expect("bitplane kernel");
        kernel.multiply_add_chunks(&prepared_input, &mut actual);

        assert_eq!(actual, expected);
    }

    #[test]
    fn generated_bitplane_prefetch_kernel_processes_prepared_chunks() {
        if !is_x86_feature_detected!("avx2") {
            return;
        }

        let input = (0..bitplane::AVX2_BLOCK_BYTES * 3)
            .map(|idx| (idx * 37 + 11) as u8)
            .collect::<Vec<_>>();
        let initial_output = (0..bitplane::AVX2_BLOCK_BYTES * 3)
            .map(|idx| (idx * 17 + 3) as u8)
            .collect::<Vec<_>>();
        let mut prepared_input = alloc_aligned_vec(input.len());
        let mut expected = vec![0u8; input.len()];
        let mut actual = alloc_aligned_vec(input.len());
        let prefetch = vec![0xccu8; input.len()];
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

        let kernel = XorJitGeneratedBitplaneKernel::new(coefficient).expect("bitplane kernel");
        kernel.multiply_add_chunks_with_prefetch(
            &prepared_input,
            &mut actual,
            Some(prefetch.as_ptr()),
        );

        assert_eq!(actual, expected);
    }

    #[test]
    fn direct_bitplane_dynamic_encoder_matches_program_encoder() {
        for coefficient in [1, 2, 3, 5, 0x100b, 0x678b, 0xc814, 0xffff] {
            let plan = BitplaneCoeffPlan::new(coefficient);
            for prefetch in [false, true] {
                let expected = emit_bitplane_chunk_program_bytes(&plan, prefetch);
                let static_prefix = xor_jit_body_static_prefix();
                let mut code =
                    exec_mem::MutableExecutableBuffer::new(XOR_JIT_TURBO_JIT_SIZE).expect("code");
                code.overwrite_at(0, static_prefix).expect("prefix");
                code.set_len_for_overwrite(static_prefix.len())
                    .expect("cursor");

                let dynamic_len = emit_bitplane_chunk_program_dynamic_into(
                    &plan,
                    prefetch,
                    static_prefix.len(),
                    &mut code,
                );
                let actual = code
                    .copy_prefix(static_prefix.len() + dynamic_len)
                    .expect("copy generated code");

                assert_eq!(
                    actual, expected,
                    "coefficient={coefficient:#06x} prefetch={prefetch}"
                );
            }
        }
    }

    #[test]
    #[ignore = "debug helper for dumping one generated body for byte comparison"]
    fn dump_selected_bitplane_program_for_byte_compare() {
        if !is_x86_feature_detected!("avx2") {
            return;
        }

        let coefficient = std::env::var("PAR2RS_XOR_JIT_COMPARE_COEFF")
            .ok()
            .and_then(|value| {
                value
                    .strip_prefix("0x")
                    .map(|hex| u16::from_str_radix(hex, 16).ok())
                    .unwrap_or_else(|| value.parse::<u16>().ok())
            })
            .unwrap_or(0xc814);
        let prefetch = std::env::var("PAR2RS_XOR_JIT_COMPARE_PREFETCH")
            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);
        let plan = BitplaneCoeffPlan::new(coefficient);
        let generated = emit_bitplane_chunk_program_bytes(&plan, prefetch);

        if prefetch {
            let _ = compile_bitplane_chunk_prefetch_program(&plan, "par2rs-xor-jit-compare")
                .expect("compile compare prefetch body");
        } else {
            let _ = compile_bitplane_chunk_program(&plan, "par2rs-xor-jit-compare", false)
                .expect("compile compare body");
        }

        eprintln!(
            "dumped par2rs xor-jit compare body coeff={coefficient:#06x} prefetch={prefetch} len={}",
            generated.len()
        );
    }

    #[test]
    fn biased_prefetch_pointer_matches_turbo_stub_bias() {
        let ptr = 1024usize as *const u8;
        assert_eq!(
            xor_jit_biased_prefetch_ptr(ptr) as usize,
            1024 - XOR_JIT_PREFETCH_STUB_BIAS_BYTES
        );
    }

    #[test]
    fn scratch_zero_coefficient_leaves_output_unloaded_and_unchanged() {
        if !is_x86_feature_detected!("avx2") {
            return;
        }

        let input = alloc_aligned_vec(bitplane::AVX2_BLOCK_BYTES);
        let mut output = alloc_aligned_vec(bitplane::AVX2_BLOCK_BYTES);
        output
            .iter_mut()
            .enumerate()
            .for_each(|(idx, byte)| *byte = (idx * 17 + 3) as u8);
        let expected = output.clone();
        let prefetch = vec![0xccu8; input.len()];
        let prepared = XorJitPreparedCoeff::new(0);
        let mut scratch = XorJitBitplaneScratch::new().expect("scratch");

        scratch.multiply_add_chunks_with_prefetch(&prepared, &input, &mut output, None);
        scratch.multiply_add_chunks_with_prefetch(
            &prepared,
            &input,
            &mut output,
            Some(prefetch.as_ptr()),
        );

        assert_eq!(output, expected);
        assert_eq!(scratch.loaded, None);
    }

    #[test]
    fn scratch_reuses_loaded_code_for_repeated_coefficient_and_mode() {
        if !is_x86_feature_detected!("avx2") {
            return;
        }

        let input = alloc_aligned_vec(bitplane::AVX2_BLOCK_BYTES);
        let mut output = alloc_aligned_vec(bitplane::AVX2_BLOCK_BYTES);
        let prefetch = vec![0xccu8; input.len()];
        let prepared = XorJitPreparedCoeff::new(0x100b);
        let mut scratch = XorJitBitplaneScratch::new().expect("scratch");

        scratch.multiply_add_chunks_with_prefetch(&prepared, &input, &mut output, None);
        let loaded_body = scratch.loaded;
        scratch.multiply_add_chunks_with_prefetch(&prepared, &input, &mut output, None);
        assert_eq!(scratch.loaded, loaded_body);

        scratch.multiply_add_chunks_with_prefetch(
            &prepared,
            &input,
            &mut output,
            Some(prefetch.as_ptr()),
        );
        let loaded_prefetch = scratch.loaded;
        scratch.multiply_add_chunks_with_prefetch(
            &prepared,
            &input,
            &mut output,
            Some(prefetch.as_ptr()),
        );
        assert_eq!(scratch.loaded, loaded_prefetch);
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
        let mut prepared_input = alloc_aligned_vec(input.len());
        let mut expected = vec![0u8; input.len()];
        let mut actual = alloc_aligned_vec(input.len());
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
    fn prepare_xor_jit_bitplane_segment_matches_full_prepare_for_aligned_segment() {
        let input = (0..bitplane::AVX2_BLOCK_BYTES * 2)
            .map(|idx| (idx * 11 + 3) as u8)
            .collect::<Vec<_>>();
        let mut expected = vec![0u8; input.len()];
        let mut actual = vec![0x55u8; input.len()];

        prepare_xor_jit_bitplane_chunks(&mut expected, &input);
        prepare_xor_jit_bitplane_segment(&mut actual, &input);

        assert_eq!(actual, expected);
    }

    #[test]
    fn prepare_xor_jit_bitplane_segment_zero_pads_tail() {
        let input = (0..bitplane::AVX2_BLOCK_BYTES / 2)
            .map(|idx| (idx * 7 + 1) as u8)
            .collect::<Vec<_>>();
        let mut prepared = vec![0x55u8; bitplane::AVX2_BLOCK_BYTES * 2];

        prepare_xor_jit_bitplane_segment(&mut prepared, &input);

        assert!(prepared[bitplane::AVX2_BLOCK_BYTES..]
            .iter()
            .all(|&byte| byte == 0));
    }

    #[test]
    fn finish_segment_roundtrips_partial_chunk() {
        let input = (0..bitplane::AVX2_BLOCK_BYTES + 37)
            .map(|idx| (idx * 5 + 9) as u8)
            .collect::<Vec<_>>();
        let mut prepared = vec![0u8; bitplane::AVX2_BLOCK_BYTES * 2];
        let mut actual = vec![0u8; input.len()];

        prepare_xor_jit_bitplane_segment(&mut prepared, &input);
        finish_xor_jit_bitplane_chunks(&mut actual, &prepared);

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

    fn aligned_copy(src: &[u8]) -> Vec<u8> {
        let mut dst = alloc_aligned_vec(src.len());
        dst.copy_from_slice(src);
        dst
    }

    #[test]
    fn xor_jit_avx2_matches_table_executor() {
        if !is_x86_feature_detected!("avx2") || !is_x86_feature_detected!("vpclmulqdq") {
            return;
        }

        for coeff in [1, 2, 7, 0x100b, 0xbeef] {
            let input = (0..257).map(|idx| (idx * 31 + 7) as u8).collect::<Vec<_>>();
            let mut expected = (0..257).map(|idx| (idx * 17 + 3) as u8).collect::<Vec<_>>();
            let mut actual = expected.clone();
            let table = build_split_mul_table(Galois16::new(coeff));
            process_slice_multiply_add(&input, &mut expected, &table);
            let prepared = XorJitPreparedCoeff::new(coeff);
            unsafe {
                process_slice_multiply_add_xor_jit(
                    &input,
                    &mut actual,
                    &prepared,
                    XorJitFlavor::Jit,
                );
            }
            assert_eq!(actual, expected, "coeff={coeff:#06x}");
        }
    }
}
