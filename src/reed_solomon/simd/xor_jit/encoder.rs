#![allow(dead_code)]

#[derive(Debug, Default, Clone)]
pub struct Program {
    instructions: Vec<Instruction>,
}

use super::exec_mem;

pub(super) trait ByteSink {
    fn push(&mut self, byte: u8);
    fn extend_from_slice(&mut self, bytes: &[u8]);
    fn len(&self) -> usize;
}

impl ByteSink for Vec<u8> {
    fn push(&mut self, byte: u8) {
        Vec::push(self, byte);
    }

    fn extend_from_slice(&mut self, bytes: &[u8]) {
        Vec::extend_from_slice(self, bytes);
    }

    fn len(&self) -> usize {
        Vec::len(self)
    }
}

impl ByteSink for exec_mem::MutableExecutableBuffer {
    fn push(&mut self, byte: u8) {
        self.append_byte(byte)
            .expect("append byte to mutable executable buffer");
    }

    fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.append_bytes(bytes)
            .expect("append bytes to mutable executable buffer");
    }

    fn len(&self) -> usize {
        exec_mem::MutableExecutableBuffer::len(self)
    }
}

pub(super) struct ProgramSink<'a, S: ByteSink> {
    encoded: &'a mut S,
}

impl<'a, S: ByteSink> ProgramSink<'a, S> {
    fn new(encoded: &'a mut S) -> Self {
        Self { encoded }
    }

    pub fn emit_bytes(self, bytes: &[u8]) -> Self {
        self.encoded.extend_from_slice(bytes);
        self
    }

    pub fn vmovdqa_ymm_from_rdi_offset(self, reg: u8, offset: i32) -> Self {
        encode_vmovdqa_load_into(
            self.encoded,
            Ymm::new(reg),
            Memory::base_offset(BaseReg::Rdi, offset),
        );
        self
    }

    pub fn vmovdqa_ymm_from_rax_offset(self, reg: u8, offset: i32) -> Self {
        encode_vmovdqa_load_into(
            self.encoded,
            Ymm::new(reg),
            Memory::base_offset(BaseReg::Rax, offset),
        );
        self
    }

    pub fn vmovdqa_ymm_from_rdx_offset(self, reg: u8, offset: i32) -> Self {
        encode_vmovdqa_load_into(
            self.encoded,
            Ymm::new(reg),
            Memory::base_offset(BaseReg::Rdx, offset),
        );
        self
    }

    pub fn vmovdqa_ymm_from_rsi_offset(self, reg: u8, offset: i32) -> Self {
        encode_vmovdqa_load_into(
            self.encoded,
            Ymm::new(reg),
            Memory::base_offset(BaseReg::Rsi, offset),
        );
        self
    }

    pub fn vpxor_ymm(self, dst: u8, lhs: u8, rhs: u8) -> Self {
        encode_vpxor_into(self.encoded, Ymm::new(dst), Ymm::new(lhs), Ymm::new(rhs));
        self
    }

    pub fn vmovdqa_ymm(self, dst: u8, src: u8) -> Self {
        encode_vmovdqa_reg_into(self.encoded, Ymm::new(dst), Ymm::new(src));
        self
    }

    pub fn vpxor_ymm_rdi_offset(self, dst: u8, lhs: u8, offset: i32) -> Self {
        encode_vpxor_memory_into(
            self.encoded,
            Ymm::new(dst),
            Ymm::new(lhs),
            Memory::base_offset(BaseReg::Rdi, offset),
        );
        self
    }

    pub fn vpxor_ymm_rsi_offset(self, dst: u8, lhs: u8, offset: i32) -> Self {
        encode_vpxor_memory_into(
            self.encoded,
            Ymm::new(dst),
            Ymm::new(lhs),
            Memory::base_offset(BaseReg::Rsi, offset),
        );
        self
    }

    pub fn vpxor_ymm_rax_offset(self, dst: u8, lhs: u8, offset: i32) -> Self {
        encode_vpxor_memory_into(
            self.encoded,
            Ymm::new(dst),
            Ymm::new(lhs),
            Memory::base_offset(BaseReg::Rax, offset),
        );
        self
    }

    pub fn vpxor_ymm_rdx_offset(self, dst: u8, lhs: u8, offset: i32) -> Self {
        encode_vpxor_memory_into(
            self.encoded,
            Ymm::new(dst),
            Ymm::new(lhs),
            Memory::base_offset(BaseReg::Rdx, offset),
        );
        self
    }

    pub fn vmovdqa_rsi_offset_from_ymm(self, offset: i32, reg: u8) -> Self {
        encode_vmovdqa_store_into(
            self.encoded,
            Memory::base_offset(BaseReg::Rsi, offset),
            Ymm::new(reg),
        );
        self
    }

    pub fn vmovdqa_rdx_offset_from_ymm(self, offset: i32, reg: u8) -> Self {
        encode_vmovdqa_store_into(
            self.encoded,
            Memory::base_offset(BaseReg::Rdx, offset),
            Ymm::new(reg),
        );
        self
    }
}

