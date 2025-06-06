use binrw::{BinReaderExt, BinWrite};
use par2rs::packets::file_description_packet::FileDescriptionPacket;
use std::fs::File;
use std::io::Cursor;

#[test]
fn test_file_description_packet_serialized_length() {
    // Open the test fixture file
    let mut file = File::open("tests/fixtures/packets/FileDescriptionPacket.par2").unwrap();

    // Read the FileDescriptionPacket from the file
    let file_description_packet: FileDescriptionPacket = file.read_le().unwrap();

    // Serialize the packet into a buffer
    let mut buffer = Cursor::new(Vec::new());
    file_description_packet.write_le(&mut buffer).unwrap();

    // Verify that the serialized length plus magic (8 bytes) matches the packet's length field
    let serialized_length = buffer.get_ref().len() as u64;
    assert_eq!(
        serialized_length + 8,
        file_description_packet.length,
        "Serialized length mismatch: expected {}, got {} (serialized length {} + magic 8 bytes)",
        file_description_packet.length,
        serialized_length + 8,
        serialized_length
    );
}
