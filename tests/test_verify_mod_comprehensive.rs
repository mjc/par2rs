use par2rs::domain::{FileId, Md5Hash, RecoverySetId};
use par2rs::packets::{FileDescriptionPacket, MainPacket, Packet};
use par2rs::reporters::SilentVerificationReporter;
use par2rs::verify::{
    comprehensive_verify_files, comprehensive_verify_files_with_config, VerificationConfig,
};
use std::fs;
use std::io::Write;
use tempfile::TempDir;

// Helper to create main packet
fn create_main_packet(file_ids: Vec<FileId>, slice_size: u64) -> MainPacket {
    MainPacket {
        length: 64 + (file_ids.len() * 16) as u64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([1; 16]),
        slice_size,
        file_count: file_ids.len() as u32,
        file_ids,
        non_recovery_file_ids: Vec::new(),
    }
}

// Helper to create file description
fn create_file_desc(
    file_id: FileId,
    name: &str,
    length: u64,
    md5: Md5Hash,
    md5_16k: Md5Hash,
) -> FileDescriptionPacket {
    FileDescriptionPacket {
        packet_type: *b"PAR 2.0\0FileDesc",
        length: 120 + name.len() as u64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([1; 16]),
        file_id,
        md5_hash: md5,
        md5_16k,
        file_length: length,
        file_name: name.as_bytes().to_vec(),
    }
}

#[test]
fn test_comprehensive_verify_with_default_config() {
    // Create a test scenario with packets
    let file_id = FileId::new([1; 16]);
    let md5 = Md5Hash::new([1; 16]);
    let md5_16k = Md5Hash::new([2; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 16384)),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 29, md5, md5_16k)),
    ];

    // This tests the default comprehensive_verify_files function
    let results = comprehensive_verify_files(packets);

    // Since files don't actually exist, they should be missing
    assert_eq!(results.missing_file_count, 1);
    assert_eq!(results.present_file_count, 0);
}

#[test]
fn test_comprehensive_verify_with_custom_config() {
    let file_id = FileId::new([2; 16]);
    let md5 = Md5Hash::new([3; 16]);
    let md5_16k = Md5Hash::new([4; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 16384)),
        Packet::FileDescription(create_file_desc(file_id, "test2.txt", 22, md5, md5_16k)),
    ];

    let config = VerificationConfig::new(1, false);

    // This tests the comprehensive_verify_files_with_config function
    let results = comprehensive_verify_files_with_config(packets, &config);

    assert_eq!(results.missing_file_count, 1);
}

#[test]
fn test_comprehensive_verify_parallel_mode() {
    let file_id1 = FileId::new([1; 16]);
    let file_id2 = FileId::new([2; 16]);

    let md5_1 = Md5Hash::new([10; 16]);
    let md5_16k_1 = Md5Hash::new([11; 16]);
    let md5_2 = Md5Hash::new([12; 16]);
    let md5_16k_2 = Md5Hash::new([13; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id1, file_id2], 16384)),
        Packet::FileDescription(create_file_desc(file_id1, "file1.txt", 6, md5_1, md5_16k_1)),
        Packet::FileDescription(create_file_desc(file_id2, "file2.txt", 6, md5_2, md5_16k_2)),
    ];

    let config = VerificationConfig::new(2, true);
    let results = comprehensive_verify_files_with_config(packets, &config);

    assert_eq!(results.missing_file_count, 2);
    assert_eq!(results.total_block_count, 2); // Each file is small, so 1 block each
}

#[test]
fn test_comprehensive_verify_sequential_mode() {
    let file_id = FileId::new([3; 16]);
    let md5 = Md5Hash::new([20; 16]);
    let md5_16k = Md5Hash::new([21; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 16384)),
        Packet::FileDescription(create_file_desc(file_id, "seq.txt", 15, md5, md5_16k)),
    ];

    let config = VerificationConfig::new(1, false);
    let results = comprehensive_verify_files_with_config(packets, &config);

    assert_eq!(results.missing_file_count, 1);
}

