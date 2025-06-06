use binrw::{BinReaderExt, BinWrite};
use par2rs::packets::input_file_slice_checksum_packet::InputFileSliceChecksumPacket;
use std::fs::File;
use std::io::Cursor;

#[test]
fn test_input_file_slice_checksum_packet_serialized_length() {
    // Open the test fixture file
    let mut file = File::open("tests/fixtures/packets/InputFileSliceChecksumPacket.par2").unwrap();

    // Read the InputFileSliceChecksumPacket from the file
    let input_file_slice_checksum_packet: InputFileSliceChecksumPacket = file.read_le().unwrap();

    // Serialize the packet into a buffer
    let mut buffer = Cursor::new(Vec::new());
    input_file_slice_checksum_packet
        .write_le(&mut buffer)
        .unwrap();

    // Verify that the serialized length matches the packet's length field
    let serialized_length = buffer.get_ref().len() as u64;
    assert_eq!(
        serialized_length, input_file_slice_checksum_packet.length,
        "Serialized length mismatch: expected {}, got {}",
        input_file_slice_checksum_packet.length, serialized_length
    );
}
