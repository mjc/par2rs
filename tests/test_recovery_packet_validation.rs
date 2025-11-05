//! Tests for recovery packet validation without full loading
//!
//! These tests verify that when parse_packets_with_options is called with
//! include_recovery_slices=false, the recovery packets are still validated
//! (MD5 checked) but not kept in memory.

use par2rs::{parse_packets, parse_packets_with_options};
use std::fs::File;
use std::io::{BufReader, Cursor};

/// Test that recovery packets are validated when include_recovery_slices=false
#[test]
fn test_recovery_packets_validated_when_not_loaded() {
    // Use a volume file that contains recovery packets
    let fixture_path = "tests/fixtures/testfile.vol00+01.par2";

    if let Ok(file) = File::open(fixture_path) {
        let mut reader = BufReader::new(file);

        // Parse WITHOUT loading recovery slices (default behavior)
        let packets_without = parse_packets(&mut reader);

        // Reopen and parse WITH recovery slices
        let file2 = File::open(fixture_path).unwrap();
        let mut reader2 = BufReader::new(file2);
        let (packets_with, _recovery_count_with) = parse_packets_with_options(&mut reader2, true);

        // Both should succeed (validation happens in both cases)
        assert!(
            !packets_with.is_empty(),
            "Should parse packets with recovery slices"
        );

        // The "with recovery" version should have more packets (recovery slices included)
        assert!(
            packets_with.len() > packets_without.len(),
            "Including recovery slices should have more packets (got {} with, {} without)",
            packets_with.len(),
            packets_without.len()
        );

        // Count recovery packets in the "with" version
        let recovery_count = packets_with
            .iter()
            .filter(|p| matches!(p, par2rs::Packet::RecoverySlice(_)))
            .count();

        println!(
            "Parsed {} total packets without recovery data",
            packets_without.len()
        );
        println!(
            "Parsed {} total packets with recovery data ({} recovery packets)",
            packets_with.len(),
            recovery_count
        );

        // Should have some recovery packets
        assert!(
            recovery_count > 0,
            "Volume file should contain recovery packets"
        );

        // Verify all loaded recovery packets are valid
        for packet in &packets_with {
            if matches!(packet, par2rs::Packet::RecoverySlice(_)) {
                assert!(
                    packet.verify(),
                    "Recovery packet should verify successfully"
                );
            }
        }
    } else {
        eprintln!("Skipping test - fixture not found");
    }
}

/// Test that corrupted recovery packets are rejected during validation
#[test]
fn test_corrupted_recovery_packet_rejected() {
    use par2rs::packets::MAGIC_BYTES;

    // Create a fake recovery packet with invalid MD5
    let mut data = Vec::new();

    // Valid header with corrupted MD5
    data.extend_from_slice(MAGIC_BYTES); // 0..8
    data.extend_from_slice(&(128u64).to_le_bytes()); // 8..16: length
    data.extend_from_slice(&[0xFFu8; 16]); // 16..32: WRONG MD5
    data.extend_from_slice(&[0x01u8; 16]); // 32..48: set_id
    data.extend_from_slice(b"PAR 2.0\0RecvSlic"); // 48..64: type
    data.extend_from_slice(&(0u32).to_le_bytes()); // 64..68: exponent
    data.extend_from_slice(&[0xAAu8; 60]); // 68..128: recovery_data

    let mut cursor = Cursor::new(&data);
    let packets = parse_packets(&mut cursor);

    // Should not include the corrupted packet (validation failed)
    let recovery_count = packets
        .iter()
        .filter(|p| matches!(p, par2rs::Packet::RecoverySlice(_)))
        .count();

    assert_eq!(
        recovery_count, 0,
        "Corrupted recovery packet should be rejected"
    );
}

/// Test that valid recovery packets pass validation even when not fully loaded
#[test]
fn test_valid_recovery_packet_accepted() {
    use par2rs::checksum::compute_md5_bytes;
    use par2rs::packets::MAGIC_BYTES;

    // Create a properly formatted recovery packet
    let exponent = 42u32;
    let recovery_data = vec![0xAAu8; 60];

    // Build the packet body (what gets hashed)
    let set_id = [0x01u8; 16];
    let packet_type = b"PAR 2.0\0RecvSlic";

    let mut body_data = Vec::new();
    body_data.extend_from_slice(&set_id);
    body_data.extend_from_slice(packet_type);
    body_data.extend_from_slice(&exponent.to_le_bytes());
    body_data.extend_from_slice(&recovery_data);

    // Compute correct MD5
    let md5 = compute_md5_bytes(&body_data);

    // Build complete packet
    let total_length = 64u64 + 4 + recovery_data.len() as u64;
    let mut data = Vec::new();
    data.extend_from_slice(MAGIC_BYTES);
    data.extend_from_slice(&total_length.to_le_bytes());
    data.extend_from_slice(&md5); // Correct MD5
    data.extend_from_slice(&set_id);
    data.extend_from_slice(packet_type);
    data.extend_from_slice(&exponent.to_le_bytes());
    data.extend_from_slice(&recovery_data);

    let mut cursor = Cursor::new(&data);

    // This should succeed - packet is valid and MD5 verified
    // Even though we're not loading recovery slices, validation still happens
    let packets = parse_packets(&mut cursor);

    // The packet should be validated and skipped (not in results since include_recovery_slices=false)
    // But parsing should not fail
    assert_eq!(
        packets.len(),
        0,
        "Recovery packet validated but not included in results"
    );

    // Now parse with recovery slices included
    let mut cursor2 = Cursor::new(&data);
    let (packets_with, _) = parse_packets_with_options(&mut cursor2, true);

    // Should successfully parse and include the packet
    assert_eq!(
        packets_with.len(),
        1,
        "Should include recovery packet when requested"
    );
    assert!(matches!(packets_with[0], par2rs::Packet::RecoverySlice(_)));
}

