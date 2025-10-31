//! Comprehensive tests for analysis module
//!
//! Tests for PAR2 analysis functions including statistics calculation and metadata extraction.

use par2rs::analysis;
use par2rs::domain::{FileId, Md5Hash, RecoverySetId};
use par2rs::packets::processing;
use par2rs::packets::{FileDescriptionPacket, MainPacket};
use par2rs::Packet;

fn create_main_packet(slice_size: u64) -> Packet {
    Packet::Main(MainPacket {
        length: 72,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
        slice_size,
        file_count: 1,
        file_ids: vec![],
        non_recovery_file_ids: vec![],
    })
}

fn create_file_description(file_id: FileId, file_name: &str, file_length: u64) -> Packet {
    let mut name_bytes = file_name.as_bytes().to_vec();
    name_bytes.resize(256, 0);

    Packet::FileDescription(FileDescriptionPacket {
        length: 120 + name_bytes.len() as u64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id,
        md5_hash: Md5Hash::new([0; 16]),
        md5_16k: Md5Hash::new([0; 16]),
        file_length,
        file_name: name_bytes,
    })
}

// ============================================================================
// extract_filenames Tests
// ============================================================================

#[test]
fn test_extract_unique_filenames_empty() {
    let packets = vec![];
    assert_eq!(processing::extract_filenames(&packets).len(), 0);
}

#[test]
fn test_extract_unique_filenames_single() {
    let packets = vec![create_file_description(
        FileId::new([1; 16]),
        "file.txt",
        1024,
    )];
    let names = processing::extract_filenames(&packets);
    assert_eq!(names.len(), 1);
    assert!(names.contains(&"file.txt".to_string()));
}

#[test]
fn test_extract_unique_filenames_multiple_different() {
    let packets = vec![
        create_file_description(FileId::new([1; 16]), "file1.txt", 1024),
        create_file_description(FileId::new([2; 16]), "file2.bin", 2048),
        create_file_description(FileId::new([3; 16]), "file3.dat", 4096),
    ];
    let names = processing::extract_filenames(&packets);
    assert_eq!(names.len(), 3);
}

#[test]
fn test_extract_unique_filenames_duplicates() {
    let packets = vec![
        create_file_description(FileId::new([1; 16]), "duplicate.txt", 1024),
        create_file_description(FileId::new([2; 16]), "duplicate.txt", 1024),
    ];
    let names = processing::extract_filenames(&packets);
    // Should deduplicate by filename, even with different file IDs
    assert_eq!(names.len(), 1);
}

#[test]
fn test_extract_unique_filenames_with_null_termination() {
    let packets = vec![create_file_description(
        FileId::new([1; 16]),
        "test\0\0\0",
        1024,
    )];
    let names = processing::extract_filenames(&packets);
    assert!(names.contains(&"test".to_string()));
}

#[test]
fn test_extract_unique_filenames_filters_non_file_packets() {
    let packets = vec![
        create_main_packet(4096),
        create_file_description(FileId::new([1; 16]), "file.txt", 1024),
        create_main_packet(4096),
    ];
    let names = processing::extract_filenames(&packets);
    assert_eq!(names.len(), 1);
}

// ============================================================================
// extract_main_stats Tests
// ============================================================================

#[test]
fn test_extract_main_packet_stats_no_packets() {
    let packets = vec![];
    let (_block_size, total_blocks) = processing::extract_main_stats(&packets);
    assert_eq!(_block_size as u32, 0);
    assert_eq!(total_blocks, 0);
}

#[test]
fn test_extract_main_packet_stats_only_main() {
    let packets = vec![create_main_packet(4096)];
    let (_block_size, total_blocks) = processing::extract_main_stats(&packets);
    assert_eq!(_block_size as u32, 4096);
    assert_eq!(total_blocks, 0);
}

