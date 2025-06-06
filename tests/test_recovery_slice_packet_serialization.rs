use binrw::{BinReaderExt, BinWrite};
use par2rs::packets::recovery_slice_packet::RecoverySlicePacket;
use std::fs::File;
use std::io::Cursor;

#[test]
fn test_recovery_slice_packet_serialized_length() {
    // Open the test fixture file
    let mut file = File::open("tests/fixtures/packets/RecoverySlicePacket.par2").unwrap();

    // Read the RecoverySlicePacket from the file
    let recovery_slice_packet: RecoverySlicePacket = file.read_le().unwrap();

    // Serialize the packet into a buffer
    let mut buffer = Cursor::new(Vec::new());
    recovery_slice_packet.write_le(&mut buffer).unwrap();

    // Verify that the serialized length matches the packet's length field
    let serialized_length = buffer.get_ref().len() as u64;
    assert_eq!(
        serialized_length, recovery_slice_packet.length,
        "Serialized length mismatch: expected {}, got {}",
        recovery_slice_packet.length, serialized_length
    );
}
