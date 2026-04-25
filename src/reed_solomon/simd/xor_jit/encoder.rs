#![allow(dead_code)]

#[derive(Debug, Default)]
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

    fn push_vmovdqu_load(&mut self, dst: Ymm, memory: Memory) {
        self.instructions
            .push(Instruction::VmovdquLoad { dst, memory });
    }

    fn push_vmovdqu_store(&mut self, memory: Memory, src: Ymm) {
        self.instructions
            .push(Instruction::VmovdquStore { memory, src });
    }

    fn push_vpxor(&mut self, dst: Ymm, lhs: Ymm, rhs: Ymm) {
        self.instructions.push(Instruction::Vpxor { dst, lhs, rhs });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Instruction {
    MovEaxImm32(u32),
    VmovdquLoad { dst: Ymm, memory: Memory },
    VmovdquStore { memory: Memory, src: Ymm },
    Vpxor { dst: Ymm, lhs: Ymm, rhs: Ymm },
    Vzeroupper,
    Ret,
}

impl Instruction {
    fn encoded_len(&self) -> usize {
        match self {
            Self::MovEaxImm32(_) => 5,
            Self::VmovdquLoad { memory, .. } | Self::VmovdquStore { memory, .. } => {
                3 + memory.encoded_len()
            }
            Self::Vpxor { .. } => 4,
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
            Self::VmovdquLoad { dst, memory } => {
                encoded.extend_from_slice(&[0xc5, 0xfe, 0x6f]);
                memory.encode_into(encoded, dst.code());
            }
            Self::VmovdquStore { memory, src } => {
                encoded.extend_from_slice(&[0xc5, 0xfe, 0x7f]);
                memory.encode_into(encoded, src.code());
            }
            Self::Vpxor { dst, lhs, rhs } => {
                debug_assert_eq!(dst, lhs);
                encoded.extend_from_slice(&[
                    0xc5,
                    vex_256_66_0f(lhs.code()),
                    0xef,
                    modrm_register(dst.code(), rhs.code()),
                ]);
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
}

impl Ymm {
    const fn code(self) -> u8 {
        match self {
            Self::Ymm0 => 0,
            Self::Ymm1 => 1,
            Self::Ymm2 => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BaseReg {
    Rdi,
    Rsi,
}

impl BaseReg {
    const fn code(self) -> u8 {
        match self {
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
    (mode << 6) | (reg << 3) | rm
}

const fn modrm_register(reg: u8, rm: u8) -> u8 {
    0xc0 | (reg << 3) | rm
}

const fn vex_256_66_0f(lhs: u8) -> u8 {
    0x80 | ((!lhs & 0x0f) << 3) | 0x05
}
