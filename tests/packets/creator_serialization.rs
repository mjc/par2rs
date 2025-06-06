//! Creator Packet Serialization Tests
//!
//! Tests for proper serialization and deserialization of creator packets.

use binrw::{BinReaderExt, BinWrite};
use par2rs::packets::creator_packet::CreatorPacket;
use std::fs::File;
use std::io::Cursor;

mod serialization {
    use super::*;

    #[test]
    fn serialized_length_matches_packet_length_field() {
        // Open the test fixture file
        let mut file = File::open("tests/fixtures/packets/CreatorPacket.par2").unwrap();

        // Read the CreatorPacket from the file
        let creator_packet: CreatorPacket = file.read_le().unwrap();

        // Serialize the packet into a buffer
        let mut buffer = Cursor::new(Vec::new());
        creator_packet.write_le(&mut buffer).unwrap();

        // Verify that the serialized length matches the packet's length field
        let serialized_length = buffer.get_ref().len() as u64;
        assert_eq!(
            serialized_length, creator_packet.length,
            "Serialized length mismatch: expected {}, got {}",
            creator_packet.length, serialized_length
        );
    }

    #[test]
    fn round_trip_serialization_preserves_data() {
        let mut file = File::open("tests/fixtures/packets/CreatorPacket.par2").unwrap();
        let original_packet: CreatorPacket = file.read_le().unwrap();

        // Serialize the packet
        let mut buffer = Cursor::new(Vec::new());
        original_packet.write_le(&mut buffer).unwrap();

        // Deserialize it back
        buffer.set_position(0);
        let deserialized_packet: CreatorPacket = buffer.read_le().unwrap();

        // Verify fields match (only comparing fields that actually exist)
        assert_eq!(original_packet.length, deserialized_packet.length);
        assert_eq!(original_packet.md5, deserialized_packet.md5);
        assert_eq!(original_packet.set_id, deserialized_packet.set_id);
        assert_eq!(
            original_packet.creator_info,
            deserialized_packet.creator_info
        );
    }
}
