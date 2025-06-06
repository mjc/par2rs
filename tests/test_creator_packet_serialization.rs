use binrw::{BinReaderExt, BinWrite};
use par2rs::packets::creator_packet::CreatorPacket;
use std::fs::File;
use std::io::Cursor;

#[test]
fn test_creator_packet_serialized_length() {
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
