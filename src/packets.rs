use binrw::BinRead;
use binrw::BinReaderExt;

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct MainPacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
    pub slice_size: u64, // Size of each slice
    #[br(count = (length - 72) / 16)] // Calculate count based on packet length and header size
    pub file_ids: Vec<[u8; 16]>, // File IDs of all files in the recovery set
    #[br(count = (length - 72 - (file_ids.len() as u64 * 16)) / 16)]
    pub non_recovery_file_ids: Vec<[u8; 16]>, // File IDs of all files in the non-recovery set
}

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct FileDescriptionPacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
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
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
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
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
    pub exponent: u32, // Exponent used to generate recovery data
    #[br(count = length as usize - (8 + 8 + 16 + 16 + 4))] // Subtract sizes of all other fields
    pub recovery_data: Vec<u8>, // Recovery data
}

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct CreatorPacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
    #[br(count = length as usize - (8 + 8 + 16 + 16 + 16))]
    // Subtract sizes of all other fields
    pub creator_info: Vec<u8>, // ASCII text identifying the client
}

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct PackedMainPacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
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
    Main(MainPacket),
    PackedMain(PackedMainPacket),
    FileDescription(FileDescriptionPacket),
    RecoverySlice(RecoverySlicePacket),
    Creator(CreatorPacket),
    InputFileSliceChecksum(InputFileSliceChecksumPacket),
}

impl Packet {
    pub fn from_bytes<R: std::io::Read + std::io::Seek>(
        bytes: &[u8],
        reader: &mut R,
    ) -> Option<Self> {
        match bytes {
            b"PAR 2.0\0Main\0\0\0\0" => reader.read_le::<MainPacket>().ok().map(Packet::Main),
            b"PAR 2.0\0PkdMain\0" => reader.read_le::<PackedMainPacket>().ok().map(Packet::PackedMain),
            b"PAR 2.0\0FileDesc" => reader.read_le::<FileDescriptionPacket>().ok().map(Packet::FileDescription),
            b"PAR 2.0\0RecvSlic" => reader.read_le::<RecoverySlicePacket>().ok().map(Packet::RecoverySlice),
            b"PAR 2.0\0Creator\0" => reader.read_le::<CreatorPacket>().ok().map(Packet::Creator),
            b"PAR 2.0\0IFSC\0\0\0\0" => reader.read_le::<InputFileSliceChecksumPacket>().ok().map(Packet::InputFileSliceChecksum),
            _ => None,
        }
    }
}



pub fn parse_packets<R: std::io::Read + std::io::Seek>(reader: &mut R) -> Vec<Packet> {
    let mut packets = Vec::new();
    let mut header = [0u8; 64];

    while let Ok(_) = reader.read_exact(&mut header) {
        let type_of_packet = &header[48..64];

        // Rewind the reader to the start of the packet
        reader.seek(std::io::SeekFrom::Current(-64)).expect("Failed to rewind reader");

        if let Some(packet) = Packet::from_bytes(type_of_packet, reader) {
            packets.push(packet);

            // Advance the reader by the packet length
            let packet_length = u64::from_le_bytes(header[0..8].try_into().unwrap());
            reader.seek(std::io::SeekFrom::Current(packet_length as i64 - 64)).expect("Failed to advance reader");
        } else {
            // Skip the rest of the packet if it cannot be parsed
            let packet_length = u64::from_le_bytes(header[0..8].try_into().unwrap());
            reader.seek(std::io::SeekFrom::Current(packet_length as i64 - 64)).expect("Failed to skip packet");
        }

        // Break the loop if the reader reaches the end of the file
        if reader.stream_position().unwrap() >= reader.seek(std::io::SeekFrom::End(0)).unwrap() {
            break;
        }
    }

    packets
}
