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
        self.instructions.push(Instruction::VmovdquYmm0FromRdi);
        self
    }

    pub fn vmovdqu_ymm1_from_rsi(mut self) -> Self {
        self.instructions.push(Instruction::VmovdquYmm1FromRsi);
        self
    }

    pub fn vpxor_ymm0_ymm0_ymm1(mut self) -> Self {
        self.instructions.push(Instruction::VpxorYmm0Ymm0Ymm1);
        self
    }

    pub fn vmovdqu_rsi_from_ymm0(mut self) -> Self {
        self.instructions.push(Instruction::VmovdquRsiFromYmm0);
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
    VmovdquYmm0FromRdi,
    VmovdquYmm1FromRsi,
    VpxorYmm0Ymm0Ymm1,
    VmovdquRsiFromYmm0,
    Vzeroupper,
    Ret,
}

impl Instruction {
    fn encode(self) -> Vec<u8> {
        match self {
            Self::MovEaxImm32(value) => [vec![0xb8], value.to_le_bytes().to_vec()].concat(),
            Self::VmovdquYmm0FromRdi => vec![0xc5, 0xfe, 0x6f, 0x07],
            Self::VmovdquYmm1FromRsi => vec![0xc5, 0xfe, 0x6f, 0x0e],
            Self::VpxorYmm0Ymm0Ymm1 => vec![0xc5, 0xfd, 0xef, 0xc1],
            Self::VmovdquRsiFromYmm0 => vec![0xc5, 0xfe, 0x7f, 0x06],
            Self::Vzeroupper => vec![0xc5, 0xf8, 0x77],
            Self::Ret => vec![0xc3],
        }
    }
}
