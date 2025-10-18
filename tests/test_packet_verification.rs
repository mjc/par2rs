//! Packet verification and formatting tests

use binrw::BinReaderExt;
use par2rs::domain::{FileId, Md5Hash, RecoverySetId};
use par2rs::packets::{CreatorPacket, MainPacket, Packet};
use std::fs::File;

mod main_packet {
    use super::*;

    #[test]
    fn verify_with_fixture() {
        if let Ok(mut file) = File::open("tests/fixtures/packets/MainPacket.par2") {
            if let Ok(packet) = file.read_le::<MainPacket>() {
                assert!(packet.verify());
                assert!(!packet.file_ids.is_empty());
            }
        }
    }

    #[test]
    fn display() {
        let packet = MainPacket {
            length: 92,
            md5: Md5Hash::new([0u8; 16]),
            set_id: RecoverySetId::new([1u8; 16]),
            slice_size: 1024,
            file_count: 1,
            file_ids: vec![FileId::new([2u8; 16])],
            non_recovery_file_ids: vec![],
        };
        let s = format!("{}", packet);
        assert!(s.contains("MainPacket"));
    }

    #[test]
    fn debug() {
        let packet = MainPacket {
            length: 92,
            md5: Md5Hash::new([0u8; 16]),
            set_id: RecoverySetId::new([1u8; 16]),
            slice_size: 2048,
            file_count: 2,
            file_ids: vec![FileId::new([2u8; 16]), FileId::new([3u8; 16])],
            non_recovery_file_ids: vec![],
        };
        let s = format!("{:?}", packet);
        assert!(s.contains("MainPacket"));
    }

    #[test]
    fn with_non_recovery() {
        let packet = MainPacket {
            length: 92,
            md5: Md5Hash::new([0u8; 16]),
            set_id: RecoverySetId::new([1u8; 16]),
            slice_size: 1024,
            file_count: 1,
            file_ids: vec![FileId::new([2u8; 16])],
            non_recovery_file_ids: vec![FileId::new([3u8; 16])],
        };
        assert_eq!(packet.non_recovery_file_ids.len(), 1);
    }

    #[test]
    fn invalid_verify() {
        let packet = MainPacket {
            length: 50,
            md5: Md5Hash::new([0u8; 16]),
            set_id: RecoverySetId::new([1u8; 16]),
            slice_size: 1024,
            file_count: 1,
            file_ids: vec![],
            non_recovery_file_ids: vec![],
        };
        assert!(!packet.verify());
    }
}

mod creator_packet {
    use super::*;

    #[test]
    fn parse_fixture() {
        if let Ok(mut file) = File::open("tests/fixtures/packets/CreatorPacket.par2") {
            let _ = file.read_le::<CreatorPacket>();
        }
    }

    #[test]
    fn in_enum() {
        if let Ok(mut file) = File::open("tests/fixtures/packets/CreatorPacket.par2") {
            if let Ok(packet) = file.read_le::<CreatorPacket>() {
                let _ = Packet::Creator(packet);
            }
        }
    }

    #[test]
    fn empty_creator() {
        let packet = CreatorPacket {
            length: 56,
            md5: Md5Hash::new([0u8; 16]),
            set_id: RecoverySetId::new([0u8; 16]),
            creator_info: Vec::new(),
        };
        let _ = packet.verify();
    }

    #[test]
    fn long_creator() {
        let s = b"A".repeat(1000);
        let packet = CreatorPacket {
            length: 56 + s.len() as u64,
            md5: Md5Hash::new([0u8; 16]),
            set_id: RecoverySetId::new([0u8; 16]),
            creator_info: s,
        };
        let _ = packet.verify();
    }
}
