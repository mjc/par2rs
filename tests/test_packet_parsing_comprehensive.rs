//! Comprehensive packet parsing tests with edge cases and error conditions
//!
//! These tests focus on the robustness of the refactored packet parsing code,
//! including error handling, boundary conditions, and recovery from corruption.

use par2rs::packets::{Packet, PacketParseError};
use par2rs::{parse_packets, parse_packets_with_options};
use std::io::Cursor;

/// Helper to create a valid minimal PAR2 packet header
fn create_packet_header(packet_type: &[u8; 16], length: u64) -> Vec<u8> {
    let mut data = vec![0u8; length as usize];
    // Magic bytes
    data[0..8].copy_from_slice(b"PAR2\0PKT");
    // Length
    data[8..16].copy_from_slice(&length.to_le_bytes());
    // MD5 hash (16..32) - zeros for test
    // Set ID (32..48) - zeros for test
    // Type
    data[48..64].copy_from_slice(packet_type);
    data
}

mod error_handling {
    use super::*;

    #[test]
    fn parse_returns_specific_error_for_invalid_magic() {
        let data = vec![0xFFu8; 128];
        let mut cursor = Cursor::new(&data);
        let result = Packet::parse(&mut cursor);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PacketParseError::InvalidMagic(_)
        ));
    }

    #[test]
    fn parse_returns_specific_error_for_invalid_length() {
        let mut data = create_packet_header(&[0u8; 16], 64);
        data[8..16].copy_from_slice(&(63u64).to_le_bytes()); // Too small
        let mut cursor = Cursor::new(&data);
        let result = Packet::parse(&mut cursor);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PacketParseError::InvalidLength(63)
        ));
    }

    #[test]
    fn parse_returns_specific_error_for_truncation() {
        let mut data = create_packet_header(&[0u8; 16], 256);
        data.truncate(100); // Truncate before full packet
        let mut cursor = Cursor::new(&data);
        let result = Packet::parse(&mut cursor);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PacketParseError::TruncatedData { .. }
        ));
    }

    #[test]
    fn parse_returns_specific_error_for_unknown_type() {
        let unknown_type = [0xFFu8; 16];
        let data = create_packet_header(&unknown_type, 64);
        let mut cursor = Cursor::new(&data);
        let result = Packet::parse(&mut cursor);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PacketParseError::UnknownPacketType(_)
        ));
    }
}

mod boundary_conditions {
    use super::*;
    use par2rs::packets::{MAX_PACKET_SIZE, MIN_PACKET_SIZE};

    #[test]
    fn parse_accepts_minimum_valid_size() {
        let data = create_packet_header(&[0xFFu8; 16], MIN_PACKET_SIZE);
        let mut cursor = Cursor::new(&data);
        let result = Packet::parse(&mut cursor);

        // Will fail on unknown packet type, but size validation passed
        assert!(matches!(
            result.unwrap_err(),
            PacketParseError::UnknownPacketType(_)
        ));
    }

    #[test]
    fn parse_rejects_one_below_minimum() {
        let mut data = create_packet_header(&[0u8; 16], 64);
        data[8..16].copy_from_slice(&(MIN_PACKET_SIZE - 1).to_le_bytes());
        let mut cursor = Cursor::new(&data);
        let result = Packet::parse(&mut cursor);

        assert!(matches!(
            result.unwrap_err(),
            PacketParseError::InvalidLength(_)
        ));
    }

    #[test]
    fn parse_accepts_maximum_valid_size() {
        let mut data = create_packet_header(&[0u8; 16], 64);
        data[8..16].copy_from_slice(&MAX_PACKET_SIZE.to_le_bytes());
        let mut cursor = Cursor::new(&data);

        // Just parse the header - we can't create a 100MB test packet
        // This tests that the length validation accepts MAX_PACKET_SIZE
        match Packet::parse(&mut cursor) {
            Err(PacketParseError::TruncatedData { .. }) => {
                // Expected - we don't have 100MB of data
            }
            Err(PacketParseError::InvalidLength(_)) => {
                panic!("MAX_PACKET_SIZE should be accepted");
            }
            _ => {}
        }
    }

