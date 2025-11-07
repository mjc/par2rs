//! Comprehensive tests for verify/verifier.rs module
//! 
//! These tests cover the FileVerifier struct which provides unified file verification
//! for both par2verify and par2repair operations.

use par2rs::domain::{FileId, Md5Hash, RecoverySetId};
use par2rs::packets::{FileDescriptionPacket, parse_packets};
use par2rs::verify::{FileStatus, FileVerifier, VerificationConfig};
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use tempfile::tempdir;

/// Helper to create a test file with specific content
fn create_test_file(dir: &std::path::Path, name: &str, content: &[u8]) -> PathBuf {
    let path = dir.join(name);
    let mut file = File::create(&path).unwrap();
    file.write_all(content).unwrap();
    path
}

/// Compute MD5 hash using par2rs checksum module
fn compute_md5(data: &[u8]) -> Md5Hash {
    use md5::{Md5, Digest};
    let hash = Md5::digest(data);
    Md5Hash::new(hash.into())
}

/// Helper to create a mock FileDescriptionPacket for testing
fn create_mock_file_desc(
    file_name: &str,
    md5_16k: Md5Hash,
    md5_full: Md5Hash,
    file_length: u64,
) -> FileDescriptionPacket {
    let file_name_bytes = file_name.as_bytes().to_vec();
    FileDescriptionPacket {
        length: 120 + file_name_bytes.len() as u64,
        md5: Md5Hash::new([0u8; 16]), // Dummy packet MD5
        set_id: RecoverySetId::new([0u8; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([0u8; 16]),
        md5_hash: md5_full,
        md5_16k,
        file_length,
        file_name: file_name_bytes,
    }
}

#[test]
fn test_file_verifier_new() {
    // Test basic construction
    let temp = tempdir().unwrap();
    let _verifier = FileVerifier::new(temp.path());
    
    // Create a test file
    let content = b"test content for verification";
    let file_path = create_test_file(temp.path(), "test.dat", content);
    
    // Verify file exists and can be accessed
    assert!(file_path.exists());
}

#[test]
fn test_file_verifier_with_config() {
    // Test construction with custom config
    let temp = tempdir().unwrap();
    
    // Test with skip_full_md5 = true
    let config = VerificationConfig {
        parallel: false,
        skip_full_file_md5: true,
        threads: 1,
    };
    let verifier = FileVerifier::with_config(temp.path(), &config);
    
    // Create and verify file - should skip full MD5
    let content = b"skip md5 test content";
    create_test_file(temp.path(), "skip_md5.dat", content);
    
    let md5_16k = compute_md5(&content[..content.len().min(16384)]);
    let md5_full = compute_md5(content);
    
    let status = verifier.determine_file_status(
        "skip_md5.dat",
        &md5_16k,
        &md5_full,
        content.len() as u64,
    );
    
    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_determine_file_status_missing() {
    // Test missing file detection
    let temp = tempdir().unwrap();
    let verifier = FileVerifier::new(temp.path());
    
    let dummy_hash = Md5Hash::new([0u8; 16]);
    let status = verifier.determine_file_status(
        "nonexistent.dat",
        &dummy_hash,
        &dummy_hash,
        1024,
    );
    
    assert_eq!(status, FileStatus::Missing);
}

#[test]
fn test_determine_file_status_wrong_size() {
    // Test file with incorrect size
    let temp = tempdir().unwrap();
    let verifier = FileVerifier::new(temp.path());
    
    let content = b"this is exactly 30 bytes!!";
    create_test_file(temp.path(), "wrong_size.dat", content);
    
    let md5_16k = compute_md5(&content[..content.len().min(16384)]);
    let md5_full = compute_md5(content);
    
    // Report wrong expected size
    let status = verifier.determine_file_status(
        "wrong_size.dat",
        &md5_16k,
        &md5_full,
        1024, // Wrong size
    );
    
    assert_eq!(status, FileStatus::Corrupted);
}

#[test]
fn test_determine_file_status_corrupted_16k_hash() {
    // Test file with corrupted 16KB hash (doesn't match)
    let temp = tempdir().unwrap();
    let verifier = FileVerifier::new(temp.path());
    
    let content = b"correct content but wrong hash expected";
    create_test_file(temp.path(), "bad_16k.dat", content);
    
    let wrong_hash = Md5Hash::new([0xFF; 16]); // Wrong hash
    let md5_full = compute_md5(content);
    
    let status = verifier.determine_file_status(
        "bad_16k.dat",
        &wrong_hash, // Wrong 16KB hash
        &md5_full,
        content.len() as u64,
    );
    
    assert_eq!(status, FileStatus::Corrupted);
}

#[test]
fn test_determine_file_status_corrupted_full_hash() {
    // Test file with correct 16KB hash but corrupted full hash
    let temp = tempdir().unwrap();
    
    let config = VerificationConfig {
        parallel: false,
        skip_full_file_md5: false, // Must check full hash
        threads: 1,
    };
    let verifier = FileVerifier::with_config(temp.path(), &config);
    
    let content = b"16KB matches but full hash doesn't";
    create_test_file(temp.path(), "bad_full.dat", content);
    
    let md5_16k = compute_md5(&content[..content.len().min(16384)]);
    let wrong_full_hash = Md5Hash::new([0xFF; 16]); // Wrong full hash
    
    let status = verifier.determine_file_status(
        "bad_full.dat",
        &md5_16k,
        &wrong_full_hash, // Wrong full hash
        content.len() as u64,
    );
    
    assert_eq!(status, FileStatus::Corrupted);
}

#[test]
fn test_determine_file_status_present() {
    // Test correct file detection
    let temp = tempdir().unwrap();
    let verifier = FileVerifier::new(temp.path());
    
    let content = b"perfect file with correct hashes";
    create_test_file(temp.path(), "perfect.dat", content);
    
    let md5_16k = compute_md5(&content[..content.len().min(16384)]);
    let md5_full = compute_md5(content);
    
    let status = verifier.determine_file_status(
        "perfect.dat",
        &md5_16k,
        &md5_full,
        content.len() as u64,
    );
    
    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_determine_file_status_present_with_skip_full_md5() {
    // Test that skip_full_md5 works correctly
    let temp = tempdir().unwrap();
    
    let config = VerificationConfig {
        parallel: false,
        skip_full_file_md5: true, // Skip full MD5 check
        threads: 1,
    };
    let verifier = FileVerifier::with_config(temp.path(), &config);
    
    let content = b"file where we only check 16KB hash";
    create_test_file(temp.path(), "skip_check.dat", content);
    
    let md5_16k = compute_md5(&content[..content.len().min(16384)]);
    let wrong_full_hash = Md5Hash::new([0xFF; 16]); // Wrong but should be ignored
    
    let status = verifier.determine_file_status(
        "skip_check.dat",
        &md5_16k,
        &wrong_full_hash, // Should be ignored due to skip_full_md5
        content.len() as u64,
    );
    
    // Should be Present because 16KB matches and we're skipping full hash
    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verify_file_from_description() {
    // Test verification using FileDescriptionPacket
    let temp = tempdir().unwrap();
    let verifier = FileVerifier::new(temp.path());
    
    // Use actual PAR2 test file
    let par2_file = PathBuf::from("tests/fixtures/testfile.par2");
    assert!(par2_file.exists(), "Test fixture not found");
    
    let mut file = std::fs::File::open(&par2_file).unwrap();
    let packets = parse_packets(&mut file);
    
    // Find first file description packet
    let file_desc = packets.iter().find_map(|p| {
        if let par2rs::packets::Packet::FileDescription(fd) = p {
            Some(fd)
        } else {
            None
        }
    }).unwrap();
    
    // Create a file with correct content (for this test, just check Missing status)
    // Note: We don't have the original file content, so expect Missing
    let status = verifier.verify_file_from_description(file_desc);
    
    // File should be missing since we didn't create it
    assert_eq!(status, FileStatus::Missing);
}

#[test]
fn test_verify_file_from_description_present() {
    // Test verify_file_from_description when file is present
    let temp = tempdir().unwrap();
    let verifier = FileVerifier::new(temp.path());
    
    let content = b"test file content for file description";
    let file_name = "testfile.dat";
    create_test_file(temp.path(), file_name, content);
    
    let md5_16k = compute_md5(&content[..content.len().min(16384)]);
    let md5_full = compute_md5(content);
    
    // Create a mock FileDescriptionPacket using helper
    let file_desc = create_mock_file_desc(
        file_name,
        md5_16k,
        md5_full,
        content.len() as u64,
    );
    
    let status = verifier.verify_file_from_description(&file_desc);
    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verify_file_with_progress_missing() {
    // Test verify_file_with_progress when file is missing
    use par2rs::checksum::ProgressReporter;
    
    struct NoOpReporter;
    impl ProgressReporter for NoOpReporter {
        fn report_scanning_progress(&self, _file_name: &str, _bytes_processed: u64, _total_bytes: u64) {}
        fn clear_progress_line(&self) {}
    }
    
    let temp = tempdir().unwrap();
    let verifier = FileVerifier::new(temp.path());
    
    let file_desc = create_mock_file_desc(
        "missing.dat",
        Md5Hash::new([0u8; 16]),
        Md5Hash::new([0u8; 16]),
        1024,
    );
    
    let reporter = NoOpReporter;
    let result = verifier.verify_file_with_progress(&file_desc, &reporter);
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), FileStatus::Missing);
}

#[test]
fn test_verify_file_with_progress_present() {
    // Test verify_file_with_progress when file is present
    use par2rs::checksum::ProgressReporter;
    
    struct NoOpReporter;
    impl ProgressReporter for NoOpReporter {
        fn report_scanning_progress(&self, _file_name: &str, _bytes_processed: u64, _total_bytes: u64) {}
        fn clear_progress_line(&self) {}
    }
    
    let temp = tempdir().unwrap();
    let verifier = FileVerifier::new(temp.path());
    
    let content = b"file content for progress test";
    let file_name = "progress.dat";
    create_test_file(temp.path(), file_name, content);
    
    let md5_16k = compute_md5(&content[..content.len().min(16384)]);
    let md5_full = compute_md5(content);
    
    let file_desc = create_mock_file_desc(
        file_name,
        md5_16k,
        md5_full,
        content.len() as u64,
    );
    
    let reporter = NoOpReporter;
    let result = verifier.verify_file_with_progress(&file_desc, &reporter);
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), FileStatus::Present);
}

#[test]
fn test_verify_file_with_progress_corrupted() {
    // Test verify_file_with_progress when file is corrupted
    use par2rs::checksum::ProgressReporter;
    
    struct NoOpReporter;
    impl ProgressReporter for NoOpReporter {
        fn report_scanning_progress(&self, _file_name: &str, _bytes_processed: u64, _total_bytes: u64) {}
        fn clear_progress_line(&self) {}
    }
    
    let temp = tempdir().unwrap();
    let verifier = FileVerifier::new(temp.path());
    
    let content = b"corrupted file content";
    let file_name = "corrupted.dat";
    create_test_file(temp.path(), file_name, content);
    
    let wrong_hash = Md5Hash::new([0xFF; 16]);
    
    let file_desc = create_mock_file_desc(
        file_name,
        wrong_hash,
        wrong_hash,
        content.len() as u64,
    );
    
    let reporter = NoOpReporter;
    let result = verifier.verify_file_with_progress(&file_desc, &reporter);
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), FileStatus::Corrupted);
}