#[test]
fn test_extract_main_packet_stats_single_file() {
    let packets = vec![
        create_main_packet(1024),
        create_file_description(FileId::new([1; 16]), "file.txt", 4096),
    ];
    let (block_size, total_blocks) = processing::extract_main_stats(&packets);
    assert_eq!(block_size as u32, 1024);
    assert_eq!(total_blocks, 4);
}

#[test]
fn test_extract_main_packet_stats_multiple_files() {
    let packets = vec![
        create_main_packet(1000),
        create_file_description(FileId::new([1; 16]), "file1.txt", 2000),
        create_file_description(FileId::new([2; 16]), "file2.txt", 3000),
    ];
    let (block_size, total_blocks) = processing::extract_main_stats(&packets);
    assert_eq!(block_size as u32, 1000);
    assert_eq!(total_blocks, 5); // 2 + 3
}

#[test]
fn test_extract_main_packet_stats_partial_blocks() {
    let packets = vec![
        create_main_packet(1000),
        create_file_description(FileId::new([1; 16]), "file.txt", 1500),
    ];
    let (_block_size, total_blocks) = processing::extract_main_stats(&packets);
    assert_eq!(total_blocks, 2); // Rounds up
}

#[test]
fn test_extract_main_packet_stats_duplicate_files() {
    let packets = vec![
        create_main_packet(1024),
        create_file_description(FileId::new([1; 16]), "file.txt", 4096),
        create_file_description(FileId::new([1; 16]), "file.txt", 4096),
    ];
    let (_block_size, total_blocks) = processing::extract_main_stats(&packets);
    assert_eq!(total_blocks, 4); // Not 8 - deduplicated by file_id
}

// ============================================================================
// extract_file_descriptions and calculate total Tests
// ============================================================================

#[test]
fn test_calculate_total_size_empty() {
    let total: u64 = processing::extract_file_descriptions(&[])
        .into_iter()
        .map(|fd| fd.file_length)
        .sum();
    assert_eq!(total, 0);
}

#[test]
fn test_calculate_total_size_single_file() {
    let packets = vec![create_file_description(
        FileId::new([1; 16]),
        "file.txt",
        5000,
    )];
    let total: u64 = processing::extract_file_descriptions(&packets)
        .into_iter()
        .map(|fd| fd.file_length)
        .sum();
    assert_eq!(total, 5000);
}

#[test]
fn test_calculate_total_size_multiple_files() {
    let packets = vec![
        create_file_description(FileId::new([1; 16]), "file1.txt", 1000),
        create_file_description(FileId::new([2; 16]), "file2.txt", 2000),
        create_file_description(FileId::new([3; 16]), "file3.txt", 3000),
    ];
    let total: u64 = processing::extract_file_descriptions(&packets)
        .into_iter()
        .map(|fd| fd.file_length)
        .sum();
    assert_eq!(total, 6000);
}

#[test]
fn test_calculate_total_size_duplicate_file_ids() {
    let packets = vec![
        create_file_description(FileId::new([1; 16]), "file.txt", 5000),
        create_file_description(FileId::new([1; 16]), "file.txt", 5000),
    ];
    let total: u64 = processing::extract_file_descriptions(&packets)
        .into_iter()
        .map(|fd| fd.file_length)
        .sum();
    assert_eq!(total, 5000); // Counted once
}

#[test]
fn test_calculate_total_size_ignores_non_file_packets() {
    let packets = vec![
        create_main_packet(1024),
        create_file_description(FileId::new([1; 16]), "file.txt", 5000),
        create_main_packet(1024),
    ];
    let total: u64 = processing::extract_file_descriptions(&packets)
        .into_iter()
        .map(|fd| fd.file_length)
        .sum();
    assert_eq!(total, 5000);
}

#[test]
fn test_calculate_total_size_large_files() {
    let packets = vec![
        create_file_description(FileId::new([1; 16]), "large1.bin", 1_000_000_000),
        create_file_description(FileId::new([2; 16]), "large2.bin", 2_000_000_000),
    ];
    let total: u64 = processing::extract_file_descriptions(&packets)
        .into_iter()
        .map(|fd| fd.file_length)
        .sum();
    assert_eq!(total, 3_000_000_000);
}

