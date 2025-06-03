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

#[derive(Debug, BinRead)]
pub struct MainPacket {
    pub slice_size: u64, // Size of each slice
    pub file_count: u32, // Number of files in the recovery set
    #[br(count = 16)]
    pub recovery_set_ids: Vec<[u8; 16]>, // File IDs of recovery set
    #[br(count = 16)]
    pub non_recovery_set_ids: Vec<[u8; 16]>, // File IDs of non-recovery set
}

#[derive(Debug, BinRead)]
pub struct FileDescriptionPacket {
    pub file_id: [u8; 16], // Unique identifier for the file
    pub md5_hash: [u8; 16], // MD5 hash of the entire file
    pub md5_16k: [u8; 16], // MD5 hash of the first 16kB of the file
    pub file_length: u64, // Length of the file
    #[br(parse_with = parse_file_name)]
    pub file_name: String, // Name of the file (variable-length, multiples of 4 bytes)
}

fn parse_file_name<R: binread::io::Read + binread::io::Seek>(reader: &mut R, _: &binread::ReadOptions, _: ()) -> binread::BinResult<String> {
    let start_pos = reader.stream_position()?;
    let mut buffer = Vec::new();
    loop {
        let mut chunk = [0u8; 4];
        let bytes_read = reader.read(&mut chunk)?;
        if bytes_read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..bytes_read]);
        if bytes_read < 4 {
            break;
        }
    }

    String::from_utf8(buffer).map_err(|_| binread::Error::Custom {
        pos: start_pos,
        err: Box::new(String::from("Invalid UTF-8 in file_name")),
    })
}

#[derive(Debug, BinRead)]
pub struct InputFileSliceChecksumPacket {
    pub file_id: [u8; 16], // File ID of the file
    #[br(count = 20)]
    pub slice_checksums: Vec<( [u8; 16], u32 )>, // MD5 and CRC32 pairs for slices
}

#[derive(Debug, BinRead)]
pub struct RecoverySlicePacket {
    pub exponent: u32, // Exponent used to generate recovery data
    #[br(count = 512)]
    pub recovery_data: Vec<u8>, // Recovery data
}

#[derive(Debug, BinRead)]
pub struct CreatorPacket {
    #[br(count = 256)]
    pub creator_info: Vec<u8>, // ASCII text identifying the client
}