impl Program {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mov_eax_imm32(mut self, value: u32) -> Self {
        self.instructions.push(Instruction::MovEaxImm32(value));
        self
    }

    pub fn ret(mut self) -> Self {
        self.instructions.push(Instruction::Ret);
        self
    }

    pub fn vmovdqu_ymm0_from_rdi(mut self) -> Self {
        self.push_vmovdqu_load(Ymm::Ymm0, Memory::base(BaseReg::Rdi));
        self
    }

    pub fn vmovdqu_ymm0_from_rdi_offset(mut self, offset: i32) -> Self {
        self.push_vmovdqu_load(Ymm::Ymm0, Memory::base_offset(BaseReg::Rdi, offset));
        self
    }

    pub fn vmovdqu_ymm0_from_rsi_offset(mut self, offset: i32) -> Self {
        self.push_vmovdqu_load(Ymm::Ymm0, Memory::base_offset(BaseReg::Rsi, offset));
        self
    }

    pub fn vmovdqu_ymm1_from_rdi_offset(mut self, offset: i32) -> Self {
        self.push_vmovdqu_load(Ymm::Ymm1, Memory::base_offset(BaseReg::Rdi, offset));
        self
    }

    pub fn vmovdqu_ymm1_from_rsi(mut self) -> Self {
        self.push_vmovdqu_load(Ymm::Ymm1, Memory::base(BaseReg::Rsi));
        self
    }

    pub fn vmovdqu_ymm1_from_rsi_offset(mut self, offset: i32) -> Self {
        self.push_vmovdqu_load(Ymm::Ymm1, Memory::base_offset(BaseReg::Rsi, offset));
        self
    }

    pub fn vmovdqu_ymm2_from_rdi_offset(mut self, offset: i32) -> Self {
        self.push_vmovdqu_load(Ymm::Ymm2, Memory::base_offset(BaseReg::Rdi, offset));
        self
    }

    pub fn vmovdqu_ymm_from_rdi_offset(mut self, reg: u8, offset: i32) -> Self {
        self.push_vmovdqu_load(Ymm::new(reg), Memory::base_offset(BaseReg::Rdi, offset));
        self
    }

    pub fn vmovdqu_ymm_from_rsi_offset(mut self, reg: u8, offset: i32) -> Self {
        self.push_vmovdqu_load(Ymm::new(reg), Memory::base_offset(BaseReg::Rsi, offset));
        self
    }

    pub fn vmovdqa_ymm_from_rdi_offset(mut self, reg: u8, offset: i32) -> Self {
        self.push_vmovdqa_load(Ymm::new(reg), Memory::base_offset(BaseReg::Rdi, offset));
        self
    }

    pub fn vmovdqa_ymm_from_rsi_offset(mut self, reg: u8, offset: i32) -> Self {
        self.push_vmovdqa_load(Ymm::new(reg), Memory::base_offset(BaseReg::Rsi, offset));
        self
    }

    pub fn vmovdqa_ymm_from_rax_offset(mut self, reg: u8, offset: i32) -> Self {
        self.push_vmovdqa_load(Ymm::new(reg), Memory::base_offset(BaseReg::Rax, offset));
        self
    }

    pub fn vmovdqa_ymm_from_rdx_offset(mut self, reg: u8, offset: i32) -> Self {
        self.push_vmovdqa_load(Ymm::new(reg), Memory::base_offset(BaseReg::Rdx, offset));
        self
    }

    pub fn vpxor_ymm0_ymm0_ymm1(mut self) -> Self {
        self.push_vpxor(Ymm::Ymm0, Ymm::Ymm0, Ymm::Ymm1);
        self
    }

    pub fn vpxor_ymm0_ymm0_ymm2(mut self) -> Self {
        self.push_vpxor(Ymm::Ymm0, Ymm::Ymm0, Ymm::Ymm2);
        self
    }

    pub fn vpxor_ymm1_ymm1_ymm2(mut self) -> Self {
        self.push_vpxor(Ymm::Ymm1, Ymm::Ymm1, Ymm::Ymm2);
        self
    }

    pub fn vpxor_ymm(mut self, dst: u8, lhs: u8, rhs: u8) -> Self {
        self.push_vpxor(Ymm::new(dst), Ymm::new(lhs), Ymm::new(rhs));
        self
    }

    pub fn vmovdqa_ymm(mut self, dst: u8, src: u8) -> Self {
        self.instructions.push(Instruction::Vmovdqa {
            dst: Ymm::new(dst),
            src: Ymm::new(src),
        });
        self
    }

    pub fn vpxor_ymm_rdi_offset(mut self, dst: u8, lhs: u8, offset: i32) -> Self {
        self.instructions.push(Instruction::VpxorMemory {
            dst: Ymm::new(dst),
            lhs: Ymm::new(lhs),
            memory: Memory::base_offset(BaseReg::Rdi, offset),
        });
        self
    }

    pub fn vpxor_ymm_rsi_offset(mut self, dst: u8, lhs: u8, offset: i32) -> Self {
        self.instructions.push(Instruction::VpxorMemory {
            dst: Ymm::new(dst),
            lhs: Ymm::new(lhs),
            memory: Memory::base_offset(BaseReg::Rsi, offset),
        });
        self
    }

