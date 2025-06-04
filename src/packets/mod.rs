use std::io::{Read, Seek, SeekFrom};
use binrw::{BinRead, BinReaderExt};

pub mod main_packet;
pub mod file_description_packet;
pub mod input_file_slice_checksum_packet;
pub mod recovery_slice_packet;
pub mod creator_packet;
pub mod packed_main_packet;

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct MainPacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
    pub slice_size: u64,  // Size of each slice
    #[br(count = (length - 72) / 16)]
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
    pub fn parse<R: Read + Seek>(reader: &mut R) -> Option<Self> {
        let (type_of_packet, packet_length) = Self::get_packet_type(reader)?;

        let packet = Self::match_packet_type(reader, &type_of_packet)?;

        Self::seek_to_end_of_packet(reader, packet_length);

        Some(packet)
    }

    fn get_packet_type<R: Read + Seek>(reader: &mut R) -> Option<([u8; 16], u64)> {
        let mut header = [0u8; 64];
        if reader.read_exact(&mut header).is_err() {
            return None;
        }
        // Rewind the reader to the start of the packet
        reader.seek(SeekFrom::Current(-64)).ok()?;
        let type_of_packet = header[48..64].try_into().ok()?;
        let packet_length = u64::from_le_bytes(header[0..8].try_into().ok()?);
        Some((type_of_packet, packet_length))
    }

    fn match_packet_type<R: Read + Seek>(reader: &mut R, type_of_packet: &[u8]) -> Option<Self> {
        match type_of_packet {
            b"PAR 2.0\0Main\0\0\0\0" => reader.read_le::<MainPacket>().ok().map(Packet::Main),
            b"PAR 2.0\0PkdMain\0" => reader.read_le::<PackedMainPacket>().ok().map(Packet::PackedMain),
            b"PAR 2.0\0FileDesc" => reader.read_le::<FileDescriptionPacket>().ok().map(Packet::FileDescription),
            b"PAR 2.0\0RecvSlic" => reader.read_le::<RecoverySlicePacket>().ok().map(Packet::RecoverySlice),
            b"PAR 2.0\0Creator\0" => reader.read_le::<CreatorPacket>().ok().map(Packet::Creator),
            b"PAR 2.0\0IFSC\0\0\0\0" => reader.read_le::<InputFileSliceChecksumPacket>().ok().map(Packet::InputFileSliceChecksum),
            _ => None,
        }
    }

    fn seek_to_end_of_packet<R: Seek>(reader: &mut R, packet_length: u64) {
        reader.seek(SeekFrom::Current(packet_length as i64 - 64)).ok();
    }
}

pub fn parse_packets<R: Read + Seek>(reader: &mut R) -> Vec<Packet> {
    let mut packets = Vec::new();

    while let Some(packet) = Packet::parse(reader) {
        packets.push(packet);

        if reader.stream_position().unwrap() >= reader.seek(SeekFrom::End(0)).unwrap() {
            break;
        }
    }

    packets
}