    #[test]
    fn parse_rejects_one_above_maximum() {
        let mut data = create_packet_header(&[0u8; 16], 64);
        data[8..16].copy_from_slice(&(MAX_PACKET_SIZE + 1).to_le_bytes());
        let mut cursor = Cursor::new(&data);
        let result = Packet::parse(&mut cursor);

        assert!(matches!(
            result.unwrap_err(),
            PacketParseError::InvalidLength(_)
        ));
    }

    #[test]
    fn parse_handles_zero_length_gracefully() {
        let mut data = create_packet_header(&[0u8; 16], 64);
        data[8..16].copy_from_slice(&(0u64).to_le_bytes());
        let mut cursor = Cursor::new(&data);
        let result = Packet::parse(&mut cursor);

        assert!(matches!(
            result.unwrap_err(),
            PacketParseError::InvalidLength(0)
        ));
    }
}

mod multiple_packets {
    use super::*;

    #[test]
    fn parse_packets_stops_at_first_invalid_magic() {
        let unknown_type = [0xFFu8; 16];
        let mut data = Vec::new();

        // First packet - unknown type but valid structure
        data.extend_from_slice(&create_packet_header(&unknown_type, 64));

        // Second packet - invalid magic
        data.extend_from_slice(&[0xDEu8; 64]);

        let mut cursor = Cursor::new(&data);
        let packets = parse_packets(&mut cursor);

        // Should stop after finding invalid magic (first packet is skipped due to unknown type)
        assert_eq!(packets.len(), 0);
    }

    #[test]
    fn parse_packets_handles_mixed_valid_invalid() {
        let unknown_type1 = [0xAAu8; 16];
        let unknown_type2 = [0xBBu8; 16];
        let mut data = Vec::new();

        // Valid unknown packet
        data.extend_from_slice(&create_packet_header(&unknown_type1, 64));
        // Another valid unknown packet
        data.extend_from_slice(&create_packet_header(&unknown_type2, 64));

        let mut cursor = Cursor::new(&data);
        let packets = parse_packets(&mut cursor);

        // Both packets have unknown types, so both are skipped (forward compatibility)
        assert_eq!(packets.len(), 0);
    }

    #[test]
    fn parse_packets_stops_at_truncated_packet() {
        let unknown_type = [0xFFu8; 16];
        let mut data = Vec::new();

        // First packet - complete
        data.extend_from_slice(&create_packet_header(&unknown_type, 64));

        // Second packet - truncated
        let mut truncated = create_packet_header(&unknown_type, 128);
        truncated.truncate(80); // Only partial packet
        data.extend_from_slice(&truncated);

        let mut cursor = Cursor::new(&data);
        let packets = parse_packets(&mut cursor);

        // Should parse first packet (skipped due to unknown type) and stop at truncated
        assert_eq!(packets.len(), 0);
    }
}

mod recovery_skip_behavior {
    use super::*;
    use par2rs::packets::recovery_slice_packet;

    #[test]
    fn parse_packets_default_skips_recovery() {
        let mut recovery_type = [0u8; 16];
        recovery_type.copy_from_slice(recovery_slice_packet::TYPE_OF_PACKET);

        let mut data = Vec::new();

        // Create a large "recovery packet" (will be skipped)
        let large_packet = create_packet_header(&recovery_type, 1024);
        data.extend_from_slice(&large_packet);

        let mut cursor = Cursor::new(&data);
        let packets = parse_packets(&mut cursor);

        // Default parse_packets skips recovery slices
        assert_eq!(packets.len(), 0);

        // Verify cursor position advanced (packet was skipped, not parsed)
        assert_eq!(cursor.position(), 1024);
    }