// ============================================================================
// extract_file_info Tests
// ============================================================================

#[test]
fn test_collect_file_info_empty() {
    let info = processing::extract_file_info(&[])
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(info.len(), 0);
}

#[test]
fn test_collect_file_info_single_file() {
    let file_id = FileId::new([42; 16]);
    let packets = vec![create_file_description(file_id, "test.txt", 5000)];
    let info = processing::extract_file_info(&packets)
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(info.len(), 1);
    assert!(info.contains_key("test.txt"));
}

#[test]
fn test_collect_file_info_multiple_files() {
    let packets = vec![
        create_file_description(FileId::new([1; 16]), "file1.txt", 1000),
        create_file_description(FileId::new([2; 16]), "file2.bin", 2000),
    ];
    let info = processing::extract_file_info(&packets)
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(info.len(), 2);
}

#[test]
fn test_collect_file_info_duplicate_names() {
    let packets = vec![
        create_file_description(FileId::new([1; 16]), "file.txt", 1000),
        create_file_description(FileId::new([2; 16]), "file.txt", 2000),
    ];
    let info = processing::extract_file_info(&packets)
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(info.len(), 1); // Last one wins
}

// ============================================================================
// calculate_par2_stats Tests
// ============================================================================

#[test]
fn test_calculate_par2_stats_empty() {
    let stats = analysis::calculate_par2_stats(&[], 0);
    assert_eq!(stats.file_count, 0);
    assert_eq!(stats.block_size, 0);
    assert_eq!(stats.total_blocks, 0);
    assert_eq!(stats.total_size, 0);
    assert_eq!(stats.recovery_blocks, 0);
}

#[test]
fn test_calculate_par2_stats_basic() {
    let packets = vec![
        create_main_packet(4096),
        create_file_description(FileId::new([1; 16]), "file1.txt", 8192),
        create_file_description(FileId::new([2; 16]), "file2.txt", 4096),
    ];
    let stats = analysis::calculate_par2_stats(&packets, 5);
    assert_eq!(stats.file_count, 2);
    assert_eq!(stats.block_size, 4096);
    assert_eq!(stats.total_blocks, 3);
    assert_eq!(stats.total_size, 12288);
    assert_eq!(stats.recovery_blocks, 5);
}

#[test]
fn test_calculate_par2_stats_complex() {
    let packets = vec![
        create_main_packet(2048),
        create_file_description(FileId::new([1; 16]), "data.bin", 10000),
        create_file_description(FileId::new([2; 16]), "backup.dat", 5000),
        create_file_description(FileId::new([3; 16]), "readme.txt", 500),
    ];
    let stats = analysis::calculate_par2_stats(&packets, 10);
    assert_eq!(stats.file_count, 3);
    assert_eq!(stats.block_size, 2048);
    assert_eq!(stats.total_size, 15500);
    assert_eq!(stats.recovery_blocks, 10);
}

// ============================================================================
// Par2Stats trait tests
// ============================================================================

#[test]
fn test_par2_stats_clone() {
    let stats = analysis::Par2Stats {
        file_count: 5,
        block_size: 4096,
        total_blocks: 20,
        total_size: 81920,
        recovery_blocks: 10,
    };
    let cloned = stats.clone();
    assert_eq!(cloned.file_count, 5);
}

#[test]
fn test_par2_stats_debug_format() {
    let stats = analysis::Par2Stats {
        file_count: 3,
        block_size: 2048,
        total_blocks: 15,
        total_size: 30720,
        recovery_blocks: 5,
    };
    let debug_str = format!("{:?}", stats);
    assert!(debug_str.contains("file_count"));
}

#[test]
fn test_par2_stats_print_summary() {
    // Just ensure it doesn't panic
    let stats = analysis::Par2Stats {
        file_count: 3,
        block_size: 4096,
        total_blocks: 10,
        total_size: 40960,
        recovery_blocks: 5,
    };
    analysis::print_summary_stats(&stats);
}
