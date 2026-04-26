#![allow(dead_code)]

#[derive(Debug, Default, Clone)]
pub struct Program {
    instructions: Vec<Instruction>,
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

    pub fn vzeroupper(mut self) -> Self {
        self.instructions.push(Instruction::Vzeroupper);
        self
    }

    pub fn finish(self) -> Vec<u8> {
        let len = self.instructions.iter().map(Instruction::encoded_len).sum();
        let mut encoded = Vec::with_capacity(len);
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

    fn finish_block_loop_inner(
        self,
        block_bytes: u32,
        prefetch_step: Option<u32>,
        pointer_bias: u32,
    ) -> Vec<u8> {
        let body_len: usize = self.instructions.iter().map(Instruction::encoded_len).sum();
        if body_len == 0 {
            return Self::new().vzeroupper().ret().finish();
        }

        let pointer_bias_header = (pointer_bias != 0).then(|| {
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
        });
        let pointer_bias_len: usize = pointer_bias_header
            .as_ref()
            .map(|instructions| instructions.iter().map(Instruction::encoded_len).sum())
            .unwrap_or(0);
        let prefetch_header = prefetch_step.map(|step| {
            [
                Instruction::AddRegImm32 {
                    reg: GpReg::Rcx,
                    value: step,
                },
                Instruction::PrefetchT1 {
                    memory: Memory::base_offset(BaseReg::Rcx, -128),
                },
                Instruction::PrefetchT1 {
                    memory: Memory::base_offset(BaseReg::Rcx, -64),
                },
                Instruction::PrefetchT1 {
                    memory: Memory::base(BaseReg::Rcx),
                },
                Instruction::PrefetchT1 {
                    memory: Memory::base_offset(BaseReg::Rcx, 64),
                },
            ]
        });
        let prefetch_len: usize = prefetch_header
            .as_ref()
            .map(|instructions| instructions.iter().map(Instruction::encoded_len).sum())
            .unwrap_or(0);
        let loop_footer = [
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
        ];
        let footer_len: usize = loop_footer.iter().map(Instruction::encoded_len).sum();
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Instruction {
    MovEaxImm32(u32),
    AddRegImm32 { reg: GpReg, value: u32 },
    SubRegImm32 { reg: GpReg, value: u32 },
    PrefetchT1 { memory: Memory },
    VmovdqaLoad { dst: Ymm, memory: Memory },
    VmovdqaStore { memory: Memory, src: Ymm },
    VmovdquLoad { dst: Ymm, memory: Memory },
    VmovdquStore { memory: Memory, src: Ymm },
    Vpxor { dst: Ymm, lhs: Ymm, rhs: Ymm },
    VpxorMemory { dst: Ymm, lhs: Ymm, memory: Memory },
    JneRel32(i32),
    Vzeroupper,
    Ret,
}

impl Instruction {
    fn encoded_len(&self) -> usize {
        match self {
            Self::MovEaxImm32(_) => 5,
            Self::AddRegImm32 { .. } | Self::SubRegImm32 { .. } => 7,
            Self::PrefetchT1 { memory } => 2 + memory.encoded_len(),
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
            Self::JneRel32(_) => 6,
            Self::Vzeroupper => 3,
            Self::Ret => 1,
        }
    }

    fn encode_into(self, encoded: &mut Vec<u8>) {
        match self {
            Self::MovEaxImm32(value) => {
                encoded.push(0xb8);
                encoded.extend_from_slice(&value.to_le_bytes());
            }
            Self::AddRegImm32 { reg, value } => encode_reg_imm32(encoded, 0x81, 0, reg, value),
            Self::SubRegImm32 { reg, value } => encode_reg_imm32(encoded, 0x81, 5, reg, value),
            Self::PrefetchT1 { memory } => {
                encoded.extend_from_slice(&[0x0f, 0x18]);
                memory.encode_into(encoded, 2);
            }
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
    Rcx,
    Rdi,
    Rsi,
}

impl BaseReg {
    const fn code(self) -> u8 {
        match self {
            Self::Rcx => 1,
            Self::Rsi => 6,
            Self::Rdi => 7,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GpReg {
    Rcx,
    Rdx,
    Rsi,
    Rdi,
}

impl GpReg {
    const fn code(self) -> u8 {
        match self {
            Self::Rcx => 1,
            Self::Rdx => 2,
            Self::Rsi => 6,
            Self::Rdi => 7,
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

    fn encode_into(self, encoded: &mut Vec<u8>, reg: u8) {
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

fn encode_vex_256_66_0f(encoded: &mut Vec<u8>, reg: Ymm, vvvv: Ymm, rm: Option<u8>) {
    encode_vex_256_0f(encoded, reg, vvvv, rm, 0x01);
}

fn encode_vex_256_f3_0f(encoded: &mut Vec<u8>, reg: Ymm, vvvv: Ymm, rm: Option<u8>) {
    encode_vex_256_0f(encoded, reg, vvvv, rm, 0x02);
}

fn encode_vex_256_0f(encoded: &mut Vec<u8>, reg: Ymm, vvvv: Ymm, rm: Option<u8>, pp: u8) {
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

fn encode_reg_imm32(encoded: &mut Vec<u8>, opcode: u8, extension: u8, reg: GpReg, value: u32) {
    encoded.push(0x48);
    encoded.push(opcode);
    encoded.push(modrm_register(extension, reg.code()));
    encoded.extend_from_slice(&value.to_le_bytes());
}
