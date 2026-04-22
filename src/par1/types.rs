use crate::domain::Md5Hash;

pub const PAR1_MAGIC: &[u8; 8] = b"PAR\0\0\0\0\0";
pub const PAR1_FILE_VERSION: u32 = 0x0001_0000;
pub const PAR1_HEADER_SIZE: usize = 96;
pub const PAR1_FILE_ENTRY_FIXED_SIZE: usize = 56;
pub const PAR1_STATUS_IN_PARITY_VOLUME: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Par1Set {
    pub set_hash: Md5Hash,
    pub program_version: u32,
    pub volume_number: u64,
    pub files: Vec<Par1FileEntry>,
    pub volume: Option<Par1Volume>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Par1FileEntry {
    pub status: u64,
    pub file_size: u64,
    pub hash_full: Md5Hash,
    pub hash_16k: Md5Hash,
    pub name: String,
}

impl Par1FileEntry {
    pub fn is_protected_file(&self) -> bool {
        self.status & PAR1_STATUS_IN_PARITY_VOLUME != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Par1Volume {
    pub exponent: u32,
    pub data_offset: u64,
    pub data_size: u64,
    pub recovery_data: Vec<u8>,
}