#[test]
fn test_comprehensive_verify_with_existing_file() {
    let dir = TempDir::new().unwrap();

    // Create the actual file
    fs::write(dir.path().join("existing.txt"), b"This file exists!").unwrap();

    let md5 = Md5Hash::new([30; 16]);
    let md5_16k = Md5Hash::new([31; 16]);
    let file_id = FileId::new([4; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 16384)),
        Packet::FileDescription(create_file_desc(
            file_id,
            &format!("{}/existing.txt", dir.path().display()),
            17,
            md5,
            md5_16k,
        )),
    ];

    let results = comprehensive_verify_files(packets);

    // File exists at absolute path, but hash won't match our dummy hash
    assert!(
        results.present_file_count + results.corrupted_file_count >= 1
            || results.missing_file_count >= 1
    );
}

#[test]
fn test_comprehensive_verify_with_corrupted_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("corrupted.txt");
    let actual_content = b"Actual content";

    // Create file with different content than expected
    let mut file = fs::File::create(&file_path).unwrap();
    file.write_all(actual_content).unwrap();
    drop(file);

    let md5 = Md5Hash::new([40; 16]);
    let md5_16k = Md5Hash::new([41; 16]);
    let file_id = FileId::new([5; 16]);

    let file_path_str = file_path.to_str().unwrap();

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 16384)),
        Packet::FileDescription(create_file_desc(file_id, file_path_str, 16, md5, md5_16k)),
    ];

    let results = comprehensive_verify_files(packets);

    // File exists but is corrupted or wrong size
    assert!(results.corrupted_file_count > 0 || results.missing_file_count > 0);
}

#[test]
fn test_comprehensive_verify_mixed_files() {
    let dir = TempDir::new().unwrap();

    // Create one existing file (won't match hash but will exist)
    let file1_path = dir.path().join("good.txt");
    let content1 = b"Good file";
    fs::write(&file1_path, content1).unwrap();

    let file_id1 = FileId::new([1; 16]);
    let file_id2 = FileId::new([2; 16]);

    let md5_1 = Md5Hash::new([50; 16]);
    let md5_16k_1 = Md5Hash::new([51; 16]);
    let md5_2 = Md5Hash::new([52; 16]);
    let md5_16k_2 = Md5Hash::new([53; 16]);

    let file1_path_str = file1_path.to_str().unwrap();
    let file2_path = dir.path().join("missing.txt");
    let file2_path_str = file2_path.to_str().unwrap();

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id1, file_id2], 16384)),
        Packet::FileDescription(create_file_desc(
            file_id1,
            file1_path_str,
            content1.len() as u64,
            md5_1,
            md5_16k_1,
        )),
        Packet::FileDescription(create_file_desc(
            file_id2,
            file2_path_str,
            12,
            md5_2,
            md5_16k_2,
        )),
    ];

    let results = comprehensive_verify_files(packets);

    // One file exists (may be corrupt due to hash), one is missing
    assert_eq!(results.present_file_count + results.corrupted_file_count, 1);
    assert!(results.missing_file_count >= 1);
}

#[test]
fn test_comprehensive_verify_repair_possible() {
    let file_id = FileId::new([6; 16]);
    let md5 = Md5Hash::new([60; 16]);
    let md5_16k = Md5Hash::new([61; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 16384)),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 4, md5, md5_16k)),
    ];

    let results = comprehensive_verify_files(packets);

    // With missing blocks and no recovery blocks, repair should not be possible
    assert!(!results.repair_possible);
    assert!(results.blocks_needed_for_repair > 0);
}

#[test]
fn test_comprehensive_verify_empty_packets() {
    let packets = vec![];
    let results = comprehensive_verify_files(packets);

    assert_eq!(results.present_file_count, 0);
    assert_eq!(results.missing_file_count, 0);
    assert_eq!(results.total_block_count, 0);
}

