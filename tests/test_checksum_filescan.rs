use par2rs::checksum::*;
use par2rs::domain::{Crc32Value, Md5Hash};
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

// Custom progress reporter for testing
struct TestProgressReporter {
    calls: Arc<AtomicU64>,
    last_bytes: Arc<AtomicU64>,
    cleared: Arc<AtomicBool>,
}

impl TestProgressReporter {
    fn new() -> Self {
        Self {
            calls: Arc::new(AtomicU64::new(0)),
            last_bytes: Arc::new(AtomicU64::new(0)),
            cleared: Arc::new(AtomicBool::new(false)),
        }
    }

    fn call_count(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }

    fn was_cleared(&self) -> bool {
        self.cleared.load(Ordering::Relaxed)
    }
}

impl ProgressReporter for TestProgressReporter {
    fn report_scanning_progress(&self, _file_name: &str, bytes_processed: u64, _total_bytes: u64) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.last_bytes.store(bytes_processed, Ordering::Relaxed);
    }

    fn clear_progress_line(&self) {
        self.cleared.store(true, Ordering::Relaxed);
    }
}

#[test]
fn test_file_checksummer_new() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("test.txt");
    fs::write(&file_path, b"test").unwrap();

    let checksummer = FileCheckSummer::new(file_path.to_str().unwrap().to_string(), 1024).unwrap();
    assert_eq!(checksummer.file_size(), 4);
}

#[test]
fn test_file_checksummer_new_missing_file() {
    let result = FileCheckSummer::new("/nonexistent/file.txt".to_string(), 1024);
    assert!(result.is_err());
}

#[test]
fn test_compute_file_hashes_small_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("small.txt");
    let content = b"Small test file";
    fs::write(&file_path, content).unwrap();

    let checksummer = FileCheckSummer::new(file_path.to_str().unwrap().to_string(), 1024).unwrap();
    let results = checksummer.compute_file_hashes().unwrap();

    assert_eq!(results.file_size, content.len() as u64);
    assert_eq!(results.hash_16k.as_bytes().len(), 16);
    assert_eq!(results.hash_full.as_bytes().len(), 16);
    // For small files, hash_16k should equal hash_full
    assert_eq!(results.hash_16k, results.hash_full);
}

#[test]
fn test_compute_file_hashes_large_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("large.bin");
    // Create a file larger than 16KB
    let content = vec![0x42u8; 20000];
    fs::write(&file_path, &content).unwrap();

    let checksummer = FileCheckSummer::new(file_path.to_str().unwrap().to_string(), 1024).unwrap();
    let results = checksummer.compute_file_hashes().unwrap();

    assert_eq!(results.file_size, 20000);
    assert_eq!(results.hash_16k.as_bytes().len(), 16);
    assert_eq!(results.hash_full.as_bytes().len(), 16);
    // For large files, hashes should differ
    assert_ne!(results.hash_16k, results.hash_full);
}

#[test]
fn test_compute_file_hashes_exactly_16kb() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("exact16k.bin");
    let content = vec![0xAAu8; 16384];
    fs::write(&file_path, &content).unwrap();

    let checksummer = FileCheckSummer::new(file_path.to_str().unwrap().to_string(), 1024).unwrap();
    let results = checksummer.compute_file_hashes().unwrap();

    assert_eq!(results.file_size, 16384);
    // At exactly 16KB, both hashes should be the same
    assert_eq!(results.hash_16k, results.hash_full);
}

#[test]
fn test_compute_file_hashes_empty_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("empty.txt");
    fs::write(&file_path, b"").unwrap();

    let checksummer = FileCheckSummer::new(file_path.to_str().unwrap().to_string(), 1024).unwrap();
    let results = checksummer.compute_file_hashes().unwrap();

    assert_eq!(results.file_size, 0);
    assert_eq!(results.hash_16k, results.hash_full);
}

