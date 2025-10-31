use binrw::{BinReaderExt, BinWrite};
use par2rs::packets::CreatorPacket;
use std::fs::File;
use std::io::Cursor;

#[test]
fn test_creator_packet_verify_valid() {
    let mut file = File::open("tests/fixtures/packets/CreatorPacket.par2").unwrap();
    let packet: CreatorPacket = file.read_le().unwrap();
    assert!(packet.verify());
}

#[test]
fn test_creator_packet_binwrite() {
    let mut file = File::open("tests/fixtures/packets/CreatorPacket.par2").unwrap();
    let packet: CreatorPacket = file.read_le().unwrap();

    let mut buffer = Cursor::new(Vec::new());
    packet.write_le(&mut buffer).unwrap();

    assert_eq!(buffer.get_ref().len() as u64, packet.length);
}

#[test]
fn test_creator_packet_roundtrip() {
    let mut file = File::open("tests/fixtures/packets/CreatorPacket.par2").unwrap();
    let original: CreatorPacket = file.read_le().unwrap();

    // Write to buffer
    let mut write_buffer = Cursor::new(Vec::new());
    original.write_le(&mut write_buffer).unwrap();

    // Read back
    let mut read_buffer = Cursor::new(write_buffer.into_inner());
    let roundtrip: CreatorPacket = read_buffer.read_le().unwrap();

    assert_eq!(original.length, roundtrip.length);
    assert_eq!(original.md5.as_bytes(), roundtrip.md5.as_bytes());
    assert_eq!(original.set_id.as_bytes(), roundtrip.set_id.as_bytes());
    assert_eq!(original.creator_info, roundtrip.creator_info);
}

#[test]
fn test_creator_packet_creator_info() {
    let mut file = File::open("tests/fixtures/packets/CreatorPacket.par2").unwrap();
    let packet: CreatorPacket = file.read_le().unwrap();

    // Creator info should not be empty
    assert!(!packet.creator_info.is_empty());

    // Should be valid ASCII/UTF-8
    let creator_str = String::from_utf8_lossy(&packet.creator_info);
    assert!(!creator_str.is_empty());
}

#[test]
fn test_creator_packet_type_constant() {
    let expected = b"PAR 2.0\0Creator\0";
    assert_eq!(par2rs::packets::creator_packet::TYPE_OF_PACKET, expected);
    assert_eq!(par2rs::packets::creator_packet::TYPE_OF_PACKET.len(), 16);
}

#[test]
fn test_creator_packet_minimum_length() {
    let mut file = File::open("tests/fixtures/packets/CreatorPacket.par2").unwrap();
    let packet: CreatorPacket = file.read_le().unwrap();

    // Minimum packet length is 64 bytes (header fields)
    assert!(packet.length >= 64);
}

#[test]
fn test_creator_packet_md5_hash_format() {
    let mut file = File::open("tests/fixtures/packets/CreatorPacket.par2").unwrap();
    let packet: CreatorPacket = file.read_le().unwrap();

    // MD5 hash should be 16 bytes
    assert_eq!(packet.md5.as_bytes().len(), 16);
}

#[test]
fn test_creator_packet_set_id_format() {
    let mut file = File::open("tests/fixtures/packets/CreatorPacket.par2").unwrap();
    let packet: CreatorPacket = file.read_le().unwrap();

    // Recovery set ID should be 16 bytes
    assert_eq!(packet.set_id.as_bytes().len(), 16);
}
