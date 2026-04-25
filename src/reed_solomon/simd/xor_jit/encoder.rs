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
        self.instructions.push(Instruction::VmovdquLoad {
            dst: Ymm::Ymm0,
            base: BaseReg::Rdi,
        });
        self
    }

    pub fn vmovdqu_ymm1_from_rsi(mut self) -> Self {
        self.instructions.push(Instruction::VmovdquLoad {
            dst: Ymm::Ymm1,
            base: BaseReg::Rsi,
        });
        self
    }

    pub fn vpxor_ymm0_ymm0_ymm1(mut self) -> Self {
        self.instructions.push(Instruction::Vpxor {
            dst: Ymm::Ymm0,
            lhs: Ymm::Ymm0,
            rhs: Ymm::Ymm1,
        });
        self
    }

    pub fn vmovdqu_rsi_from_ymm0(mut self) -> Self {
        self.instructions.push(Instruction::VmovdquStore {
            base: BaseReg::Rsi,
            src: Ymm::Ymm0,
        });
        self
    }

    pub fn vzeroupper(mut self) -> Self {
        self.instructions.push(Instruction::Vzeroupper);
        self
    }

    pub fn finish(self) -> Vec<u8> {
        self.instructions
            .into_iter()
            .flat_map(Instruction::encode)
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Instruction {
    MovEaxImm32(u32),
    VmovdquLoad { dst: Ymm, base: BaseReg },
    VmovdquStore { base: BaseReg, src: Ymm },
    Vpxor { dst: Ymm, lhs: Ymm, rhs: Ymm },
    Vzeroupper,
    Ret,
}

impl Instruction {
    fn encode(self) -> Vec<u8> {
        match self {
            Self::MovEaxImm32(value) => [vec![0xb8], value.to_le_bytes().to_vec()].concat(),
            Self::VmovdquLoad { dst, base } => {
                vec![0xc5, 0xfe, 0x6f, modrm_memory(dst.code(), base.code())]
            }
            Self::VmovdquStore { base, src } => {
                vec![0xc5, 0xfe, 0x7f, modrm_memory(src.code(), base.code())]
            }
            Self::Vpxor { dst, lhs, rhs } => {
                debug_assert_eq!(dst, lhs);
                vec![0xc5, 0xfd, 0xef, modrm_register(dst.code(), rhs.code())]
            }
            Self::Vzeroupper => vec![0xc5, 0xf8, 0x77],
            Self::Ret => vec![0xc3],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ymm {
    Ymm0,
    Ymm1,
}

impl Ymm {
    const fn code(self) -> u8 {
        match self {
            Self::Ymm0 => 0,
            Self::Ymm1 => 1,
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

const fn modrm_memory(reg: u8, rm: u8) -> u8 {
    (reg << 3) | rm
}

const fn modrm_register(reg: u8, rm: u8) -> u8 {
    0xc0 | (reg << 3) | rm
}