#[test]
fn test_compute_file_hashes_with_progress() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("progress_test.bin");
    // Create a file larger than 1MB to trigger progress reporting
    let content = vec![0x55u8; 2 * 1024 * 1024]; // 2MB
    fs::write(&file_path, &content).unwrap();

    let checksummer = FileCheckSummer::new(file_path.to_str().unwrap().to_string(), 1024).unwrap();
    let reporter = TestProgressReporter::new();

    let results = checksummer
        .compute_file_hashes_with_progress(&reporter)
        .unwrap();

    assert_eq!(results.file_size, 2 * 1024 * 1024);
    // Progress should have been reported
    assert!(reporter.call_count() > 0);
    assert!(reporter.was_cleared());
}

#[test]
fn test_compute_file_hashes_with_progress_small_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("small.txt");
    // Small file (< 1MB) should not trigger progress
    let content = b"Small file";
    fs::write(&file_path, content).unwrap();

    let checksummer = FileCheckSummer::new(file_path.to_str().unwrap().to_string(), 1024).unwrap();
    let reporter = TestProgressReporter::new();

    let results = checksummer
        .compute_file_hashes_with_progress(&reporter)
        .unwrap();

    assert_eq!(results.file_size, content.len() as u64);
    // No progress for small files
    assert_eq!(reporter.call_count(), 0);
}

#[test]
fn test_scan_with_block_checksums_no_expected() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("test.bin");
    let content = vec![0x42u8; 4096]; // 4 blocks of 1024
    fs::write(&file_path, &content).unwrap();

    let checksummer = FileCheckSummer::new(file_path.to_str().unwrap().to_string(), 1024).unwrap();

    // No expected checksums
    let expected: Vec<(Md5Hash, Crc32Value)> = vec![];
    let (hash_16k, hash_full, valid_count, damaged) =
        checksummer.scan_with_block_checksums(&expected).unwrap();

    assert_eq!(hash_16k.as_bytes().len(), 16);
    assert_eq!(hash_full.as_bytes().len(), 16);
    assert_eq!(valid_count, 0); // No expected checksums means no valid blocks
    assert_eq!(damaged.len(), 0);
}

#[test]
fn test_scan_with_block_checksums_matching() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("test.bin");
    let block_size = 1024;

    // Create file with known content
    let block1 = vec![0x11u8; block_size];
    let block2 = vec![0x22u8; block_size];
    let mut content = Vec::new();
    content.extend_from_slice(&block1);
    content.extend_from_slice(&block2);
    fs::write(&file_path, &content).unwrap();

    // Compute expected checksums
    let expected = vec![
        compute_block_checksums(&block1),
        compute_block_checksums(&block2),
    ];

    let checksummer =
        FileCheckSummer::new(file_path.to_str().unwrap().to_string(), block_size).unwrap();

    let (hash_16k, hash_full, valid_count, damaged) =
        checksummer.scan_with_block_checksums(&expected).unwrap();

    assert_eq!(hash_16k.as_bytes().len(), 16);
    assert_eq!(hash_full.as_bytes().len(), 16);
    assert_eq!(valid_count, 2); // Both blocks should match
    assert_eq!(damaged.len(), 0);
}

#[test]
fn test_scan_with_block_checksums_corrupted() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("corrupted.bin");
    let block_size = 1024;

    // Create file with content
    let block1 = vec![0x11u8; block_size];
    let block2 = vec![0x22u8; block_size]; // This will be "corrupted"
    let mut content = Vec::new();
    content.extend_from_slice(&block1);
    content.extend_from_slice(&block2);
    fs::write(&file_path, &content).unwrap();

    // Create expected checksums where block2 is different
    let wrong_block2 = vec![0x33u8; block_size];
    let expected = vec![
        compute_block_checksums(&block1),
        compute_block_checksums(&wrong_block2), // Wrong checksum
    ];

    let checksummer =
        FileCheckSummer::new(file_path.to_str().unwrap().to_string(), block_size).unwrap();

    let (_hash_16k, _hash_full, valid_count, damaged) =
        checksummer.scan_with_block_checksums(&expected).unwrap();

    assert_eq!(valid_count, 1); // Only first block matches
    assert_eq!(damaged.len(), 1); // Second block is damaged
    assert_eq!(damaged[0], 1); // Block index 1
}

