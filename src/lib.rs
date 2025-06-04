pub mod args;

pub use args::parse_args;

use binread::BinRead;

#[derive(Debug, BinRead)]
pub struct MainPacket {
    pub magic: [u8; 8], // "PAR2\0PKT"
    pub length: u64,   // Length of the packet
    pub md5: [u8; 16], // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(map = |b: [u8; 16]| String::from_utf8_lossy(&b).to_string(), pad_after = 4)]
    pub type_of_packet: String, // Type of the packet, converted to a string
    pub slice_size: u64, // Size of each slice
    #[br(count = (length - 72) / 16)] // Calculate count based on packet length and header size
    pub file_ids: Vec<[u8; 16]>, // File IDs of all files in the recovery set
}

#[derive(Debug, BinRead)]
pub struct FileDescriptionPacket {
    pub magic: [u8; 8], // "PAR2\0PKT"
    pub length: u64,   // Length of the packet
    pub md5: [u8; 16], // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(map = |b: [u8; 16]| String::from_utf8_lossy(&b).to_string())]
    pub type_of_packet: String, // Type of the packet, converted to a string
    pub file_id: [u8; 16], // Unique identifier for the file
    pub md5_hash: [u8; 16], // MD5 hash of the entire file
    pub md5_16k: [u8; 16], // MD5 hash of the first 16kB of the file
    pub file_length: u64, // Length of the file
    #[br(count = length - 120)] // Subtract sizes of all other fields
    pub file_name: Vec<u8>, // Name of the file (not null-terminated)
}

#[derive(Debug, BinRead)]
pub struct InputFileSliceChecksumPacket {
    pub magic: [u8; 8], // "PAR2\0PKT"
    pub length: u64,   // Length of the packet
    pub md5: [u8; 16], // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(map = |b: [u8; 16]| String::from_utf8_lossy(&b).to_string())]
    pub type_of_packet: String, // Type of the packet, converted to a string
    pub file_id: [u8; 16], // File ID of the file
    #[br(count = (length - 64 - 16) / 20)] // Calculate count based on packet length and header size
    pub slice_checksums: Vec<([u8; 16], u32)>, // MD5 and CRC32 pairs for slices
}

#[derive(Debug, BinRead)]
pub struct RecoverySlicePacket {
    pub magic: [u8; 8], // "PAR2\0PKT"
    pub length: u64,   // Length of the packet
    pub md5: [u8; 16], // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(map = |b: [u8; 16]| String::from_utf8_lossy(&b).to_string())]
    pub type_of_packet: String, // Type of the packet, converted to a string
    pub exponent: u32, // Exponent used to generate recovery data
    #[br(count = length as usize - (8 + 8 + 16 + 16 + 4))] // Subtract sizes of all other fields
    pub recovery_data: Vec<u8>, // Recovery data
}

#[derive(Debug, BinRead)]
pub struct CreatorPacket {
    pub magic: [u8; 8], // "PAR2\0PKT"
    pub length: u64,   // Length of the packet
    pub md5: [u8; 16], // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(map = |b: [u8; 16]| String::from_utf8_lossy(&b).to_string())]
    pub type_of_packet: String, // Type of the packet, converted to a string
    #[br(count = length as usize - (8 + 8 + 16 + 16 + 16))] // Subtract sizes of all other fields
    pub creator_info: Vec<u8>, // ASCII text identifying the client
}

#[derive(Debug, BinRead)]
pub struct PackedMainPacket {
    pub magic: [u8; 8], // "PAR2\0PKT"
    pub length: u64,   // Length of the packet
    pub md5: [u8; 16], // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(map = |b: [u8; 16]| String::from_utf8_lossy(&b).to_string())]
    pub type_of_packet: String, // Type of the packet, converted to a string
    pub subslice_size: u64, // Subslice size. Must be a multiple of 4 and equally divide the slice size.
    pub slice_size: u64, // Slice size. Must be a multiple of 4 and a multiple of the subslice size.
    pub file_count: u32, // Number of files in the recovery set.
    #[br(count = file_count)]
    pub recovery_set_ids: Vec<[u8; 16]>, // File IDs of all files in the recovery set.
    #[br(count = (length as usize - 64 - 8 - 8 - 4 - (file_count as usize * 16)) / 16)]
    pub non_recovery_set_ids: Vec<[u8; 16]>, // File IDs of all files in the non-recovery set.
}

pub enum Packet {
    MainPacket(MainPacket),
    PackedMainPacket(PackedMainPacket),
    FileDescriptionPacket(FileDescriptionPacket),
    RecoverySlicePacket(RecoverySlicePacket),
    CreatorPacket(CreatorPacket),
    InputFileSliceChecksumPacket(InputFileSliceChecksumPacket),
}
