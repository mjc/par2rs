use par2rs::{parse_packets, Packet};
use std::fs::File;
use std::path::Path;

fn assert_packet_type(
    packets: &[Packet],
    matcher: fn(&Packet) -> bool,
    error_message: &'static str,
) {
    assert!(packets.iter().any(matcher), "{}", error_message);
}

#[test]
fn test_parse_packets() {
    let test_file_path = Path::new("test/fixtures/testfile.par2");
    assert!(test_file_path.exists(), "Test file does not exist");

    let mut file = File::open(test_file_path).expect("Failed to open test file");
    let packets = parse_packets(&mut file);

    assert!(!packets.is_empty(), "No packets were parsed from the file");

    assert_packet_type(
        &packets,
        |p| matches!(p, Packet::MainPacket(_)),
        "MainPacket not found in parsed packets",
    );
    assert_packet_type(
        &packets,
        |p| matches!(p, Packet::CreatorPacket(_)),
        "CreatorPacket not found in parsed packets",
    );

    println!("Parsed packets successfully: {:?}", packets);
}