#[test]
fn test_scan_with_block_checksums_partial_block() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("partial.bin");
    let block_size = 1024;

    // Create file with one full block and one partial
    let block1 = vec![0x11u8; block_size];
    let partial_block = vec![0x22u8; 500]; // Partial block
    let mut content = Vec::new();
    content.extend_from_slice(&block1);
    content.extend_from_slice(&partial_block);
    fs::write(&file_path, &content).unwrap();

    // Compute expected checksums with padding
    let expected = vec![
        compute_block_checksums(&block1),
        compute_block_checksums_padded(&partial_block, block_size),
    ];

    let checksummer =
        FileCheckSummer::new(file_path.to_str().unwrap().to_string(), block_size).unwrap();

    let (_hash_16k, _hash_full, valid_count, damaged) =
        checksummer.scan_with_block_checksums(&expected).unwrap();

    assert_eq!(valid_count, 2); // Both blocks should match
    assert_eq!(damaged.len(), 0);
}

#[test]
fn test_scan_with_block_checksums_with_progress() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("progress.bin");
    let block_size = 1024;

    // Create large file to trigger progress
    let content = vec![0x42u8; 2 * 1024 * 1024]; // 2MB
    fs::write(&file_path, &content).unwrap();

    let checksummer =
        FileCheckSummer::new(file_path.to_str().unwrap().to_string(), block_size).unwrap();
    let reporter = TestProgressReporter::new();

    let expected: Vec<(Md5Hash, Crc32Value)> = vec![];
    let (_hash_16k, _hash_full, _valid_count, _damaged) = checksummer
        .scan_with_block_checksums_with_progress(&expected, &reporter)
        .unwrap();

    // Progress should be reported for large files
    assert!(reporter.call_count() > 0);
    assert!(reporter.was_cleared());
}

#[test]
fn test_console_progress_reporter_new() {
    let reporter = ConsoleProgressReporter::new();
    // Should be created successfully
    let _ = reporter;
}

#[test]
fn test_console_progress_reporter_report_progress() {
    let reporter = ConsoleProgressReporter::new();
    // Should not panic when reporting progress
    reporter.report_scanning_progress("test.txt", 50, 100);
    reporter.report_scanning_progress("test.txt", 100, 100);
    reporter.clear_progress_line();
}

#[test]
fn test_console_progress_reporter_zero_total() {
    let reporter = ConsoleProgressReporter::new();
    // Should handle zero total bytes gracefully
    reporter.report_scanning_progress("empty.txt", 0, 0);
}

#[test]
fn test_console_progress_reporter_long_filename() {
    let reporter = ConsoleProgressReporter::new();
    let long_name = "a".repeat(100);
    // Should truncate long filenames
    reporter.report_scanning_progress(&long_name, 50, 100);
}

#[test]
fn test_silent_progress_reporter() {
    let reporter = SilentProgressReporter;
    // Should do nothing, but not panic
    reporter.report_scanning_progress("test.txt", 50, 100);
    reporter.clear_progress_line();
}

#[test]
fn test_checksum_results_structure() {
    let results = ChecksumResults {
        hash_16k: Md5Hash::new([1; 16]),
        hash_full: Md5Hash::new([2; 16]),
        file_size: 12345,
    };

    assert_eq!(results.file_size, 12345);
    assert_eq!(results.hash_16k.as_bytes().len(), 16);
    assert_eq!(results.hash_full.as_bytes().len(), 16);
}

#[test]
fn test_file_checksummer_different_block_sizes() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("test.bin");
    let content = vec![0x42u8; 10000];
    fs::write(&file_path, &content).unwrap();

    // Test with different block sizes
    for block_size in [512, 1024, 4096, 16384] {
        let checksummer =
            FileCheckSummer::new(file_path.to_str().unwrap().to_string(), block_size).unwrap();
        let results = checksummer.compute_file_hashes().unwrap();
        assert_eq!(results.file_size, 10000);
    }
}

