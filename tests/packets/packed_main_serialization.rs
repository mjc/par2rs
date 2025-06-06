use binrw::{BinReaderExt, BinWrite};
use par2rs::packets::packed_main_packet::PackedMainPacket;
use std::fs::File;
use std::io::Cursor;

// TODO: get test data for this

#[test]
#[ignore]
fn test_packed_main_packet_serialized_length() {
    // Open the test fixture file
    let mut file = File::open("tests/fixtures/packets/PackedMainPacket.par2").unwrap();

    // Read the PackedMainPacket from the file
    let packed_main_packet: PackedMainPacket = file.read_le().unwrap();

    // Serialize the packet into a buffer
    let mut buffer = Cursor::new(Vec::new());
    packed_main_packet.write_le(&mut buffer).unwrap();

    // Verify that the serialized length matches the packet's length field
    let serialized_length = buffer.get_ref().len() as u64;
    assert_eq!(
        serialized_length, packed_main_packet.length,
        "Serialized length mismatch: expected {}, got {}",
        packed_main_packet.length, serialized_length
    );
}
