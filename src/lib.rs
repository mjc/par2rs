pub mod args;

pub use args::parse_args;

use binread::BinRead;
use binread::BinReaderExt;

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct MainPacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(map = |b: [u8; 16]| String::from_utf8_lossy(&b).to_string(), pad_after = 4)]
    pub type_of_packet: String, // Type of the packet, converted to a string
    pub slice_size: u64,  // Size of each slice
    #[br(count = (length - 72) / 16)] // Calculate count based on packet length and header size
    pub file_ids: Vec<[u8; 16]>, // File IDs of all files in the recovery set
}

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct FileDescriptionPacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(map = |b: [u8; 16]| String::from_utf8_lossy(&b).to_string())]
    pub type_of_packet: String, // Type of the packet, converted to a string
    pub file_id: [u8; 16], // Unique identifier for the file
    pub md5_hash: [u8; 16], // MD5 hash of the entire file
    pub md5_16k: [u8; 16], // MD5 hash of the first 16kB of the file
    pub file_length: u64, // Length of the file
    #[br(count = length - 120)] // Adjusted count to account for removed magic field
    pub file_name: Vec<u8>, // Name of the file (not null-terminated)
}

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct InputFileSliceChecksumPacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(map = |b: [u8; 16]| String::from_utf8_lossy(&b).to_string())]
    pub type_of_packet: String, // Type of the packet, converted to a string
    pub file_id: [u8; 16], // File ID of the file
    #[br(count = (length - 64 - 16) / 20)]
    // Calculate count based on packet length and header size
    pub slice_checksums: Vec<([u8; 16], u32)>, // MD5 and CRC32 pairs for slices
}

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct RecoverySlicePacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(map = |b: [u8; 16]| String::from_utf8_lossy(&b).to_string())]
    pub type_of_packet: String, // Type of the packet, converted to a string
    pub exponent: u32,    // Exponent used to generate recovery data
    #[br(count = length as usize - (8 + 8 + 16 + 16 + 4))] // Subtract sizes of all other fields
    pub recovery_data: Vec<u8>, // Recovery data
}

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct CreatorPacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(map = |b: [u8; 16]| String::from_utf8_lossy(&b).to_string())]
    pub type_of_packet: String, // Type of the packet, converted to a string
    #[br(count = length as usize - (8 + 8 + 16 + 16 + 16))] // Subtract sizes of all other fields
    pub creator_info: Vec<u8>, // ASCII text identifying the client
}

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct PackedMainPacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
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

#[derive(Debug)]
pub enum Packet {
    MainPacket(MainPacket),
    PackedMainPacket(PackedMainPacket),
    FileDescriptionPacket(FileDescriptionPacket),
    RecoverySlicePacket(RecoverySlicePacket),
    CreatorPacket(CreatorPacket),
    InputFileSliceChecksumPacket(InputFileSliceChecksumPacket),
}

impl Packet {
    pub fn parse<R: std::io::Read + std::io::Seek>(reader: &mut R, _type_of_packet: &[u8]) -> Option<Self> {
        let mut header = [0u8; 64];
        reader.read_exact(&mut header).ok()?;
        let type_of_packet = &header[48..64];

        // Rewind the reader to the start of the packet
        reader.seek(std::io::SeekFrom::Current(-64)).ok()?;

        let packet_parsers: &[(&[u8], fn(&mut R) -> Option<Packet>)] = &[
            (b"PAR 2.0\0Main\0\0\0\0", |r: &mut R| r.read_le::<MainPacket>().ok().map(Packet::MainPacket)),
            (b"PAR 2.0\0PkdMain\0", |r: &mut R| r.read_le::<PackedMainPacket>().ok().map(Packet::PackedMainPacket)),
            (b"PAR 2.0\0FileDesc", |r: &mut R| r.read_le::<FileDescriptionPacket>().ok().map(Packet::FileDescriptionPacket)),
            (b"PAR 2.0\0RecvSlic", |r: &mut R| r.read_le::<RecoverySlicePacket>().ok().map(Packet::RecoverySlicePacket)),
            (b"PAR 2.0\0Creator\0", |r: &mut R| r.read_le::<CreatorPacket>().ok().map(Packet::CreatorPacket)),
            (b"PAR 2.0\0IFSC\0\0\0\0", |r: &mut R| r.read_le::<InputFileSliceChecksumPacket>().ok().map(Packet::InputFileSliceChecksumPacket)),
        ];

        for (packet_type, parser) in packet_parsers {
            if type_of_packet == *packet_type {
                return parser(reader);
            }
        }

        None
    }
}

pub fn parse_packets<R: std::io::Read + std::io::Seek>(reader: &mut R) -> Vec<Packet> {
    let mut packets = Vec::new();

    while let Some(packet) = Packet::parse(reader, &[]) {
        packets.push(packet);
    }

    packets
}