    pub fn vpxor_ymm_rax_offset(mut self, dst: u8, lhs: u8, offset: i32) -> Self {
        self.instructions.push(Instruction::VpxorMemory {
            dst: Ymm::new(dst),
            lhs: Ymm::new(lhs),
            memory: Memory::base_offset(BaseReg::Rax, offset),
        });
        self
    }

    pub fn vpxor_ymm_rdx_offset(mut self, dst: u8, lhs: u8, offset: i32) -> Self {
        self.instructions.push(Instruction::VpxorMemory {
            dst: Ymm::new(dst),
            lhs: Ymm::new(lhs),
            memory: Memory::base_offset(BaseReg::Rdx, offset),
        });
        self
    }

    pub fn vmovdqu_rsi_from_ymm0(mut self) -> Self {
        self.push_vmovdqu_store(Memory::base(BaseReg::Rsi), Ymm::Ymm0);
        self
    }

    pub fn vmovdqu_rsi_offset_from_ymm0(mut self, offset: i32) -> Self {
        self.push_vmovdqu_store(Memory::base_offset(BaseReg::Rsi, offset), Ymm::Ymm0);
        self
    }

    pub fn vmovdqu_rsi_offset_from_ymm1(mut self, offset: i32) -> Self {
        self.push_vmovdqu_store(Memory::base_offset(BaseReg::Rsi, offset), Ymm::Ymm1);
        self
    }

    pub fn vmovdqu_rsi_offset_from_ymm(mut self, offset: i32, reg: u8) -> Self {
        self.push_vmovdqu_store(Memory::base_offset(BaseReg::Rsi, offset), Ymm::new(reg));
        self
    }

    pub fn vmovdqa_rsi_offset_from_ymm(mut self, offset: i32, reg: u8) -> Self {
        self.push_vmovdqa_store(Memory::base_offset(BaseReg::Rsi, offset), Ymm::new(reg));
        self
    }

    pub fn vmovdqa_rdx_offset_from_ymm(mut self, offset: i32, reg: u8) -> Self {
        self.push_vmovdqa_store(Memory::base_offset(BaseReg::Rdx, offset), Ymm::new(reg));
        self
    }

    pub fn vzeroupper(mut self) -> Self {
        self.instructions.push(Instruction::Vzeroupper);
        self
    }

    pub fn finish(self) -> Vec<u8> {
        let len = encoded_len(&self.instructions);
        let mut encoded = Vec::with_capacity(len);
        self.instructions
            .into_iter()
            .for_each(|instruction| instruction.encode_into(&mut encoded));
        encoded
    }

    pub fn finish_block_loop_prefix(
        self,
        prefetch_step: Option<u32>,
        pointer_bias: u32,
    ) -> Vec<u8> {
        let pointer_bias_header = block_loop_pointer_bias_header(pointer_bias);
        let pointer_bias_len = pointer_bias_header.as_ref().map_or(0, |v| encoded_len(v));
        let prefetch_header = block_loop_prefetch_header(prefetch_step);
        let prefetch_len = prefetch_header.as_ref().map_or(0, |v| encoded_len(v));
        let body_len = encoded_len(&self.instructions);
        let total_len = pointer_bias_len + prefetch_len + body_len;
        let mut encoded = Vec::with_capacity(total_len);

        if let Some(pointer_bias_header) = pointer_bias_header {
            pointer_bias_header
                .into_iter()
                .for_each(|instruction| instruction.encode_into(&mut encoded));
        }
        if let Some(prefetch_header) = prefetch_header {
            prefetch_header
                .into_iter()
                .for_each(|instruction| instruction.encode_into(&mut encoded));
        }
        self.instructions
            .into_iter()
            .for_each(|instruction| instruction.encode_into(&mut encoded));
        debug_assert_eq!(encoded.len(), total_len);
        encoded
    }

    pub fn finish_turbo_block_loop_prefix(self) -> Vec<u8> {
        let pointer_header = [
            Instruction::AddRegImm32 {
                reg: GpReg::Rax,
                value: 512,
            },
            Instruction::AddRegImm32 {
                reg: GpReg::Rdx,
                value: 512,
            },
        ];
        let pointer_len = encoded_len(&pointer_header);
        let body_len = encoded_len(&self.instructions);
        let mut encoded = Vec::with_capacity(pointer_len + body_len);

        pointer_header
            .into_iter()
            .for_each(|instruction| instruction.encode_into(&mut encoded));
        self.instructions
            .into_iter()
            .for_each(|instruction| instruction.encode_into(&mut encoded));
        encoded
    }

    pub fn finish_block_loop(self, block_bytes: u32) -> Vec<u8> {
        self.finish_block_loop_inner(block_bytes, None, 0)
    }

    pub fn finish_block_loop_with_prefetch(self, block_bytes: u32, prefetch_step: u32) -> Vec<u8> {
        self.finish_block_loop_inner(block_bytes, Some(prefetch_step), 0)
    }

