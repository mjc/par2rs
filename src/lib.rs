pub mod args;

pub use args::parse_args;

use binread::BinRead;

#[derive(Debug, BinRead)]
pub struct Par2Header {
    pub magic: [u8; 8], // "PAR2\0PKT"
    pub length: u64,   // Length of the packet
    pub md5: [u8; 16], // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    pub type_of_packet: [u8; 16], // Type of the packet
    pub creator: [u8; 16], // Creator of the PAR2 file
    pub file_count: u32,  // Number of files in the PAR2 set
    pub recovery_block_count: u32, // Number of recovery blocks
    pub recovery_block_size: u64,  // Size of each recovery block
    #[br(count = 256)] // Example: Adjust the count based on the PAR2 specification
    pub file_description: Vec<u8>, // Description of the file

    #[br(count = 512)] // Example: Adjust the count based on the PAR2 specification
    pub packet_data: Vec<u8>, // Additional packet data
}
