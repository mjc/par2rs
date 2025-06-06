use binrw::BinReaderExt;
use std::io::{Read, Seek, SeekFrom};

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
        let (type_of_packet, _packet_length) = Self::get_packet_type(reader)?;

        let packet = Self::match_packet_type(reader, &type_of_packet)?;

        Some(packet)
    }

    fn get_packet_type<R: Read + Seek>(reader: &mut R) -> Option<([u8; 16], u64)> {
        let mut header = [0u8; 64];
        if reader.read_exact(&mut header).is_err() {
            return None;
        }

        // Check magic signature
        if &header[0..8] != MAGIC_BYTES {
            return None;
        }

        let type_of_packet: [u8; 16] = header[48..64].try_into().ok()?;
        let packet_length = u64::from_le_bytes(header[8..16].try_into().ok()?);

        // Rewind to start of packet so the packet struct can read the magic bytes again
        reader.seek(SeekFrom::Current(-64)).ok()?;

        Some((type_of_packet, packet_length))
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