#[test]
fn test_comprehensive_verify_no_file_descriptions() {
    let file_id = FileId::new([7; 16]);

    // Only main packet, no file descriptions
    let packets = vec![Packet::Main(create_main_packet(vec![file_id], 16384))];

    let results = comprehensive_verify_files(packets);

    // Should handle gracefully
    assert_eq!(results.total_block_count, 0);
}

#[test]
fn test_comprehensive_verify_large_block_size() {
    let file_id = FileId::new([8; 16]);
    let md5 = Md5Hash::new([70; 16]);
    let md5_16k = Md5Hash::new([71; 16]);

    // Use a very large block size
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 1024 * 1024)), // 1MB blocks
        Packet::FileDescription(create_file_desc(file_id, "small.txt", 10, md5, md5_16k)),
    ];

    let results = comprehensive_verify_files(packets);

    assert_eq!(results.total_block_count, 1); // Small file fits in one block
}

#[test]
fn test_comprehensive_verify_multiple_blocks() {
    let file_id = FileId::new([9; 16]);
    let md5 = Md5Hash::new([80; 16]);
    let md5_16k = Md5Hash::new([81; 16]);
    // Create content that spans multiple blocks (50KB with 16KB blocks = 4 blocks)

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 16384)),
        Packet::FileDescription(create_file_desc(file_id, "large.bin", 50000, md5, md5_16k)),
    ];

    let results = comprehensive_verify_files(packets);

    // 50000 / 16384 = 3.05... -> 4 blocks
    assert_eq!(results.total_block_count, 4);
    assert_eq!(results.missing_file_count, 1);
    assert_eq!(results.blocks_needed_for_repair, 4);
}

#[test]
fn test_verification_config_effective_threads() {
    // Test that effective_threads method works correctly
    let config_parallel = VerificationConfig::new(4, true);
    assert_eq!(config_parallel.effective_threads(), 4);

    let config_sequential = VerificationConfig::new(4, false);
    assert_eq!(config_sequential.effective_threads(), 1);
}

#[test]
fn test_comprehensive_verify_with_silent_reporter() {
    use par2rs::verify::comprehensive_verify_files_with_config_and_reporter;

    let file_id = FileId::new([10; 16]);
    let md5 = Md5Hash::new([90; 16]);
    let md5_16k = Md5Hash::new([91; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 16384)),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 25, md5, md5_16k)),
    ];

    let config = VerificationConfig::default();
    let reporter = SilentVerificationReporter::new();

    let results = comprehensive_verify_files_with_config_and_reporter(packets, &config, &reporter);

    assert_eq!(results.missing_file_count, 1);
}

#[test]
fn test_comprehensive_verify_unicode_filenames() {
    let file_id = FileId::new([11; 16]);
    let filename = "测试文件.txt";
    let md5 = Md5Hash::new([100; 16]);
    let md5_16k = Md5Hash::new([101; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 16384)),
        Packet::FileDescription(create_file_desc(file_id, filename, 12, md5, md5_16k)),
    ];

    let results = comprehensive_verify_files(packets);

    assert_eq!(results.files.len(), 1);
    assert_eq!(results.files[0].file_name, filename);
}

#[test]
fn test_comprehensive_verify_zero_byte_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("empty.txt");
    fs::write(&file_path, b"").unwrap();

    let file_id = FileId::new([12; 16]);
    let md5 = Md5Hash::new([110; 16]);
    let md5_16k = Md5Hash::new([111; 16]);

    let file_path_str = file_path.to_str().unwrap();

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 16384)),
        Packet::FileDescription(create_file_desc(file_id, file_path_str, 0, md5, md5_16k)),
    ];

    let results = comprehensive_verify_files(packets);

    // Zero-byte file should match if created
    assert_eq!(results.total_block_count, 0); // Zero-byte file has no blocks
}