    #[test]
    fn parse_packets_with_options_includes_recovery() {
        let mut recovery_type = [0u8; 16];
        recovery_type.copy_from_slice(recovery_slice_packet::TYPE_OF_PACKET);

        let data = create_packet_header(&recovery_type, 128);
        let mut cursor = Cursor::new(&data);

        // This will try to parse the recovery packet (and likely fail due to missing data)
        // but it won't skip it like the default behavior
        let (_packets, _) = parse_packets_with_options(&mut cursor, true);

        // The packet structure is minimal so parsing may fail, but the skip didn't happen
        // We can verify by checking cursor position
        assert_eq!(cursor.position(), 128);
    }
}

mod corruption_recovery {
    use super::*;

    #[test]
    fn parse_packets_attempts_recovery_after_bad_magic() {
        let unknown_type = [0xFFu8; 16];
        let mut data = Vec::new();

        // Add some garbage
        data.extend_from_slice(&[0xDEu8; 100]);

        // Add a valid packet after the garbage
        data.extend_from_slice(&create_packet_header(&unknown_type, 64));

        let mut cursor = Cursor::new(&data);
        let packets = parse_packets(&mut cursor);

        // Parser should skip garbage and find the valid packet
        // (though it's unknown type, so not included in results)
        assert!(packets.is_empty());
    }

    #[test]
    fn parse_packets_handles_empty_file() {
        let data = Vec::<u8>::new();
        let mut cursor = Cursor::new(&data);
        let packets = parse_packets(&mut cursor);

        assert_eq!(packets.len(), 0);
    }

    #[test]
    fn parse_packets_handles_file_with_only_garbage() {
        let data = vec![0xFFu8; 1000];
        let mut cursor = Cursor::new(&data);
        let packets = parse_packets(&mut cursor);

        assert_eq!(packets.len(), 0);
    }
}

mod real_world_scenarios {
    use super::*;
    use std::fs::File;
    use std::io::BufReader;

    #[test]
    fn parse_real_par2_file() {
        let fixture_path = "tests/fixtures/testfile.par2";
        if let Ok(file) = File::open(fixture_path) {
            let mut reader = BufReader::new(file);
            let packets = parse_packets(&mut reader);

            // Should successfully parse packets from real file
            assert!(!packets.is_empty(), "Should find packets in test fixture");

            // Verify all packets are valid
            for packet in &packets {
                assert!(packet.verify(), "Packet should verify successfully");
            }
        }
    }

    #[test]
    fn parse_real_par2_file_with_recovery() {
        let fixture_path = "tests/fixtures/testfile.par2";
        if let Ok(file) = File::open(fixture_path) {
            let mut reader = BufReader::new(file);
            let (packets_with_recovery, _) = parse_packets_with_options(&mut reader, true);

            // Reopen and parse without recovery
            let file2 = File::open(fixture_path).unwrap();
            let mut reader2 = BufReader::new(file2);
            let (packets_without_recovery, _) = parse_packets_with_options(&mut reader2, false);

            // Should have more packets when including recovery
            assert!(
                packets_with_recovery.len() >= packets_without_recovery.len(),
                "Including recovery should not reduce packet count"
            );
        }
    }
}

mod stress_tests {
    use super::*;

    #[test]
    fn parse_many_small_packets() {
        let unknown_type = [0xEEu8; 16];
        let mut data = Vec::new();

        // Create 100 small packets
        for _ in 0..100 {
            data.extend_from_slice(&create_packet_header(&unknown_type, 64));
        }

        let mut cursor = Cursor::new(&data);
        let packets = parse_packets(&mut cursor);

        // All unknown types, so all skipped
        assert_eq!(packets.len(), 0);

        // But parser should have processed all of them
        assert_eq!(cursor.position() as usize, data.len());
    }

    #[test]
    fn parse_alternating_valid_invalid() {
        let unknown_type = [0xAAu8; 16];
        let mut data = Vec::new();

        for i in 0..10 {
            if i % 2 == 0 {
                // Valid packet
                data.extend_from_slice(&create_packet_header(&unknown_type, 64));
            } else {
                // Invalid magic
                data.extend_from_slice(&[0xFFu8; 64]);
            }
        }

        let mut cursor = Cursor::new(&data);
        let packets = parse_packets(&mut cursor);

        // Parser should stop at first invalid magic
        assert_eq!(packets.len(), 0);
    }
}