    pub fn finish_block_loop_with_pointer_bias(
        self,
        block_bytes: u32,
        pointer_bias: u32,
    ) -> Vec<u8> {
        self.finish_block_loop_inner(block_bytes, None, pointer_bias)
    }

    pub fn finish_block_loop_with_prefetch_and_pointer_bias(
        self,
        block_bytes: u32,
        prefetch_step: u32,
        pointer_bias: u32,
    ) -> Vec<u8> {
        self.finish_block_loop_inner(block_bytes, Some(prefetch_step), pointer_bias)
    }

    pub fn finish_block_loop_with_static_prefix(
        self,
        static_prefix: &[u8],
        block_bytes: u32,
        prefetch_step: Option<u32>,
    ) -> Vec<u8> {
        self.finish_block_loop_with_static_prefix_inner(
            static_prefix,
            block_bytes,
            prefetch_step,
            true,
        )
    }

    pub fn finish_block_loop_with_static_prefix_no_vzeroupper(
        self,
        static_prefix: &[u8],
        block_bytes: u32,
        prefetch_step: Option<u32>,
    ) -> Vec<u8> {
        self.finish_block_loop_with_static_prefix_inner(
            static_prefix,
            block_bytes,
            prefetch_step,
            false,
        )
    }

    pub fn finish_block_loop_dynamic_after_static_prefix_no_vzeroupper_into(
        self,
        static_prefix_len: usize,
        block_bytes: u32,
        prefetch_step: Option<u32>,
        encoded: &mut impl ByteSink,
    ) -> usize {
        self.finish_block_loop_dynamic_after_static_prefix_into(
            static_prefix_len,
            block_bytes,
            prefetch_step,
            false,
            encoded,
        )
    }

    fn finish_block_loop_with_static_prefix_inner(
        self,
        static_prefix: &[u8],
        _block_bytes: u32,
        prefetch_step: Option<u32>,
        vzeroupper: bool,
    ) -> Vec<u8> {
        let body_len = encoded_len(&self.instructions);
        if static_prefix.is_empty() && body_len == 0 {
            return if vzeroupper {
                Self::new().vzeroupper().ret().finish()
            } else {
                Self::new().ret().finish()
            };
        }

        let prefetch_header = block_loop_prefetch_header_for(prefetch_step, BaseReg::Rsi);
        let prefetch_len = prefetch_header.as_ref().map_or(0, |v| encoded_len(v));
        let loop_footer = block_loop_turbo_cmp_footer();
        let footer_len = encoded_len(&loop_footer);
        let jump_len = Instruction::JlRel32(0).encoded_len();
        let back_edge =
            -((static_prefix.len() + prefetch_len + body_len + footer_len + jump_len) as i32);
        let total_len = static_prefix.len()
            + prefetch_len
            + body_len
            + footer_len
            + jump_len
            + if vzeroupper {
                Instruction::Vzeroupper.encoded_len()
            } else {
                0
            }
            + Instruction::Ret.encoded_len();
        let mut encoded = Vec::with_capacity(total_len);

        encoded.extend_from_slice(static_prefix);
        if let Some(prefetch_header) = prefetch_header {
            prefetch_header
                .into_iter()
                .for_each(|instruction| instruction.encode_into(&mut encoded));
        }
        self.instructions
            .into_iter()
            .for_each(|instruction| instruction.encode_into(&mut encoded));
        loop_footer
            .into_iter()
            .for_each(|instruction| instruction.encode_into(&mut encoded));
        Instruction::JlRel32(back_edge).encode_into(&mut encoded);
        if vzeroupper {
            Instruction::Vzeroupper.encode_into(&mut encoded);
        }
        Instruction::Ret.encode_into(&mut encoded);
        debug_assert_eq!(encoded.len(), total_len);
        encoded
    }

    fn finish_block_loop_dynamic_after_static_prefix_into(
        self,
        static_prefix_len: usize,
        _block_bytes: u32,
        prefetch_step: Option<u32>,
        vzeroupper: bool,
        encoded: &mut impl ByteSink,
    ) -> usize {
        let body_len = encoded_len(&self.instructions);
        let prefetch_header = block_loop_prefetch_header_for(prefetch_step, BaseReg::Rsi);
        let prefetch_len = prefetch_header.as_ref().map_or(0, |v| encoded_len(v));
        let loop_footer = block_loop_turbo_cmp_footer();
        let footer_len = encoded_len(&loop_footer);
        let jump_len = Instruction::JlRel32(0).encoded_len();
        let back_edge =
            -((static_prefix_len + prefetch_len + body_len + footer_len + jump_len) as i32);
        let dynamic_len = prefetch_len
            + body_len
            + footer_len
            + jump_len
            + if vzeroupper {
                Instruction::Vzeroupper.encoded_len()
            } else {
                0
            }
            + Instruction::Ret.encoded_len();
        let start_len = encoded.len();

        if let Some(prefetch_header) = prefetch_header {
            prefetch_header
                .into_iter()
                .for_each(|instruction| instruction.encode_into(encoded));
        }
        self.instructions
            .into_iter()
            .for_each(|instruction| instruction.encode_into(encoded));
        loop_footer
            .into_iter()
            .for_each(|instruction| instruction.encode_into(encoded));
        Instruction::JlRel32(back_edge).encode_into(encoded);
        if vzeroupper {
            Instruction::Vzeroupper.encode_into(encoded);
        }
        Instruction::Ret.encode_into(encoded);
        debug_assert_eq!(encoded.len(), start_len + dynamic_len);
        dynamic_len
    }

