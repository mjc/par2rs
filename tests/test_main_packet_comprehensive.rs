use binrw::{BinReaderExt, BinWrite};
use par2rs::packets::MainPacket;
use std::fs::File;
use std::io::Cursor;

#[test]
fn test_main_packet_verify_valid() {
    let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
    let packet: MainPacket = file.read_le().unwrap();
    assert!(packet.verify());
}

#[test]
fn test_main_packet_binwrite() {
    let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
    let packet: MainPacket = file.read_le().unwrap();

    let mut buffer = Cursor::new(Vec::new());
    packet.write_le(&mut buffer).unwrap();

    assert_eq!(buffer.get_ref().len() as u64, packet.length);
}

#[test]
fn test_main_packet_roundtrip() {
    let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
    let original: MainPacket = file.read_le().unwrap();

    // Write to buffer
    let mut write_buffer = Cursor::new(Vec::new());
    original.write_le(&mut write_buffer).unwrap();

    // Read back
    let mut read_buffer = Cursor::new(write_buffer.into_inner());
    let roundtrip: MainPacket = read_buffer.read_le().unwrap();

    assert_eq!(original.length, roundtrip.length);
    assert_eq!(original.md5.as_bytes(), roundtrip.md5.as_bytes());
    assert_eq!(original.set_id.as_bytes(), roundtrip.set_id.as_bytes());
    assert_eq!(original.slice_size, roundtrip.slice_size);
    assert_eq!(original.file_count, roundtrip.file_count);
}

#[test]
fn test_main_packet_type_constant() {
    let expected = b"PAR 2.0\0Main\0\0\0\0";
    assert_eq!(par2rs::packets::main_packet::TYPE_OF_PACKET, expected);
    assert_eq!(par2rs::packets::main_packet::TYPE_OF_PACKET.len(), 16);
}

#[test]
fn test_main_packet_minimum_length() {
    let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
    let packet: MainPacket = file.read_le().unwrap();

    // Minimum packet length is 64 bytes (header fields)
    assert!(packet.length >= 64);
}

#[test]
fn test_main_packet_slice_size_nonzero() {
    let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
    let packet: MainPacket = file.read_le().unwrap();

    // Slice size should be non-zero and typically a power of 2
    assert!(packet.slice_size > 0);
}

#[test]
fn test_main_packet_file_count_matches() {
    let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
    let packet: MainPacket = file.read_le().unwrap();

    // File count should match the number of file IDs
    assert_eq!(packet.file_count as usize, packet.file_ids.len());
}

#[test]
fn test_main_packet_file_ids_not_empty() {
    let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
    let packet: MainPacket = file.read_le().unwrap();

    // Should have at least one file
    assert!(!packet.file_ids.is_empty());
}

#[test]
fn test_main_packet_md5_hash_format() {
    let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
    let packet: MainPacket = file.read_le().unwrap();

    // MD5 hash should be 16 bytes
    assert_eq!(packet.md5.as_bytes().len(), 16);
}

#[test]
fn test_main_packet_set_id_format() {
    let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
    let packet: MainPacket = file.read_le().unwrap();

    // Recovery set ID should be 16 bytes
    assert_eq!(packet.set_id.as_bytes().len(), 16);
}

#[test]
fn test_main_packet_clone() {
    let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
    let original: MainPacket = file.read_le().unwrap();

    let cloned = original.clone();

    assert_eq!(original.length, cloned.length);
    assert_eq!(original.slice_size, cloned.slice_size);
    assert_eq!(original.file_count, cloned.file_count);
    assert_eq!(original.file_ids, cloned.file_ids);
}

#[test]
fn test_main_packet_non_recovery_file_ids() {
    let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
    let packet: MainPacket = file.read_le().unwrap();

    // non_recovery_file_ids should exist (may be empty)
    assert!(packet.non_recovery_file_ids.len() <= packet.file_ids.len());
}
