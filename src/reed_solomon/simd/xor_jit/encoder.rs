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
    Ret,
}

impl Instruction {
    fn encode(self) -> Vec<u8> {
        match self {
            Self::MovEaxImm32(value) => [vec![0xb8], value.to_le_bytes().to_vec()].concat(),
            Self::Ret => vec![0xc3],
        }
    }
}
