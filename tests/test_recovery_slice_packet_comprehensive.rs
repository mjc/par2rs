use binrw::{BinReaderExt, BinWrite};
use par2rs::packets::RecoverySlicePacket;
use std::fs::File;
use std::io::Cursor;

#[test]
fn test_recovery_slice_packet_verify_valid() {
    let mut file = File::open("tests/fixtures/packets/RecoverySlicePacket.par2").unwrap();
    let packet: RecoverySlicePacket = file.read_le().unwrap();
    assert!(packet.verify());
}

#[test]
fn test_recovery_slice_packet_binwrite() {
    let mut file = File::open("tests/fixtures/packets/RecoverySlicePacket.par2").unwrap();
    let packet: RecoverySlicePacket = file.read_le().unwrap();

    let mut buffer = Cursor::new(Vec::new());
    packet.write_le(&mut buffer).unwrap();

    assert_eq!(buffer.get_ref().len() as u64, packet.length);
}

#[test]
fn test_recovery_slice_packet_roundtrip() {
    let mut file = File::open("tests/fixtures/packets/RecoverySlicePacket.par2").unwrap();
    let original: RecoverySlicePacket = file.read_le().unwrap();

    // Write to buffer
    let mut write_buffer = Cursor::new(Vec::new());
    original.write_le(&mut write_buffer).unwrap();

    // Read back
    let mut read_buffer = Cursor::new(write_buffer.into_inner());
    let roundtrip: RecoverySlicePacket = read_buffer.read_le().unwrap();

    assert_eq!(original.length, roundtrip.length);
    assert_eq!(original.md5.as_bytes(), roundtrip.md5.as_bytes());
    assert_eq!(original.set_id.as_bytes(), roundtrip.set_id.as_bytes());
    assert_eq!(original.exponent, roundtrip.exponent);
}

#[test]
fn test_recovery_slice_packet_type_constant() {
    let expected = b"PAR 2.0\0RecvSlic";
    assert_eq!(
        par2rs::packets::recovery_slice_packet::TYPE_OF_PACKET,
        expected
    );
    assert_eq!(
        par2rs::packets::recovery_slice_packet::TYPE_OF_PACKET.len(),
        16
    );
}

#[test]
fn test_recovery_slice_packet_minimum_length() {
    let mut file = File::open("tests/fixtures/packets/RecoverySlicePacket.par2").unwrap();
    let packet: RecoverySlicePacket = file.read_le().unwrap();

    // Minimum packet length is 64 bytes (header fields) + recovery data
    assert!(packet.length >= 64);
}

#[test]
fn test_recovery_slice_packet_recovery_data_not_empty() {
    let mut file = File::open("tests/fixtures/packets/RecoverySlicePacket.par2").unwrap();
    let packet: RecoverySlicePacket = file.read_le().unwrap();

    // Recovery data should not be empty
    assert!(!packet.recovery_data.is_empty());
}

#[test]
fn test_recovery_slice_packet_md5_hash_format() {
    let mut file = File::open("tests/fixtures/packets/RecoverySlicePacket.par2").unwrap();
    let packet: RecoverySlicePacket = file.read_le().unwrap();

    // MD5 hash should be 16 bytes
    assert_eq!(packet.md5.as_bytes().len(), 16);
}

#[test]
fn test_recovery_slice_packet_set_id_format() {
    let mut file = File::open("tests/fixtures/packets/RecoverySlicePacket.par2").unwrap();
    let packet: RecoverySlicePacket = file.read_le().unwrap();

    // Recovery set ID should be 16 bytes
    assert_eq!(packet.set_id.as_bytes().len(), 16);
}

#[test]
fn test_recovery_slice_packet_exponent() {
    let mut file = File::open("tests/fixtures/packets/RecoverySlicePacket.par2").unwrap();
    let packet: RecoverySlicePacket = file.read_le().unwrap();

    // Exponent should exist and be valid (u32 type)
    let _ = packet.exponent;
}

#[test]
fn test_recovery_slice_packet_data_length_matches() {
    let mut file = File::open("tests/fixtures/packets/RecoverySlicePacket.par2").unwrap();
    let packet: RecoverySlicePacket = file.read_le().unwrap();

    // Recovery data length should match expected size from packet length
    let expected_data_len = packet.length - 68; // Header is 68 bytes
    assert_eq!(packet.recovery_data.len() as u64, expected_data_len);
}
