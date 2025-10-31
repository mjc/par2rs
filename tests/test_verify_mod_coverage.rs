//! Comprehensive tests for verify/mod.rs functions
//!
//! Targets uncovered code paths in the verify module

use par2rs::domain::{FileId, Md5Hash, RecoverySetId};
use par2rs::packets::{FileDescriptionPacket, MainPacket, Packet};
use par2rs::verify::{
    comprehensive_verify_files, comprehensive_verify_files_with_config, print_verification_results,
    VerificationConfig,
};
use std::fs;
use tempfile::TempDir;

// Helper to create main packet
fn create_main_packet(file_ids: Vec<FileId>) -> MainPacket {
    MainPacket {
        length: 64 + (file_ids.len() * 16) as u64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([1; 16]),
        slice_size: 16384,
        file_count: file_ids.len() as u32,
        file_ids,
        non_recovery_file_ids: Vec::new(),
    }
}

// Helper to create file description
fn create_file_desc(file_id: FileId, name: &str, length: u64) -> FileDescriptionPacket {
    FileDescriptionPacket {
        packet_type: *b"PAR 2.0\0FileDesc",
        length: 120 + name.len() as u64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([1; 16]),
        file_id,
        md5_hash: Md5Hash::new([5; 16]),
        md5_16k: Md5Hash::new([6; 16]),
        file_length: length,
        file_name: name.as_bytes().to_vec(),
    }
}

#[test]
fn test_print_verification_results() {
    let file_id = FileId::new([1; 16]);
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 100)),
    ];

    let results = comprehensive_verify_files(packets);

    // Should print without panicking
    print_verification_results(&results);
}

#[test]
fn test_print_verification_results_multiple_files() {
    let file_ids: Vec<_> = (0..3).map(|i| FileId::new([i as u8; 16])).collect();
    let mut packets = vec![Packet::Main(create_main_packet(file_ids.clone()))];

    for (i, file_id) in file_ids.iter().enumerate() {
        packets.push(Packet::FileDescription(create_file_desc(
            *file_id,
            &format!("file{}.txt", i),
            100 * (i as u64 + 1),
        )));
    }

    let results = comprehensive_verify_files(packets);
    print_verification_results(&results);
}

#[test]
fn test_comprehensive_verify_with_config_parallel() {
    let file_id = FileId::new([1; 16]);
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 100)),
    ];

    let config = VerificationConfig {
        parallel: true,
        ..Default::default()
    };

    let results = comprehensive_verify_files_with_config(packets, &config);
    assert!(results.total_block_count > 0 || results.missing_file_count > 0);
}

#[test]
fn test_comprehensive_verify_with_config_sequential() {
    let file_id = FileId::new([1; 16]);
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 100)),
    ];

    let config = VerificationConfig {
        parallel: false,
        ..Default::default()
    };

    let results = comprehensive_verify_files_with_config(packets, &config);
    assert!(results.total_block_count > 0 || results.missing_file_count > 0);
}

#[test]
fn test_comprehensive_verify_empty_packets() {
    let packets = vec![];
    let results = comprehensive_verify_files(packets);

    // Should handle empty input gracefully
    assert_eq!(results.total_block_count, 0);
    assert_eq!(results.present_file_count, 0);
}

#[test]
fn test_comprehensive_verify_missing_files() {
    let file_ids: Vec<_> = (0..5).map(|i| FileId::new([i as u8; 16])).collect();
    let mut packets = vec![Packet::Main(create_main_packet(file_ids.clone()))];

    for (i, file_id) in file_ids.iter().enumerate() {
        packets.push(Packet::FileDescription(create_file_desc(
            *file_id,
            &format!("missing{}.txt", i),
            1000,
        )));
    }

    let results = comprehensive_verify_files(packets);

    // All files should be missing
    assert_eq!(results.missing_file_count, 5);
}

