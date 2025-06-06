//! MD5 Verification Tests
//!
//! Tests for MD5 hash validation in packet headers and data integrity verification.

use binrw::BinReaderExt;
use par2rs::packets::main_packet::MainPacket;
use std::fs::File;

mod packet_md5_validation {
    use super::*;

    #[test]
    fn validates_main_packet_md5_hash() {
        let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
        let main_packet: MainPacket = file.read_le().unwrap();

        let expected_md5 = [
            0xbb, 0xcf, 0x29, 0x18, 0x55, 0x6d, 0x0c, 0xd3, 0xaf, 0xe9, 0x0a, 0xb5, 0x12, 0x3c,
            0x3f, 0xac,
        ];

        assert_eq!(main_packet.md5, expected_md5, "MD5 mismatch");
    }

    #[test]
    fn verifies_md5_is_not_empty() {
        let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
        let main_packet: MainPacket = file.read_le().unwrap();

        // MD5 should not be all zeros
        assert_ne!(main_packet.md5, [0; 16], "MD5 should not be empty");
    }

    #[test]
    fn validates_md5_length() {
        let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
        let main_packet: MainPacket = file.read_le().unwrap();

        // MD5 should always be 16 bytes
        assert_eq!(main_packet.md5.len(), 16, "MD5 should be 16 bytes");
    }
}