    fn finish_block_loop_inner(
        self,
        block_bytes: u32,
        prefetch_step: Option<u32>,
        pointer_bias: u32,
    ) -> Vec<u8> {
        let body_len = encoded_len(&self.instructions);
        if body_len == 0 {
            return Self::new().vzeroupper().ret().finish();
        }

        let pointer_bias_header = block_loop_pointer_bias_header(pointer_bias);
        let pointer_bias_len = pointer_bias_header.as_ref().map_or(0, |v| encoded_len(v));
        let prefetch_header = block_loop_prefetch_header(prefetch_step);
        let prefetch_len = prefetch_header.as_ref().map_or(0, |v| encoded_len(v));
        let loop_footer = block_loop_footer(block_bytes);
        let footer_len = encoded_len(&loop_footer);
        let jump_len = Instruction::JneRel32(0).encoded_len();
        let back_edge = -((prefetch_len + body_len + footer_len + jump_len) as i32);
        let total_len = pointer_bias_len
            + prefetch_len
            + body_len
            + footer_len
            + jump_len
            + Instruction::Vzeroupper.encoded_len()
            + Instruction::Ret.encoded_len();
        let mut encoded = Vec::with_capacity(total_len);

        if let Some(pointer_bias_header) = pointer_bias_header {
            pointer_bias_header
                .into_iter()
                .for_each(|instruction| instruction.encode_into(&mut encoded));
        }
        if let Some(prefetch_header) = prefetch_header {
            prefetch_header
                .into_iter()
                .for_each(|instruction| instruction.encode_into(&mut encoded));
        }
        self.instructions
            .into_iter()
            .for_each(|instruction| instruction.encode_into(&mut encoded));
        loop_footer
            .into_iter()
            .for_each(|instruction| instruction.encode_into(&mut encoded));
        Instruction::JneRel32(back_edge).encode_into(&mut encoded);
        Instruction::Vzeroupper.encode_into(&mut encoded);
        Instruction::Ret.encode_into(&mut encoded);
        debug_assert_eq!(encoded.len(), total_len);
        encoded
    }

    fn push_vmovdqu_load(&mut self, dst: Ymm, memory: Memory) {
        self.instructions
            .push(Instruction::VmovdquLoad { dst, memory });
    }

    fn push_vmovdqu_store(&mut self, memory: Memory, src: Ymm) {
        self.instructions
            .push(Instruction::VmovdquStore { memory, src });
    }

    fn push_vmovdqa_load(&mut self, dst: Ymm, memory: Memory) {
        self.instructions
            .push(Instruction::VmovdqaLoad { dst, memory });
    }

    fn push_vmovdqa_store(&mut self, memory: Memory, src: Ymm) {
        self.instructions
            .push(Instruction::VmovdqaStore { memory, src });
    }

    fn push_vpxor(&mut self, dst: Ymm, lhs: Ymm, rhs: Ymm) {
        self.instructions.push(Instruction::Vpxor { dst, lhs, rhs });
    }
}

pub(super) fn encode_block_loop_dynamic_after_static_prefix_no_vzeroupper_into<S, F>(
    static_prefix_len: usize,
    _block_bytes: u32,
    prefetch_step: Option<u32>,
    encoded: &mut S,
    build_body: F,
) -> usize
where
    S: ByteSink,
    F: for<'a> FnOnce(ProgramSink<'a, S>) -> ProgramSink<'a, S>,
{
    let start_len = encoded.len();
    let prefetch_header = block_loop_prefetch_header_for(prefetch_step, BaseReg::Rsi);

    if let Some(prefetch_header) = prefetch_header {
        prefetch_header
            .into_iter()
            .for_each(|instruction| instruction.encode_into(encoded));
    }

    let body_start_len = encoded.len();
    let body = ProgramSink::new(encoded);
    let _body = build_body(body);
    let body_end_len = _body.encoded.len();
    drop(_body);
    let body_len = body_end_len - body_start_len;

    let prefetch_len = body_start_len - start_len;
    let loop_footer = block_loop_turbo_cmp_footer();
    let footer_len = encoded_len(&loop_footer);
    let jump_len = Instruction::JlRel32(0).encoded_len();
    let back_edge = -((static_prefix_len + prefetch_len + body_len + footer_len + jump_len) as i32);

    loop_footer
        .into_iter()
        .for_each(|instruction| instruction.encode_into(encoded));
    Instruction::JlRel32(back_edge).encode_into(encoded);
    Instruction::Ret.encode_into(encoded);

    encoded.len() - start_len
}

fn block_loop_pointer_bias_header(pointer_bias: u32) -> Option<[Instruction; 2]> {
    (pointer_bias != 0).then(|| {
        [
            Instruction::AddRegImm32 {
                reg: GpReg::Rdi,
                value: pointer_bias,
            },
            Instruction::AddRegImm32 {
                reg: GpReg::Rsi,
                value: pointer_bias,
            },
        ]
    })
}

