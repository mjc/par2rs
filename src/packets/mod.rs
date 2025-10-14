use binrw::BinReaderExt;
use std::io::{Read, Seek};

pub mod creator_packet;
pub mod file_description_packet;
pub mod input_file_slice_checksum_packet;
pub mod main_packet;
pub mod packed_main_packet;
pub mod recovery_slice_packet;

pub use creator_packet::CreatorPacket;
pub use file_description_packet::FileDescriptionPacket;
pub use input_file_slice_checksum_packet::InputFileSliceChecksumPacket;
pub use main_packet::MainPacket;
pub use packed_main_packet::PackedMainPacket;
pub use recovery_slice_packet::RecoverySlicePacket;

pub const MAGIC_BYTES: &[u8] = b"PAR2\0PKT";

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
    pub fn verify(&self) -> bool {
        match self {
            Packet::Main(packet) => packet.verify(),
            Packet::PackedMain(packet) => packet.verify(),
            Packet::FileDescription(packet) => packet.verify(),
            Packet::RecoverySlice(packet) => packet.verify(),
            Packet::Creator(packet) => packet.verify(),
            Packet::InputFileSliceChecksum(packet) => packet.verify(),
        }
    }

    pub fn parse<R: Read + Seek>(reader: &mut R) -> Option<Self> {
        // OPTIMIZATION: Read entire packet into memory buffer first
        // This is much faster than letting binrw do many small reads
        let mut header = [0u8; 64];
        if reader.read_exact(&mut header).is_err() {
            return None;
        }

        // Check magic signature
        if &header[0..8] != MAGIC_BYTES {
            return None;
        }

        let type_of_packet: [u8; 16] = header[48..64].try_into().ok()?;
        let packet_length = u64::from_le_bytes(header[8..16].try_into().ok()?) as usize;

        // Validate packet length
        if !(64..=100 * 1024 * 1024).contains(&packet_length) {
            return None;
        }

        // Read the entire packet into a buffer (we already have the first 64 bytes)
        let mut packet_data = vec![0u8; packet_length];
        packet_data[..64].copy_from_slice(&header);

        if reader.read_exact(&mut packet_data[64..]).is_err() {
            return None;
        }

        // Parse from memory buffer (much faster than streaming)
        let mut cursor = std::io::Cursor::new(&packet_data);
        Self::match_packet_type(&mut cursor, &type_of_packet)
    }

    fn match_packet_type<R: Read + Seek>(reader: &mut R, type_of_packet: &[u8]) -> Option<Self> {
        match type_of_packet {
            main_packet::TYPE_OF_PACKET => reader.read_le::<MainPacket>().ok().map(Packet::Main),
            packed_main_packet::TYPE_OF_PACKET => reader
                .read_le::<PackedMainPacket>()
                .ok()
                .map(Packet::PackedMain),
            file_description_packet::TYPE_OF_PACKET => reader
                .read_le::<FileDescriptionPacket>()
                .ok()
                .map(Packet::FileDescription),
            recovery_slice_packet::TYPE_OF_PACKET => reader
                .read_le::<RecoverySlicePacket>()
                .ok()
                .map(Packet::RecoverySlice),
            creator_packet::TYPE_OF_PACKET => {
                reader.read_le::<CreatorPacket>().ok().map(Packet::Creator)
            }
            input_file_slice_checksum_packet::TYPE_OF_PACKET => reader
                .read_le::<InputFileSliceChecksumPacket>()
                .ok()
                .map(Packet::InputFileSliceChecksum),
            _ => None,
        }
    }
}

pub fn parse_packets<R: Read + Seek>(reader: &mut R) -> Vec<Packet> {
    let mut packets = Vec::new();

    while let Some(packet) = Packet::parse(reader) {
        packets.push(packet);
    }

    packets
}
