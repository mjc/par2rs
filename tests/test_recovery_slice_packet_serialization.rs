use binrw::{BinReaderExt, BinWrite};
use par2rs::packets::recovery_slice_packet::RecoverySlicePacket;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};

#[test]
fn test_recovery_slice_packet_serialized_length() {
    // Open the test fixture file
    let mut file = File::open("tests/fixtures/packets/RecoverySlicePacket.par2").unwrap();

    // Read the original file bytes
    let mut original_bytes = Vec::new();
    file.read_to_end(&mut original_bytes).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();

    // Read the RecoverySlicePacket from the file
    let recovery_slice_packet: RecoverySlicePacket = file.read_le().unwrap();

    // Serialize the packet into a buffer
    let mut buffer = Cursor::new(Vec::new());
    recovery_slice_packet.write_le(&mut buffer).unwrap();
    let serialized_bytes = buffer.get_ref();

    // Print debug information
    println!("Original file length: {}", original_bytes.len());
    println!("Packet length field: {}", recovery_slice_packet.length);
    println!("Serialized length: {}", serialized_bytes.len());
    println!("Recovery data length: {}", recovery_slice_packet.recovery_data.len());

    // Compare byte by byte
    let min_len = std::cmp::min(original_bytes.len(), serialized_bytes.len());
    let mut differences = Vec::new();
    
    for i in 0..min_len {
        if original_bytes[i] != serialized_bytes[i] {
            differences.push((i, original_bytes[i], serialized_bytes[i]));
        }
    }

    if !differences.is_empty() {
        println!("Found {} byte differences:", differences.len());
        for (i, orig, ser) in differences.iter().take(10) {
            println!("  Offset {}: original=0x{:02x}, serialized=0x{:02x}", i, orig, ser);
        }
        if differences.len() > 10 {
            println!("  ... and {} more differences", differences.len() - 10);
        }
    }

    if original_bytes.len() != serialized_bytes.len() {
        println!("Length difference: original={}, serialized={}", original_bytes.len(), serialized_bytes.len());
        if original_bytes.len() > serialized_bytes.len() {
            let missing_bytes = &original_bytes[serialized_bytes.len()..];
            println!("Missing bytes in serialized (first 32): {:?}", &missing_bytes[..std::cmp::min(32, missing_bytes.len())]);
        } else {
            let extra_bytes = &serialized_bytes[original_bytes.len()..];
            println!("Extra bytes in serialized (first 32): {:?}", &extra_bytes[..std::cmp::min(32, extra_bytes.len())]);
        }
    }

    // Verify that the serialized length matches the packet's length field
    let serialized_length = serialized_bytes.len() as u64;
    assert_eq!(
        serialized_length, recovery_slice_packet.length,
        "Serialized length mismatch: expected {}, got {}",
        recovery_slice_packet.length, serialized_length
    );
}
