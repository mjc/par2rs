//! Comprehensive Packet Serialization Tests
//!
//! This test module combines all packet serialization tests into logical groups
//! for easier maintenance and understanding.

use binrw::{BinReaderExt, BinWrite};
use par2rs::packets::{
    creator_packet::CreatorPacket, file_description_packet::FileDescriptionPacket,
    main_packet::MainPacket,
};
use std::fs::File;
use std::io::Cursor;

mod creator_packet_tests {
    use super::*;

    #[test]
    fn serialized_length_matches_packet_length_field() {
        let mut file = File::open("tests/fixtures/packets/CreatorPacket.par2").unwrap();
        let creator_packet: CreatorPacket = file.read_le().unwrap();

        let mut buffer = Cursor::new(Vec::new());
        creator_packet.write_le(&mut buffer).unwrap();

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

        let mut buffer = Cursor::new(Vec::new());
        original_packet.write_le(&mut buffer).unwrap();

        buffer.set_position(0);
        let deserialized_packet: CreatorPacket = buffer.read_le().unwrap();

        assert_eq!(original_packet.length, deserialized_packet.length);
        assert_eq!(original_packet.md5, deserialized_packet.md5);
        assert_eq!(original_packet.set_id, deserialized_packet.set_id);
        assert_eq!(
            original_packet.creator_info,
            deserialized_packet.creator_info
        );
    }
}

mod file_description_packet_tests {
    use super::*;

    #[test]
    fn deserializes_packet_correctly() {
        let mut file = File::open("tests/fixtures/packets/FileDescriptionPacket.par2").unwrap();
        let file_description_packet: FileDescriptionPacket = file.read_le().unwrap();

        assert_eq!(file_description_packet.length, 128);
        assert_eq!(file_description_packet.packet_type, *b"PAR 2.0\0FileDesc");
        assert_eq!(file_description_packet.file_length, 1048576);

        assert_ne!(file_description_packet.file_id, [0; 16]);
        assert_ne!(file_description_packet.md5_hash, [0; 16]);
        assert_ne!(file_description_packet.md5_16k, [0; 16]);
    }

    #[test]
    fn filename_is_extracted_correctly() {
        let mut file = File::open("tests/fixtures/packets/FileDescriptionPacket.par2").unwrap();
        let file_description_packet: FileDescriptionPacket = file.read_le().unwrap();

        let filename_bytes = &file_description_packet.file_name;
        let null_pos = filename_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(filename_bytes.len());
        let filename = String::from_utf8_lossy(&filename_bytes[..null_pos]);

        assert_eq!(filename, "testfile");
    }
}

mod main_packet_tests {
    use super::*;

    #[test]
    fn validates_md5_hash() {
        let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
        let main_packet: MainPacket = file.read_le().unwrap();

        let expected_md5 = [
            0xbb, 0xcf, 0x29, 0x18, 0x55, 0x6d, 0x0c, 0xd3, 0xaf, 0xe9, 0x0a, 0xb5, 0x12, 0x3c,
            0x3f, 0xac,
        ];

        assert_eq!(main_packet.md5, expected_md5, "MD5 mismatch");
        assert_ne!(main_packet.md5, [0; 16], "MD5 should not be empty");
    }

    #[test]
    fn has_valid_packet_structure() {
        let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
        let main_packet: MainPacket = file.read_le().unwrap();

        assert!(main_packet.length > 0);
        assert_ne!(main_packet.set_id, [0; 16]);
        assert!(main_packet.slice_size > 0);
        assert!(main_packet.file_count > 0);
    }
}

mod recovery_slice_packet_tests {
    #[test]
    #[ignore]
    fn validates_recovery_slice_structure() {
        // This test would need a recovery slice packet fixture
        // For now, it's ignored until we have the proper test data

        // Example of what the test would look like:
        // use par2rs::packets::recovery_slice_packet::RecoverySlicePacket;
        // let mut file = File::open("tests/fixtures/packets/RecoverySlicePacket.par2").unwrap();
        // let recovery_packet: RecoverySlicePacket = file.read_le().unwrap();
        //
        // assert!(recovery_packet.length > 0);
    }
}

mod serialization_consistency {
    use super::*;

    #[test]
    fn all_packets_have_valid_lengths() {
        let mut creator_file = File::open("tests/fixtures/packets/CreatorPacket.par2").unwrap();
        let creator: CreatorPacket = creator_file.read_le().unwrap();
        assert!(creator.length > 64); // Minimum packet size

        let mut file_desc_file =
            File::open("tests/fixtures/packets/FileDescriptionPacket.par2").unwrap();
        let file_desc: FileDescriptionPacket = file_desc_file.read_le().unwrap();
        assert!(file_desc.length > 64);

        let mut main_file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
        let main: MainPacket = main_file.read_le().unwrap();
        assert!(main.length > 64);
    }

    #[test]
    fn all_packets_have_valid_set_ids() {
        let mut creator_file = File::open("tests/fixtures/packets/CreatorPacket.par2").unwrap();
        let creator: CreatorPacket = creator_file.read_le().unwrap();
        assert_ne!(creator.set_id, [0; 16]);

        let mut file_desc_file =
            File::open("tests/fixtures/packets/FileDescriptionPacket.par2").unwrap();
        let file_desc: FileDescriptionPacket = file_desc_file.read_le().unwrap();
        assert_ne!(file_desc.set_id, [0; 16]);

        let mut main_file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
        let main: MainPacket = main_file.read_le().unwrap();
        assert_ne!(main.set_id, [0; 16]);

        // All packets should have the same set ID
        assert_eq!(creator.set_id, file_desc.set_id);
        assert_eq!(file_desc.set_id, main.set_id);
    }
}