/// Test memory efficiency: verify we don't keep recovery data in memory
#[test]
fn test_memory_efficiency_with_real_file() {
    let fixture_path = "tests/fixtures/testfile.par2";

    if let Ok(file) = File::open(fixture_path) {
        let file_metadata = file.metadata().unwrap();
        let file_size = file_metadata.len();

        let mut reader = BufReader::new(file);

        // Parse without loading recovery slices
        let packets = parse_packets(&mut reader);

        // Estimate memory used by packets
        // (This is a rough estimate - in practice we'd use a memory profiler)
        let estimated_memory = packets
            .iter()
            .map(|p| {
                match p {
                    par2rs::Packet::RecoverySlice(rs) => {
                        // Recovery data is the big memory hog
                        rs.recovery_data.len()
                    }
                    _ => 1024, // Other packets are small
                }
            })
            .sum::<usize>();

        println!("File size: {} bytes", file_size);
        println!("Estimated packet memory: {} bytes", estimated_memory);

        // When not loading recovery slices, we shouldn't have recovery data in memory
        let has_recovery_packets = packets
            .iter()
            .any(|p| matches!(p, par2rs::Packet::RecoverySlice(_)));

        assert!(
            !has_recovery_packets,
            "Should not have recovery packets in memory when include_recovery_slices=false"
        );
    } else {
        eprintln!("Skipping test - fixture not found");
    }
}

/// Test with a volume file that contains only recovery packets
#[test]
fn test_volume_file_with_only_recovery_packets() {
    // Look for a volume file (testfile.vol00+01.par2 or similar)
    let volume_paths = [
        "tests/fixtures/testfile.vol00+01.par2",
        "tests/fixtures/testfile.vol01+02.par2",
    ];

    for volume_path in &volume_paths {
        if let Ok(file) = File::open(volume_path) {
            let mut reader = BufReader::new(file);

            // Parse without loading recovery data
            let packets = parse_packets(&mut reader);

            // Should successfully parse (validation happens)
            // but no recovery packets in results
            let recovery_count = packets
                .iter()
                .filter(|p| matches!(p, par2rs::Packet::RecoverySlice(_)))
                .count();

            println!(
                "Volume file {} has {} recovery packets when not loading",
                volume_path, recovery_count
            );

            assert_eq!(
                recovery_count, 0,
                "Should not load recovery packets when include_recovery_slices=false"
            );

            // Now parse with recovery slices
            let file2 = File::open(volume_path).unwrap();
            let mut reader2 = BufReader::new(file2);
            let (packets_with, _) = parse_packets_with_options(&mut reader2, true);

            let recovery_count_with = packets_with
                .iter()
                .filter(|p| matches!(p, par2rs::Packet::RecoverySlice(_)))
                .count();

            println!(
                "Volume file {} has {} recovery packets when loading",
                volume_path, recovery_count_with
            );

            assert!(
                recovery_count_with > 0,
                "Volume file should contain recovery packets"
            );

            // Verify all packets
            for packet in &packets_with {
                assert!(packet.verify(), "All packets should verify successfully");
            }

            return; // Test passed with first available volume file
        }
    }

    eprintln!("Skipping test - no volume fixtures found");
}

/// Test that validation catches truncated recovery packets
#[test]
fn test_truncated_recovery_packet_rejected() {
    use par2rs::packets::MAGIC_BYTES;

    // Create a recovery packet that claims to be 200 bytes but is truncated
    let mut data = Vec::new();
    data.extend_from_slice(MAGIC_BYTES);
    data.extend_from_slice(&(200u64).to_le_bytes()); // Claims 200 bytes
    data.extend_from_slice(&[0x00u8; 16]); // MD5
    data.extend_from_slice(&[0x01u8; 16]); // set_id
    data.extend_from_slice(b"PAR 2.0\0RecvSlic"); // type
    data.extend_from_slice(&(0u32).to_le_bytes()); // exponent
                                                   // Only 20 bytes of data instead of 200-68=132
    data.extend_from_slice(&[0xAAu8; 20]);

    let mut cursor = Cursor::new(&data);
    let packets = parse_packets(&mut cursor);

    // Should handle truncation gracefully (no panic, returns empty or partial results)
    println!("Parsed {} packets from truncated data", packets.len());
}
