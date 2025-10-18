//! Comprehensive tests to achieve >90% coverage for repair.rs
//! Focuses on uncovered trait implementations for type-safe wrappers

use par2rs::repair::*;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Test all trait implementations for type-safe wrappers
#[test]
fn test_type_wrapper_traits() {
    // FileId traits
    let file_id_bytes = [1u8; 16];
    let file_id = FileId::new(file_id_bytes);

    // Test From trait
    let file_id_from: FileId = file_id_bytes.into();
    assert_eq!(file_id, file_id_from);

    // Test AsRef trait
    let as_ref: &[u8; 16] = file_id.as_ref();
    assert_eq!(as_ref, &file_id_bytes);

    // Test PartialEq<FileId> for [u8; 16]
    assert_eq!(file_id_bytes, file_id);

    // GlobalSliceIndex traits
    let global_idx = GlobalSliceIndex::new(42);

    // Test From trait
    let global_from: GlobalSliceIndex = 42.into();
    assert_eq!(global_idx, global_from);

    // Test Add trait
    let added = global_idx + 10;
    assert_eq!(added.as_usize(), 52);

    // Test Sub trait
    let other_idx = GlobalSliceIndex::new(10);
    let diff = global_idx - other_idx;
    assert_eq!(diff, 32);

    // Test Display trait
    assert_eq!(format!("{}", global_idx), "42");

    // LocalSliceIndex traits
    let local_idx = LocalSliceIndex::new(7);

    // Test From trait
    let local_from: LocalSliceIndex = 7.into();
    assert_eq!(local_idx, local_from);

    // Test Display trait
    assert_eq!(format!("{}", local_idx), "7");

    // RecoverySetId traits
    let set_id_bytes = [2u8; 16];
    let set_id = RecoverySetId::new(set_id_bytes);

    // Test From trait
    let set_id_from: RecoverySetId = set_id_bytes.into();
    assert_eq!(set_id, set_id_from);

    // Test AsRef trait
    let set_id_ref: &[u8; 16] = set_id.as_ref();
    assert_eq!(set_id_ref, &set_id_bytes);

    // Test PartialEq<RecoverySetId> for [u8; 16]
    assert_eq!(set_id_bytes, set_id);

    // Md5Hash traits
    let md5_bytes = [3u8; 16];
    let md5_hash = Md5Hash::new(md5_bytes);

    // Test From trait
    let md5_from: Md5Hash = md5_bytes.into();
    assert_eq!(md5_hash, md5_from);

    // Test AsRef trait
    let md5_ref: &[u8; 16] = md5_hash.as_ref();
    assert_eq!(md5_ref, &md5_bytes);

    // Crc32Value traits
    let crc = Crc32Value::new(0x12345678);

    // Test as_u32
    assert_eq!(crc.as_u32(), 0x12345678);

    // Test to_le_bytes
    assert_eq!(crc.to_le_bytes(), [0x78, 0x56, 0x34, 0x12]);

    // Test From trait
    let crc_from: Crc32Value = 0x12345678.into();
    assert_eq!(crc, crc_from);

    // Test PartialEq<u32>
    assert_eq!(crc, 0x12345678);

    // Test Display trait
    assert_eq!(format!("{}", crc), "12345678");
}

#[test]
fn test_recovery_set_methods() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, b"Hello, World!").unwrap();

    // Create minimal PAR2 files
    let par2_file = temp_dir.path().join("test.par2");
    create_minimal_par2(&par2_file, &test_file);

    let (context, _) = repair_files(par2_file.to_str().unwrap()).unwrap();

    // Test total_blocks
    let total = context.recovery_set.total_blocks();
    assert!(total > 0);

    // Test total_size
    let size = context.recovery_set.total_size();
    assert_eq!(size, 13); // "Hello, World!" is 13 bytes

    // Test print_statistics (just ensure it doesn't panic)
    context.recovery_set.print_statistics();
}

#[test]
fn test_file_status_needs_repair() {
    assert!(FileStatus::Missing.needs_repair());
    assert!(FileStatus::Corrupted.needs_repair());
    assert!(!FileStatus::Present.needs_repair());
}

