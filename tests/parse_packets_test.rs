use std::fs::File;
use std::path::Path;
use par2rs::{parse_packets, Packet};

#[test]
fn test_parse_packets() {
    let test_file_path = Path::new("test/fixtures/testfile.par2");
    assert!(test_file_path.exists(), "Test file does not exist");

    let mut file = File::open(test_file_path).expect("Failed to open test file");
    let packets = parse_packets(&mut file);

    assert!(!packets.is_empty(), "No packets were parsed from the file");

    // Check for specific packet types
    let main_packet_found = packets.iter().any(|p| matches!(p, Packet::MainPacket(_)));
    assert!(main_packet_found, "MainPacket not found in parsed packets");

    let creator_packet_found = packets.iter().any(|p| matches!(p, Packet::CreatorPacket(_)));
    assert!(creator_packet_found, "CreatorPacket not found in parsed packets");

    println!("Parsed packets successfully: {:?}", packets);
}