#[test]
fn test_scan_empty_file_with_checksums() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("empty.txt");
    fs::write(&file_path, b"").unwrap();

    let checksummer = FileCheckSummer::new(file_path.to_str().unwrap().to_string(), 1024).unwrap();

    let expected: Vec<(Md5Hash, Crc32Value)> = vec![];
    let (hash_16k, hash_full, valid_count, damaged) =
        checksummer.scan_with_block_checksums(&expected).unwrap();

    assert_eq!(hash_16k, hash_full);
    assert_eq!(valid_count, 0);
    assert_eq!(damaged.len(), 0);
}

#[test]
fn test_scan_with_crc_match_md5_mismatch() {
    // This tests the edge case where CRC matches but MD5 doesn't
    // In practice this is extremely rare, but we should handle it
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("test.bin");
    let block_size = 1024;

    let block = vec![0x42u8; block_size];
    fs::write(&file_path, &block).unwrap();

    // Create a checksum with same CRC but different MD5
    let (_md5, crc) = compute_block_checksums(&block);
    let wrong_md5 = Md5Hash::new([99; 16]);
    let expected = vec![(wrong_md5, crc)];

    let checksummer =
        FileCheckSummer::new(file_path.to_str().unwrap().to_string(), block_size).unwrap();

    let (_hash_16k, _hash_full, valid_count, damaged) =
        checksummer.scan_with_block_checksums(&expected).unwrap();

    // Should detect MD5 mismatch even if CRC matches
    assert_eq!(valid_count, 0);
    assert_eq!(damaged.len(), 1);
}

#[test]
fn test_multiple_damaged_blocks() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("multi_damaged.bin");
    let block_size = 1024;

    // Create 4 blocks
    let blocks: Vec<Vec<u8>> = vec![
        vec![0x11u8; block_size],
        vec![0x22u8; block_size],
        vec![0x33u8; block_size],
        vec![0x44u8; block_size],
    ];

    let mut content = Vec::new();
    for block in &blocks {
        content.extend_from_slice(block);
    }
    fs::write(&file_path, &content).unwrap();

    // Create expected where blocks 1 and 3 are wrong
    let expected = vec![
        compute_block_checksums(&blocks[0]),                // Good
        compute_block_checksums(&vec![0xFFu8; block_size]), // Bad
        compute_block_checksums(&blocks[2]),                // Good
        compute_block_checksums(&vec![0xEEu8; block_size]), // Bad
    ];

    let checksummer =
        FileCheckSummer::new(file_path.to_str().unwrap().to_string(), block_size).unwrap();

    let (_hash_16k, _hash_full, valid_count, damaged) =
        checksummer.scan_with_block_checksums(&expected).unwrap();

    assert_eq!(valid_count, 2); // Blocks 0 and 2
    assert_eq!(damaged.len(), 2); // Blocks 1 and 3
    assert!(damaged.contains(&1));
    assert!(damaged.contains(&3));
}

#[test]
fn test_file_exactly_one_block() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("one_block.bin");
    let block_size = 1024;

    let content = vec![0x42u8; block_size];
    fs::write(&file_path, &content).unwrap();

    let checksummer =
        FileCheckSummer::new(file_path.to_str().unwrap().to_string(), block_size).unwrap();

    let expected = vec![compute_block_checksums(&content)];
    let (_hash_16k, _hash_full, valid_count, damaged) =
        checksummer.scan_with_block_checksums(&expected).unwrap();

    assert_eq!(valid_count, 1);
    assert_eq!(damaged.len(), 0);
}

#[test]
fn test_hash_accumulator_16k_boundary() {
    // Test that files just over 16KB correctly compute both hashes
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("just_over_16k.bin");

    // 16KB + 1 byte
    let content = vec![0x42u8; 16385];
    fs::write(&file_path, &content).unwrap();

    let checksummer = FileCheckSummer::new(file_path.to_str().unwrap().to_string(), 1024).unwrap();
    let results = checksummer.compute_file_hashes().unwrap();

    assert_eq!(results.file_size, 16385);
    // Hashes should be different
    assert_ne!(results.hash_16k, results.hash_full);
}