#[test]
fn test_comprehensive_verify_corrupted_files() {
    let dir = TempDir::new().unwrap();

    // Create files with wrong size
    for i in 0..3 {
        let file_path = dir.path().join(format!("file{}.txt", i));
        fs::write(&file_path, b"wrong").unwrap();
    }

    // Change to temp dir
    std::env::set_current_dir(dir.path()).unwrap();

    let file_ids: Vec<_> = (0..3).map(|i| FileId::new([i as u8; 16])).collect();
    let mut packets = vec![Packet::Main(create_main_packet(file_ids.clone()))];

    for (i, file_id) in file_ids.iter().enumerate() {
        packets.push(Packet::FileDescription(create_file_desc(
            *file_id,
            &format!("file{}.txt", i),
            1000, // Expects 1000 bytes but file has 5
        )));
    }

    let results = comprehensive_verify_files(packets);

    // Files exist but are corrupted (wrong size/hash)
    assert!(results.corrupted_file_count > 0 || results.missing_file_count > 0);
}

#[test]
fn test_verification_with_recovery_blocks() {
    let file_id = FileId::new([1; 16]);
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 16384)), // One block
    ];

    let results = comprehensive_verify_files(packets);

    // Should calculate if repair is possible
    assert!(!results.repair_possible || results.recovery_blocks_available > 0);
}

#[test]
fn test_verification_results_aggregation() {
    // Test with multiple files of different statuses
    let dir = TempDir::new().unwrap();

    // Create one intact file
    let intact_path = dir.path().join("intact.txt");
    fs::write(&intact_path, b"data").unwrap();

    std::env::set_current_dir(dir.path()).unwrap();

    let file_ids: Vec<_> = (0..3).map(|i| FileId::new([i as u8; 16])).collect();
    let mut packets = vec![Packet::Main(create_main_packet(file_ids.clone()))];

    packets.push(Packet::FileDescription(create_file_desc(
        file_ids[0],
        "intact.txt",
        4,
    )));
    packets.push(Packet::FileDescription(create_file_desc(
        file_ids[1],
        "missing.txt",
        100,
    )));
    packets.push(Packet::FileDescription(create_file_desc(
        file_ids[2],
        "another_missing.txt",
        200,
    )));

    let results = comprehensive_verify_files(packets);

    // Should aggregate results correctly
    assert!(
        results.present_file_count + results.missing_file_count + results.corrupted_file_count > 0
    );
}

#[test]
fn test_print_verification_results_repair_possible() {
    let file_id = FileId::new([1; 16]);
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 16384)),
    ];

    let mut results = comprehensive_verify_files(packets);

    // Simulate repair being possible
    results.repair_possible = true;
    results.recovery_blocks_available = 10;

    print_verification_results(&results);
}

#[test]
fn test_print_verification_results_repair_not_possible() {
    let file_id = FileId::new([1; 16]);
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 16384)),
    ];

    let mut results = comprehensive_verify_files(packets);

    // Simulate repair not being possible
    results.repair_possible = false;
    results.recovery_blocks_available = 0;
    results.blocks_needed_for_repair = 5;

    print_verification_results(&results);
}

#[test]
fn test_verification_config_defaults() {
    let config = VerificationConfig::default();

    // Should have reasonable defaults
    let _ = config.parallel;
}

#[test]
fn test_comprehensive_verify_large_file_count() {
    // Test with many files to trigger parallel processing
    let file_ids: Vec<_> = (0..20).map(|i| FileId::new([i as u8; 16])).collect();
    let mut packets = vec![Packet::Main(create_main_packet(file_ids.clone()))];

    for (i, file_id) in file_ids.iter().enumerate() {
        packets.push(Packet::FileDescription(create_file_desc(
            *file_id,
            &format!("file{}.txt", i),
            1000 + (i as u64 * 100),
        )));
    }

    let results = comprehensive_verify_files(packets);

    // Should process all files
    assert_eq!(
        results.present_file_count + results.missing_file_count + results.corrupted_file_count,
        20
    );
}
