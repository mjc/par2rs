#![allow(dead_code)]

#[derive(Debug, Default)]
pub struct Program {
    bytes: Vec<u8>,
}

impl Program {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mov_eax_imm32(mut self, value: u32) -> Self {
        self.bytes.push(0xb8);
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub fn ret(mut self) -> Self {
        self.bytes.push(0xc3);
        self
    }

    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}