fn block_loop_prefetch_header(prefetch_step: Option<u32>) -> Option<[Instruction; 5]> {
    block_loop_prefetch_header_for(prefetch_step, BaseReg::Rcx)
}

fn block_loop_prefetch_header_for(
    prefetch_step: Option<u32>,
    reg: BaseReg,
) -> Option<[Instruction; 5]> {
    prefetch_step.map(|step| {
        [
            Instruction::AddRegImm32 {
                reg: GpReg::from_base(reg),
                value: step,
            },
            Instruction::PrefetchT1 {
                memory: Memory::base_offset(reg, -128),
            },
            Instruction::PrefetchT1 {
                memory: Memory::base_offset(reg, -64),
            },
            Instruction::PrefetchT1 {
                memory: Memory::base(reg),
            },
            Instruction::PrefetchT1 {
                memory: Memory::base_offset(reg, 64),
            },
        ]
    })
}

fn block_loop_footer(block_bytes: u32) -> [Instruction; 3] {
    [
        Instruction::AddRegImm32 {
            reg: GpReg::Rdi,
            value: block_bytes,
        },
        Instruction::AddRegImm32 {
            reg: GpReg::Rsi,
            value: block_bytes,
        },
        Instruction::SubRegImm32 {
            reg: GpReg::Rdx,
            value: block_bytes,
        },
    ]
}

fn block_loop_len_footer(block_bytes: u32) -> [Instruction; 1] {
    [Instruction::SubRegImm32 {
        reg: GpReg::Rdx,
        value: block_bytes,
    }]
}

fn block_loop_turbo_cmp_footer() -> [Instruction; 1] {
    [Instruction::CmpRegReg {
        lhs: GpReg::Rdx,
        rhs: GpReg::Rcx,
    }]
}