#[test]
fn test_large_file_16k_optimization() {
    // Test that 16KB optimization works for large files
    let temp = tempdir().unwrap();
    let verifier = FileVerifier::new(temp.path());
    
    // Create a 20KB file
    let mut content = vec![0u8; 20 * 1024];
    for i in 0..content.len() {
        content[i] = (i % 256) as u8;
    }
    
    create_test_file(temp.path(), "large.dat", &content);
    
    let md5_16k = compute_md5(&content[..16384]);
    let md5_full = compute_md5(&content);
    
    let status = verifier.determine_file_status(
        "large.dat",
        &md5_16k,
        &md5_full,
        content.len() as u64,
    );
    
    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_small_file_no_16k_optimization() {
    // Test files smaller than 16KB
    let temp = tempdir().unwrap();
    let verifier = FileVerifier::new(temp.path());
    
    let content = b"small file under 16KB";
    create_test_file(temp.path(), "small.dat", content);
    
    let md5_16k = compute_md5(content); // Entire file
    let md5_full = compute_md5(content);
    
    let status = verifier.determine_file_status(
        "small.dat",
        &md5_16k,
        &md5_full,
        content.len() as u64,
    );
    
    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_check_file_existence_error_path() {
    // Test error handling in check_file_existence
    let temp = tempdir().unwrap();
    let verifier = FileVerifier::new(temp.path());
    
    // Try to verify a file in a nonexistent directory
    let status = verifier.determine_file_status(
        "nonexistent_dir/file.dat",
        &Md5Hash::new([0u8; 16]),
        &Md5Hash::new([0u8; 16]),
        1024,
    );
    
    assert_eq!(status, FileStatus::Missing);
}

#[test]
fn test_check_file_size_metadata_error() {
    // Test metadata access errors
    let temp = tempdir().unwrap();
    let verifier = FileVerifier::new(temp.path());
    
    let content = b"test content";
    let file_path = create_test_file(temp.path(), "test.dat", content);
    
    // Make file inaccessible by removing read permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&file_path).unwrap().permissions();
        perms.set_mode(0o000); // No permissions
        fs::set_permissions(&file_path, perms).unwrap();
        
        let status = verifier.determine_file_status(
            "test.dat",
            &Md5Hash::new([0u8; 16]),
            &Md5Hash::new([0u8; 16]),
            content.len() as u64,
        );
        
        // Should be Corrupted due to metadata access error
        assert_eq!(status, FileStatus::Corrupted);
        
        // Restore permissions for cleanup
        let mut perms = fs::metadata(&file_path).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&file_path, perms).unwrap();
    }
}
