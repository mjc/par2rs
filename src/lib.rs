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
    // Add more fields as per the PAR2 specification
}
