use par2rs::domain::{FileId, Md5Hash, RecoverySetId};
use par2rs::packets::FileDescriptionPacket;
use par2rs::verify::{calculate_file_md5, calculate_file_md5_16k};
use par2rs::verify::{FileStatus, FileVerifier};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

// Helper to create a test file
fn create_test_file(dir: &TempDir, name: &str, content: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    let mut file = fs::File::create(&path).unwrap();
    file.write_all(content).unwrap();
    path
}

// Helper to create a FileDescription packet from a file
fn create_file_description(file_path: &std::path::Path, file_name: &str) -> FileDescriptionPacket {
    let metadata = fs::metadata(file_path).unwrap();
    let md5_full = calculate_file_md5(file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(file_path).unwrap();

    FileDescriptionPacket {
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([1; 16]),
        md5_hash: md5_full,
        md5_16k,
        file_length: metadata.len(),
        file_name: file_name.as_bytes().to_vec(),
        length: 120 + file_name.len() as u64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
    }
}

#[test]
fn test_file_verifier_new() {
    let dir = TempDir::new().unwrap();
    let _verifier = FileVerifier::new(dir.path());
    // Should create successfully
}

#[test]
fn test_determine_file_status_present() {
    let dir = TempDir::new().unwrap();
    let content = b"Hello, world!";
    let file_path = create_test_file(&dir, "test.txt", content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();
    let file_length = content.len() as u64;

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.determine_file_status("test.txt", &md5_16k, &md5_full, file_length);

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_determine_file_status_missing() {
    let dir = TempDir::new().unwrap();
    let verifier = FileVerifier::new(dir.path());

    let dummy_hash = Md5Hash::new([0; 16]);
    let status = verifier.determine_file_status("nonexistent.txt", &dummy_hash, &dummy_hash, 100);

    assert_eq!(status, FileStatus::Missing);
}

#[test]
fn test_determine_file_status_wrong_size() {
    let dir = TempDir::new().unwrap();
    let content = b"Hello, world!";
    let file_path = create_test_file(&dir, "test.txt", content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    // Pass wrong length
    let status = verifier.determine_file_status("test.txt", &md5_16k, &md5_full, 999);

    assert_eq!(status, FileStatus::Corrupted);
}

#[test]
fn test_determine_file_status_wrong_16k_hash() {
    let dir = TempDir::new().unwrap();
    let content = b"Hello, world!";
    let file_path = create_test_file(&dir, "test.txt", content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let file_length = content.len() as u64;

    let verifier = FileVerifier::new(dir.path());
    let wrong_hash = Md5Hash::new([0xFF; 16]);
    let status = verifier.determine_file_status("test.txt", &wrong_hash, &md5_full, file_length);

    assert_eq!(status, FileStatus::Corrupted);
}

#[test]
fn test_determine_file_status_wrong_full_hash() {
    let dir = TempDir::new().unwrap();
    let content = b"Hello, world!";
    let file_path = create_test_file(&dir, "test.txt", content);

    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();
    let file_length = content.len() as u64;

    let verifier = FileVerifier::new(dir.path());
    let wrong_hash = Md5Hash::new([0xFF; 16]);
    let status = verifier.determine_file_status("test.txt", &md5_16k, &wrong_hash, file_length);

    assert_eq!(status, FileStatus::Corrupted);
}

#[test]
fn test_verify_file_from_description_present() {
    let dir = TempDir::new().unwrap();
    let content = b"Test file content";
    let file_path = create_test_file(&dir, "test.txt", content);

    let file_desc = create_file_description(&file_path, "test.txt");
    let verifier = FileVerifier::new(dir.path());

    let status = verifier.verify_file_from_description(&file_desc);
    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verify_file_from_description_missing() {
    let dir = TempDir::new().unwrap();

    let file_desc = FileDescriptionPacket {
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([1; 16]),
        md5_hash: Md5Hash::new([0; 16]),
        md5_16k: Md5Hash::new([0; 16]),
        file_length: 100,
        file_name: b"nonexistent.txt".to_vec(),
        length: 135,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
    };

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.verify_file_from_description(&file_desc);
    assert_eq!(status, FileStatus::Missing);
}

#[test]
fn test_verify_file_from_description_corrupted() {
    let dir = TempDir::new().unwrap();
    let content = b"Test file content";
    let file_path = create_test_file(&dir, "test.txt", content);

    let mut file_desc = create_file_description(&file_path, "test.txt");
    // Corrupt the hash
    file_desc.md5_hash = Md5Hash::new([0xFF; 16]);

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.verify_file_from_description(&file_desc);
    assert_eq!(status, FileStatus::Corrupted);
}

#[test]
fn test_verifier_with_large_file() {
    let dir = TempDir::new().unwrap();
    // Create a file larger than 16KB
    let content = vec![0xAB; 20000];
    let file_path = create_test_file(&dir, "large.bin", &content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.determine_file_status("large.bin", &md5_16k, &md5_full, 20000);

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verifier_edge_case_exactly_16kb() {
    let dir = TempDir::new().unwrap();
    let content = vec![0x42; 16384]; // Exactly 16KB
    let file_path = create_test_file(&dir, "exact16k.bin", &content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.determine_file_status("exact16k.bin", &md5_16k, &md5_full, 16384);

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verifier_with_zero_byte_file() {
    let dir = TempDir::new().unwrap();
    let content = vec![];
    let file_path = create_test_file(&dir, "empty.txt", &content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.determine_file_status("empty.txt", &md5_16k, &md5_full, 0);

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verifier_with_subdirectory() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("subdir")).unwrap();
    let content = b"Nested file";
    let file_path = dir.path().join("subdir").join("nested.txt");
    let mut file = fs::File::create(&file_path).unwrap();
    file.write_all(content).unwrap();

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.determine_file_status(
        "subdir/nested.txt",
        &md5_16k,
        &md5_full,
        content.len() as u64,
    );

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verifier_binary_content() {
    let dir = TempDir::new().unwrap();
    let content: Vec<u8> = (0..=255).collect(); // All possible byte values
    let file_path = create_test_file(&dir, "binary.dat", &content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    let status =
        verifier.determine_file_status("binary.dat", &md5_16k, &md5_full, content.len() as u64);

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verifier_file_path_with_spaces() {
    let dir = TempDir::new().unwrap();
    let content = b"File with spaces";
    let file_path = create_test_file(&dir, "file with spaces.txt", content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.determine_file_status(
        "file with spaces.txt",
        &md5_16k,
        &md5_full,
        content.len() as u64,
    );

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verifier_unicode_filename() {
    let dir = TempDir::new().unwrap();
    let content = b"Unicode test";
    let file_path = create_test_file(&dir, "тест_测试.txt", content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    let status =
        verifier.determine_file_status("тест_测试.txt", &md5_16k, &md5_full, content.len() as u64);

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_multiple_files_same_verifier() {
    let dir = TempDir::new().unwrap();

    let content1 = b"File 1";
    let file1 = create_test_file(&dir, "file1.txt", content1);
    let md5_1 = calculate_file_md5(&file1).unwrap();
    let md5_16k_1 = calculate_file_md5_16k(&file1).unwrap();

    let content2 = b"File 2 with different content";
    let file2 = create_test_file(&dir, "file2.txt", content2);
    let md5_2 = calculate_file_md5(&file2).unwrap();
    let md5_16k_2 = calculate_file_md5_16k(&file2).unwrap();

    let verifier = FileVerifier::new(dir.path());

    let status1 =
        verifier.determine_file_status("file1.txt", &md5_16k_1, &md5_1, content1.len() as u64);
    let status2 =
        verifier.determine_file_status("file2.txt", &md5_16k_2, &md5_2, content2.len() as u64);

    assert_eq!(status1, FileStatus::Present);
    assert_eq!(status2, FileStatus::Present);
}

#[test]
fn test_verifier_large_file_wrong_16k_hash() {
    let dir = TempDir::new().unwrap();
    let content = vec![0u8; 50000]; // Larger than 16KB
    let file_path = create_test_file(&dir, "large.bin", &content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let wrong_md5_16k = Md5Hash::new([0xFF; 16]);

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.determine_file_status(
        "large.bin",
        &wrong_md5_16k,
        &md5_full,
        content.len() as u64,
    );

    // Should be detected as corrupted based on 16KB hash alone
    assert_eq!(status, FileStatus::Corrupted);
}

// ============================================================================
// Additional tests for comprehensive coverage
// ============================================================================

use par2rs::checksum::ProgressReporter;
use par2rs::domain::Crc32Value;
use std::path::Path;

// Simple progress reporter for testing
struct TestProgressReporter {
    calls: std::sync::Arc<std::sync::Mutex<usize>>,
}

impl TestProgressReporter {
    fn new() -> Self {
        Self {
            calls: std::sync::Arc::new(std::sync::Mutex::new(0)),
        }
    }

    fn call_count(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl ProgressReporter for TestProgressReporter {
    fn report_scanning_progress(&self, _file_name: &str, _bytes_processed: u64, _total_bytes: u64) {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
    }

    fn clear_progress_line(&self) {
        // No-op for testing
    }
}

#[test]
fn test_verify_file_with_progress_present() {
    let dir = TempDir::new().unwrap();
    // Use a larger file to ensure progress is reported
    let content = vec![0x42; 50000]; // 50KB file
    let file_path = create_test_file(&dir, "test.bin", &content);

    let file_desc = create_file_description(&file_path, "test.bin");
    let verifier = FileVerifier::new(dir.path());
    let progress = TestProgressReporter::new();

    let status = verifier.verify_file_with_progress(&file_desc, &progress);
    assert!(status.is_ok());
    assert_eq!(status.unwrap(), FileStatus::Present);
    // Note: Progress reporting depends on buffer size and file size thresholds
    // The main thing is that the method accepts a progress reporter and completes successfully
}

#[test]
fn test_verify_file_with_progress_missing() {
    let dir = TempDir::new().unwrap();

    let file_desc = FileDescriptionPacket {
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([1; 16]),
        md5_hash: Md5Hash::new([0; 16]),
        md5_16k: Md5Hash::new([0; 16]),
        file_length: 100,
        file_name: b"missing.txt".to_vec(),
        length: 131,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
    };

    let verifier = FileVerifier::new(dir.path());
    let progress = TestProgressReporter::new();

    let status = verifier.verify_file_with_progress(&file_desc, &progress);
    assert!(status.is_ok());
    assert_eq!(status.unwrap(), FileStatus::Missing);
    // No progress for missing files
    assert_eq!(progress.call_count(), 0);
}

#[test]
fn test_verify_file_with_progress_corrupted() {
    let dir = TempDir::new().unwrap();
    let content = b"Test content";
    let file_path = create_test_file(&dir, "test.txt", content);

    let mut file_desc = create_file_description(&file_path, "test.txt");
    // Corrupt the full hash
    file_desc.md5_hash = Md5Hash::new([0xFF; 16]);

    let verifier = FileVerifier::new(dir.path());
    let progress = TestProgressReporter::new();

    let status = verifier.verify_file_with_progress(&file_desc, &progress);
    assert!(status.is_ok());
    assert_eq!(status.unwrap(), FileStatus::Corrupted);
}

#[test]
fn test_verify_file_with_progress_wrong_size() {
    let dir = TempDir::new().unwrap();
    let content = b"Test content";
    let file_path = create_test_file(&dir, "test.txt", content);

    let mut file_desc = create_file_description(&file_path, "test.txt");
    // Wrong file length
    file_desc.file_length = 999;

    let verifier = FileVerifier::new(dir.path());
    let progress = TestProgressReporter::new();

    let status = verifier.verify_file_with_progress(&file_desc, &progress);
    assert!(status.is_ok());
    assert_eq!(status.unwrap(), FileStatus::Corrupted);
}

#[test]
fn test_verify_file_with_progress_large_file() {
    let dir = TempDir::new().unwrap();
    let content = vec![0x42; 500000]; // 500KB file to ensure multiple callbacks
    let file_path = create_test_file(&dir, "large.bin", &content);

    let file_desc = create_file_description(&file_path, "large.bin");
    let verifier = FileVerifier::new(dir.path());
    let progress = TestProgressReporter::new();

    let status = verifier.verify_file_with_progress(&file_desc, &progress);
    assert!(status.is_ok());
    assert_eq!(status.unwrap(), FileStatus::Present);
    // Note: Progress reporting depends on implementation details
    // The key test is that verify_file_with_progress works correctly
}

#[test]
fn test_verify_file_with_progress_empty_file() {
    let dir = TempDir::new().unwrap();
    let content = b"";
    let file_path = create_test_file(&dir, "empty.txt", content);

    let file_desc = create_file_description(&file_path, "empty.txt");
    let verifier = FileVerifier::new(dir.path());
    let progress = TestProgressReporter::new();

    let status = verifier.verify_file_with_progress(&file_desc, &progress);
    assert!(status.is_ok());
    assert_eq!(status.unwrap(), FileStatus::Present);
}

#[test]
fn test_verify_file_with_progress_exactly_16kb() {
    let dir = TempDir::new().unwrap();
    let content = vec![0x55; 16384]; // Exactly 16KB
    let file_path = create_test_file(&dir, "16kb.bin", &content);

    let file_desc = create_file_description(&file_path, "16kb.bin");
    let verifier = FileVerifier::new(dir.path());
    let progress = TestProgressReporter::new();

    let status = verifier.verify_file_with_progress(&file_desc, &progress);
    assert!(status.is_ok());
    assert_eq!(status.unwrap(), FileStatus::Present);
}

#[test]
fn test_verify_file_with_progress_just_over_16kb() {
    let dir = TempDir::new().unwrap();
    let content = vec![0x66; 16385]; // Just over 16KB
    let file_path = create_test_file(&dir, "over16kb.bin", &content);

    let file_desc = create_file_description(&file_path, "over16kb.bin");
    let verifier = FileVerifier::new(dir.path());
    let progress = TestProgressReporter::new();

    let status = verifier.verify_file_with_progress(&file_desc, &progress);
    assert!(status.is_ok());
    assert_eq!(status.unwrap(), FileStatus::Present);
    // Progress may or may not be called for files just over 16KB
}

#[test]
fn test_verify_file_with_progress_16k_hash_mismatch() {
    let dir = TempDir::new().unwrap();
    let content = vec![0x77; 32768]; // 32KB file
    let file_path = create_test_file(&dir, "test.bin", &content);

    let mut file_desc = create_file_description(&file_path, "test.bin");
    // Corrupt the 16K hash but keep the full hash correct
    file_desc.md5_16k = Md5Hash::new([0xFF; 16]);

    let verifier = FileVerifier::new(dir.path());
    let progress = TestProgressReporter::new();

    let status = verifier.verify_file_with_progress(&file_desc, &progress);
    assert!(status.is_ok());
    assert_eq!(status.unwrap(), FileStatus::Corrupted);
}

#[test]
fn test_verify_file_with_unicode_filename() {
    let dir = TempDir::new().unwrap();
    let content = b"Unicode test content";
    let filename = "测试文件.txt"; // Chinese characters
    let file_path = dir.path().join(filename);
    std::fs::write(&file_path, content).unwrap();

    let file_desc = create_file_description(&file_path, filename);
    let verifier = FileVerifier::new(dir.path());

    let status = verifier.verify_file_from_description(&file_desc);
    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verify_file_with_special_chars() {
    let dir = TempDir::new().unwrap();
    let content = b"Special chars test";
    let filename = "file with spaces & special!@#.txt";
    let file_path = dir.path().join(filename);
    std::fs::write(&file_path, content).unwrap();

    let file_desc = create_file_description(&file_path, filename);
    let verifier = FileVerifier::new(dir.path());

    let status = verifier.verify_file_from_description(&file_desc);
    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verify_file_io_error_handling() {
    let dir = TempDir::new().unwrap();

    // Create a file descriptor for a file in a non-existent subdirectory
    let file_desc = FileDescriptionPacket {
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([1; 16]),
        md5_hash: Md5Hash::new([0; 16]),
        md5_16k: Md5Hash::new([0; 16]),
        file_length: 100,
        file_name: b"nonexistent/subdir/file.txt".to_vec(),
        length: 146,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
    };

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.verify_file_from_description(&file_desc);
    assert_eq!(status, FileStatus::Missing);
}

#[test]
fn test_verify_file_with_symlink() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        let content = b"Original file content";
        let original = dir.path().join("original.txt");
        std::fs::write(&original, content).unwrap();

        let link = dir.path().join("link.txt");
        symlink(&original, &link).unwrap();

        let file_desc = create_file_description(&link, "link.txt");
        let verifier = FileVerifier::new(dir.path());

        let status = verifier.verify_file_from_description(&file_desc);
        // Symlinks should be followed and verified
        assert_eq!(status, FileStatus::Present);
    }
}

#[test]
fn test_verify_multiple_files_same_verifier() {
    let dir = TempDir::new().unwrap();

    let file1_content = b"File 1 content";
    let file1_path = create_test_file(&dir, "file1.txt", file1_content);
    let file1_desc = create_file_description(&file1_path, "file1.txt");

    let file2_content = b"File 2 different content";
    let file2_path = create_test_file(&dir, "file2.txt", file2_content);
    let file2_desc = create_file_description(&file2_path, "file2.txt");

    let verifier = FileVerifier::new(dir.path());

    let status1 = verifier.verify_file_from_description(&file1_desc);
    assert_eq!(status1, FileStatus::Present);

    let status2 = verifier.verify_file_from_description(&file2_desc);
    assert_eq!(status2, FileStatus::Present);
}

#[test]
fn test_verify_file_readonly() {
    let dir = TempDir::new().unwrap();
    let content = b"Read-only file content";
    let file_path = create_test_file(&dir, "readonly.txt", content);

    // Make file read-only
    let mut perms = std::fs::metadata(&file_path).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&file_path, perms).unwrap();

    let file_desc = create_file_description(&file_path, "readonly.txt");
    let verifier = FileVerifier::new(dir.path());

    let status = verifier.verify_file_from_description(&file_desc);
    // Should still be able to read and verify readonly files
    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verify_file_very_large_descriptor_length() {
    let dir = TempDir::new().unwrap();
    let content = b"Test";
    let file_path = create_test_file(&dir, "test.txt", content);

    let mut file_desc = create_file_description(&file_path, "test.txt");
    // Set an unusually large length value in packet metadata
    file_desc.length = 999999;

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.verify_file_from_description(&file_desc);
    // Should still work, packet length field doesn't affect file verification
    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verify_file_binary_data() {
    let dir = TempDir::new().unwrap();
    let content: Vec<u8> = (0..256).map(|i| i as u8).collect();
    let file_path = create_test_file(&dir, "binary.dat", &content);

    let file_desc = create_file_description(&file_path, "binary.dat");
    let verifier = FileVerifier::new(dir.path());

    let status = verifier.verify_file_from_description(&file_desc);
    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verify_file_all_zeros() {
    let dir = TempDir::new().unwrap();
    let content = vec![0u8; 1024];
    let file_path = create_test_file(&dir, "zeros.bin", &content);

    let file_desc = create_file_description(&file_path, "zeros.bin");
    let verifier = FileVerifier::new(dir.path());

    let status = verifier.verify_file_from_description(&file_desc);
    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verify_file_all_ones() {
    let dir = TempDir::new().unwrap();
    let content = vec![0xFFu8; 1024];
    let file_path = create_test_file(&dir, "ones.bin", &content);

    let file_desc = create_file_description(&file_path, "ones.bin");
    let verifier = FileVerifier::new(dir.path());

    let status = verifier.verify_file_from_description(&file_desc);
    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verify_file_with_null_bytes_in_name() {
    let dir = TempDir::new().unwrap();
    let content = b"Test content";

    // Create a file descriptor with null bytes in the file name
    // This tests handling of potentially malformed packet data
    let mut file_name = b"test".to_vec();
    file_name.push(0);
    file_name.extend_from_slice(b"file.txt");

    let file_path = create_test_file(&dir, "testfile.txt", content);

    let file_desc = FileDescriptionPacket {
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([1; 16]),
        md5_hash: calculate_file_md5(&file_path).unwrap(),
        md5_16k: calculate_file_md5_16k(&file_path).unwrap(),
        file_length: content.len() as u64,
        file_name,
        length: 118,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
    };

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.verify_file_from_description(&file_desc);
    // Should handle gracefully, likely returning Missing since name won't match
    assert!(status == FileStatus::Missing || status == FileStatus::Corrupted);
}

#[test]
fn test_determine_file_status_method() {
    // Test the determine_file_status method with explicit parameters
    let dir = TempDir::new().unwrap();
    let content = b"Test content for determine_file_status";
    let filename = "testfile.txt";
    let file_path = dir.path().join(filename);
    std::fs::write(&file_path, content).unwrap();

    let verifier = FileVerifier::new(dir.path());

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();
    let length = content.len() as u64;

    let status = verifier.determine_file_status(filename, &md5_16k, &md5_full, length);
    assert_eq!(status, FileStatus::Present);
}