fn encoded_len(instructions: &[Instruction]) -> usize {
    instructions.iter().map(Instruction::encoded_len).sum()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Instruction {
    MovEaxImm32(u32),
    AddRegImm32 { reg: GpReg, value: u32 },
    SubRegImm32 { reg: GpReg, value: u32 },
    CmpRegReg { lhs: GpReg, rhs: GpReg },
    PrefetchT1 { memory: Memory },
    Vmovdqa { dst: Ymm, src: Ymm },
    VmovdqaLoad { dst: Ymm, memory: Memory },
    VmovdqaStore { memory: Memory, src: Ymm },
    VmovdquLoad { dst: Ymm, memory: Memory },
    VmovdquStore { memory: Memory, src: Ymm },
    Vpxor { dst: Ymm, lhs: Ymm, rhs: Ymm },
    VpxorMemory { dst: Ymm, lhs: Ymm, memory: Memory },
    JneRel32(i32),
    JlRel32(i32),
    Vzeroupper,
    Ret,
}

impl Instruction {
    fn encoded_len(&self) -> usize {
        match self {
            Self::MovEaxImm32(_) => 5,
            Self::AddRegImm32 { reg, .. } => {
                if *reg == GpReg::Rax {
                    6
                } else {
                    7
                }
            }
            Self::SubRegImm32 { .. } => 7,
            Self::CmpRegReg { .. } => 3,
            Self::PrefetchT1 { memory } => 2 + memory.encoded_len(),
            Self::Vmovdqa { src, .. } => {
                if src.needs_extension() {
                    5
                } else {
                    4
                }
            }
            Self::VmovdqaLoad { memory, .. } | Self::VmovdqaStore { memory, .. } => {
                3 + memory.encoded_len()
            }
            Self::VmovdquLoad { memory, .. } | Self::VmovdquStore { memory, .. } => {
                3 + memory.encoded_len()
            }
            Self::Vpxor { rhs, .. } => {
                if rhs.needs_extension() {
                    5
                } else {
                    4
                }
            }
            Self::VpxorMemory { memory, .. } => 3 + memory.encoded_len(),
            Self::JneRel32(_) | Self::JlRel32(_) => 6,
            Self::Vzeroupper => 3,
            Self::Ret => 1,
        }
    }

    fn encode_into(self, encoded: &mut impl ByteSink) {
        match self {
            Self::MovEaxImm32(value) => {
                encoded.push(0xb8);
                encoded.extend_from_slice(&value.to_le_bytes());
            }
            Self::AddRegImm32 { reg, value } => encode_add_reg_imm32(encoded, reg, value),
            Self::SubRegImm32 { reg, value } => encode_reg_imm32(encoded, 0x81, 5, reg, value),
            Self::CmpRegReg { lhs, rhs } => {
                encoded.push(0x48);
                encoded.push(0x39);
                encoded.push(modrm_register(rhs.code(), lhs.code()));
            }
            Self::PrefetchT1 { memory } => {
                encoded.extend_from_slice(&[0x0f, 0x18]);
                memory.encode_into(encoded, 2);
            }
            Self::Vmovdqa { dst, src } => encode_vmovdqa_reg_into(encoded, dst, src),
            Self::VmovdqaLoad { dst, memory } => {
                encode_vex_256_66_0f(encoded, dst, Ymm::Ymm0, memory.base_code());
                encoded.push(0x6f);
                memory.encode_into(encoded, dst.code());
            }
            Self::VmovdqaStore { memory, src } => {
                encode_vex_256_66_0f(encoded, src, Ymm::Ymm0, memory.base_code());
                encoded.push(0x7f);
                memory.encode_into(encoded, src.code());
            }
            Self::VmovdquLoad { dst, memory } => {
                encode_vex_256_f3_0f(encoded, dst, Ymm::Ymm0, memory.base_code());
                encoded.push(0x6f);
                memory.encode_into(encoded, dst.code());
            }
            Self::VmovdquStore { memory, src } => {
                encode_vex_256_f3_0f(encoded, src, Ymm::Ymm0, memory.base_code());
                encoded.push(0x7f);
                memory.encode_into(encoded, src.code());
            }
            Self::Vpxor { dst, lhs, rhs } => {
                encode_vex_256_66_0f(encoded, dst, lhs, Some(rhs.code()));
                encoded.push(0xef);
                encoded.push(modrm_register(dst.code(), rhs.code()));
            }
            Self::VpxorMemory { dst, lhs, memory } => {
                encode_vex_256_66_0f(encoded, dst, lhs, memory.base_code());
                encoded.push(0xef);
                memory.encode_into(encoded, dst.code());
            }
            Self::JneRel32(offset) => {
                encoded.extend_from_slice(&[0x0f, 0x85]);
                encoded.extend_from_slice(&offset.to_le_bytes());
            }
            Self::JlRel32(offset) => {
                encoded.extend_from_slice(&[0x0f, 0x8c]);
                encoded.extend_from_slice(&offset.to_le_bytes());
            }
            Self::Vzeroupper => encoded.extend_from_slice(&[0xc5, 0xf8, 0x77]),
            Self::Ret => encoded.push(0xc3),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ymm {
    Ymm0,
    Ymm1,
    Ymm2,
    Ymm3,
    Ymm4,
    Ymm5,
    Ymm6,
    Ymm7,
    Ymm8,
    Ymm9,
    Ymm10,
    Ymm11,
    Ymm12,
    Ymm13,
    Ymm14,
    Ymm15,
}

impl Ymm {
    const fn new(index: u8) -> Self {
        match index {
            0 => Self::Ymm0,
            1 => Self::Ymm1,
            2 => Self::Ymm2,
            3 => Self::Ymm3,
            4 => Self::Ymm4,
            5 => Self::Ymm5,
            6 => Self::Ymm6,
            7 => Self::Ymm7,
            8 => Self::Ymm8,
            9 => Self::Ymm9,
            10 => Self::Ymm10,
            11 => Self::Ymm11,
            12 => Self::Ymm12,
            13 => Self::Ymm13,
            14 => Self::Ymm14,
            15 => Self::Ymm15,
            _ => panic!("invalid YMM register"),
        }
    }

    const fn code(self) -> u8 {
        match self {
            Self::Ymm0 => 0,
            Self::Ymm1 => 1,
            Self::Ymm2 => 2,
            Self::Ymm3 => 3,
            Self::Ymm4 => 4,
            Self::Ymm5 => 5,
            Self::Ymm6 => 6,
            Self::Ymm7 => 7,
            Self::Ymm8 => 8,
            Self::Ymm9 => 9,
            Self::Ymm10 => 10,
            Self::Ymm11 => 11,
            Self::Ymm12 => 12,
            Self::Ymm13 => 13,
            Self::Ymm14 => 14,
            Self::Ymm15 => 15,
        }
    }

    const fn needs_extension(self) -> bool {
        self.code() > 7
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BaseReg {
    Rax,
    Rcx,
    Rdx,
    Rdi,
    Rsi,
}

impl BaseReg {
    const fn code(self) -> u8 {
        match self {
            Self::Rax => 0,
            Self::Rcx => 1,
            Self::Rdx => 2,
            Self::Rsi => 6,
            Self::Rdi => 7,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GpReg {
    Rax,
    Rcx,
    Rdx,
    Rsi,
    Rdi,
}

impl GpReg {
    const fn code(self) -> u8 {
        match self {
            Self::Rax => 0,
            Self::Rcx => 1,
            Self::Rdx => 2,
            Self::Rsi => 6,
            Self::Rdi => 7,
        }
    }

    const fn from_base(reg: BaseReg) -> Self {
        match reg {
            BaseReg::Rax => Self::Rax,
            BaseReg::Rcx => Self::Rcx,
            BaseReg::Rdx => Self::Rdx,
            BaseReg::Rsi => Self::Rsi,
            BaseReg::Rdi => Self::Rdi,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Memory {
    base: BaseReg,
    displacement: i32,
}

impl Memory {
    const fn base(base: BaseReg) -> Self {
        Self {
            base,
            displacement: 0,
        }
    }

    const fn base_offset(base: BaseReg, displacement: i32) -> Self {
        Self { base, displacement }
    }

    fn encoded_len(&self) -> usize {
        match displacement_size(self.displacement) {
            DisplacementSize::None => 1,
            DisplacementSize::Byte => 2,
            DisplacementSize::Dword => 5,
        }
    }

    fn encode_into(self, encoded: &mut impl ByteSink, reg: u8) {
        let rm = self.base.code();
        match displacement_size(self.displacement) {
            DisplacementSize::None => encoded.push(modrm_memory(0b00, reg, rm)),
            DisplacementSize::Byte => {
                encoded.push(modrm_memory(0b01, reg, rm));
                encoded.push(self.displacement as u8);
            }
            DisplacementSize::Dword => {
                encoded.push(modrm_memory(0b10, reg, rm));
                encoded.extend_from_slice(&self.displacement.to_le_bytes());
            }
        }
    }

    const fn base_code(self) -> Option<u8> {
        Some(self.base.code())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplacementSize {
    None,
    Byte,
    Dword,
}

fn displacement_size(displacement: i32) -> DisplacementSize {
    match displacement {
        0 => DisplacementSize::None,
        -128..=127 => DisplacementSize::Byte,
        _ => DisplacementSize::Dword,
    }
}

const fn modrm_memory(mode: u8, reg: u8, rm: u8) -> u8 {
    (mode << 6) | ((reg & 7) << 3) | (rm & 7)
}

const fn modrm_register(reg: u8, rm: u8) -> u8 {
    0xc0 | ((reg & 7) << 3) | (rm & 7)
}

fn encode_vex_256_66_0f(encoded: &mut impl ByteSink, reg: Ymm, vvvv: Ymm, rm: Option<u8>) {
    encode_vex_256_0f(encoded, reg, vvvv, rm, 0x01);
}

fn encode_vmovdqa_load_into(encoded: &mut impl ByteSink, dst: Ymm, memory: Memory) {
    encode_vex_256_66_0f(encoded, dst, Ymm::Ymm0, memory.base_code());
    encoded.push(0x6f);
    memory.encode_into(encoded, dst.code());
}

fn encode_vmovdqa_store_into(encoded: &mut impl ByteSink, memory: Memory, src: Ymm) {
    encode_vex_256_66_0f(encoded, src, Ymm::Ymm0, memory.base_code());
    encoded.push(0x7f);
    memory.encode_into(encoded, src.code());
}

fn encode_vmovdqa_reg_into(encoded: &mut impl ByteSink, dst: Ymm, src: Ymm) {
    encode_vex_256_66_0f(encoded, dst, Ymm::Ymm0, Some(src.code()));
    encoded.push(0x6f);
    encoded.push(modrm_register(dst.code(), src.code()));
}

fn encode_vpxor_into(encoded: &mut impl ByteSink, dst: Ymm, lhs: Ymm, rhs: Ymm) {
    encode_vex_256_66_0f(encoded, dst, lhs, Some(rhs.code()));
    encoded.push(0xef);
    encoded.push(modrm_register(dst.code(), rhs.code()));
}

fn encode_vpxor_memory_into(encoded: &mut impl ByteSink, dst: Ymm, lhs: Ymm, memory: Memory) {
    encode_vex_256_66_0f(encoded, dst, lhs, memory.base_code());
    encoded.push(0xef);
    memory.encode_into(encoded, dst.code());
}

fn encode_vex_256_f3_0f(encoded: &mut impl ByteSink, reg: Ymm, vvvv: Ymm, rm: Option<u8>) {
    encode_vex_256_0f(encoded, reg, vvvv, rm, 0x02);
}

fn encode_vex_256_0f(encoded: &mut impl ByteSink, reg: Ymm, vvvv: Ymm, rm: Option<u8>, pp: u8) {
    let reg_code = reg.code();
    let rm_code = rm.unwrap_or(0);
    let r = ((reg_code >> 3) & 1) == 0;
    let b = ((rm_code >> 3) & 1) == 0;

    if b {
        encoded.push(0xc5);
        encoded.push((u8::from(r) << 7) | ((!vvvv.code() & 0x0f) << 3) | 0x04 | pp);
    } else {
        encoded.push(0xc4);
        encoded.push((u8::from(r) << 7) | 0x40 | (u8::from(b) << 5) | 0x01);
        encoded.push(((!vvvv.code() & 0x0f) << 3) | 0x04 | pp);
    }
}

fn encode_reg_imm32(
    encoded: &mut impl ByteSink,
    opcode: u8,
    extension: u8,
    reg: GpReg,
    value: u32,
) {
    encoded.push(0x48);
    encoded.push(opcode);
    encoded.push(modrm_register(extension, reg.code()));
    encoded.extend_from_slice(&value.to_le_bytes());
}

fn encode_add_reg_imm32(encoded: &mut impl ByteSink, reg: GpReg, value: u32) {
    encoded.push(0x48);
    if reg == GpReg::Rax {
        encoded.push(0x05);
    } else {
        encoded.push(0x81);
        encoded.push(modrm_register(0, reg.code()));
    }
    encoded.extend_from_slice(&value.to_le_bytes());
}