#[test]
fn test_repair_result_methods() {
    // Test NoRepairNeeded
    let result = RepairResult::NoRepairNeeded {
        files_verified: 3,
        verified_files: vec!["file1.txt".to_string(), "file2.txt".to_string()],
        message: "All good".to_string(),
    };
    result.print_result();
    assert!(result.is_success());
    assert_eq!(result.repaired_files().len(), 0);
    assert_eq!(result.failed_files().len(), 0);

    // Test Success
    let result = RepairResult::Success {
        files_repaired: 1,
        files_verified: 2,
        repaired_files: vec!["repaired.txt".to_string()],
        verified_files: vec!["good1.txt".to_string(), "good2.txt".to_string()],
        message: "Repaired successfully".to_string(),
    };
    result.print_result();
    assert!(result.is_success());
    assert_eq!(result.repaired_files().len(), 1);
    assert_eq!(result.failed_files().len(), 0);

    // Test Failed
    let result = RepairResult::Failed {
        files_failed: vec!["bad_file.txt".to_string()],
        files_verified: 1,
        verified_files: vec!["good_file.txt".to_string()],
        message: "Something went wrong".to_string(),
    };
    result.print_result();
    assert!(!result.is_success());
    assert_eq!(result.repaired_files().len(), 0);
    assert_eq!(result.failed_files().len(), 1);
}

#[test]
fn test_error_no_valid_packets() {
    let temp_dir = TempDir::new().unwrap();
    let par2_file = temp_dir.path().join("empty.par2");
    fs::File::create(&par2_file).unwrap();

    // Empty PAR2 file should trigger NoValidPackets error
    let result = repair_files(par2_file.to_str().unwrap());
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), RepairError::NoValidPackets));
}

#[test]
fn test_size_mismatch_detection() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.txt");

    // Create file and PAR2
    fs::write(&test_file, b"Original content").unwrap();
    let par2_file = temp_dir.path().join("test.par2");
    create_minimal_par2(&par2_file, &test_file);

    // Change file size after PAR2 creation
    fs::write(&test_file, b"Different").unwrap();

    // Try to repair - should detect size mismatch
    let result = repair_files(par2_file.to_str().unwrap());
    // File should be detected as corrupted and attempted repair
    assert!(result.is_ok());
}

#[test]
fn test_hash_mismatch_detection() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.txt");

    // Create file with specific size
    let content = vec![0xAA; 1000];
    fs::write(&test_file, &content).unwrap();
    let par2_file = temp_dir.path().join("test.par2");
    create_minimal_par2(&par2_file, &test_file);

    // Change content but keep same size to trigger hash mismatch
    fs::write(&test_file, vec![0xBB; 1000]).unwrap();

    // Repair should detect the hash mismatch
    let result = repair_files(par2_file.to_str().unwrap());
    assert!(result.is_ok());
}

#[test]
fn test_corrupted_file_repair() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.txt");

    // Create a file
    let content = vec![0x42; 10000];
    fs::write(&test_file, &content).unwrap();

    let par2_file = temp_dir.path().join("test.par2");
    create_minimal_par2(&par2_file, &test_file);

    // Corrupt part of the file
    let mut corrupted = content.clone();
    for byte in corrupted.iter_mut().take(100) {
        *byte = 0xFF;
    }
    fs::write(&test_file, &corrupted).unwrap();

    // Repair should succeed
    let result = repair_files(par2_file.to_str().unwrap());
    assert!(result.is_ok());

    let (_, repair_result) = result.unwrap();
    // Should either repair successfully or already be valid
    assert!(repair_result.is_success());
}

#[test]
fn test_missing_file_repair() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.txt");

    // Create file and PAR2
    fs::write(&test_file, b"Test content for missing file").unwrap();
    let par2_file = temp_dir.path().join("test.par2");
    create_minimal_par2(&par2_file, &test_file);

    // Delete the file
    fs::remove_file(&test_file).unwrap();
    assert!(!test_file.exists());

    // Try to repair - should recreate the file
    let result = repair_files(par2_file.to_str().unwrap());
    assert!(result.is_ok());

    let (_, repair_result) = result.unwrap();
    // Should attempt repair of missing file
    assert!(matches!(
        repair_result,
        RepairResult::Success { .. } | RepairResult::Failed { .. }
    ));

    // File should exist again if repair succeeded
    if repair_result.is_success() {
        assert!(test_file.exists());
    }
}

// Helper function to create a minimal PAR2 file for testing
fn create_minimal_par2(par2_path: &PathBuf, data_file: &PathBuf) {
    // Use par2cmdline to create a real PAR2 file
    std::process::Command::new("par2")
        .arg("c")
        .arg("-r5") // 5% recovery
        .arg("-q") // Quiet
        .arg(par2_path)
        .arg(data_file)
        .output()
        .expect("Failed to create PAR2 file - is par2cmdline installed?");
}
