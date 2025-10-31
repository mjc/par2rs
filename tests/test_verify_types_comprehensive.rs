use par2rs::domain::{Crc32Value, FileId, Md5Hash};
use par2rs::verify::{
    BlockVerificationResult, FileStatus, FileVerificationResult, VerificationResults,
};

// FileStatus tests
#[test]
fn test_file_status_present() {
    let status = FileStatus::Present;
    assert!(!status.needs_repair());
    assert_eq!(status.to_string(), "present");
}

#[test]
fn test_file_status_renamed() {
    let status = FileStatus::Renamed;
    assert!(!status.needs_repair());
    assert_eq!(status.to_string(), "renamed");
}

#[test]
fn test_file_status_corrupted() {
    let status = FileStatus::Corrupted;
    assert!(status.needs_repair());
    assert_eq!(status.to_string(), "corrupted");
}

#[test]
fn test_file_status_missing() {
    let status = FileStatus::Missing;
    assert!(status.needs_repair());
    assert_eq!(status.to_string(), "missing");
}

#[test]
fn test_file_status_eq() {
    assert_eq!(FileStatus::Present, FileStatus::Present);
    assert_eq!(FileStatus::Missing, FileStatus::Missing);
    assert_ne!(FileStatus::Present, FileStatus::Missing);
}

#[test]
fn test_file_status_clone() {
    let status1 = FileStatus::Corrupted;
    let status2 = status1.clone();
    assert_eq!(status1, status2);
}

#[test]
fn test_file_status_debug() {
    let status = FileStatus::Present;
    let debug_str = format!("{:?}", status);
    assert!(debug_str.contains("Present"));
}

// BlockVerificationResult tests
#[test]
fn test_block_verification_result_creation() {
    let file_id = FileId::new([1; 16]);
    let hash = Md5Hash::new([2; 16]);
    let crc = Crc32Value::new(12345);

    let result = BlockVerificationResult {
        block_number: 5,
        file_id,
        is_valid: true,
        expected_hash: Some(hash),
        expected_crc: Some(crc),
    };

    assert_eq!(result.block_number, 5);
    assert_eq!(result.file_id, file_id);
    assert!(result.is_valid);
    assert_eq!(result.expected_hash, Some(hash));
    assert_eq!(result.expected_crc, Some(crc));
}

#[test]
fn test_block_verification_result_no_checksums() {
    let file_id = FileId::new([3; 16]);

    let result = BlockVerificationResult {
        block_number: 0,
        file_id,
        is_valid: false,
        expected_hash: None,
        expected_crc: None,
    };

    assert_eq!(result.block_number, 0);
    assert!(!result.is_valid);
    assert!(result.expected_hash.is_none());
    assert!(result.expected_crc.is_none());
}

#[test]
fn test_block_verification_result_clone() {
    let file_id = FileId::new([4; 16]);
    let result1 = BlockVerificationResult {
        block_number: 10,
        file_id,
        is_valid: true,
        expected_hash: None,
        expected_crc: None,
    };

    let result2 = result1.clone();
    assert_eq!(result1.block_number, result2.block_number);
    assert_eq!(result1.is_valid, result2.is_valid);
}

// FileVerificationResult tests
#[test]
fn test_file_verification_result_creation() {
    let file_id = FileId::new([5; 16]);

    let result = FileVerificationResult {
        file_name: "test.txt".to_string(),
        file_id,
        status: FileStatus::Present,
        blocks_available: 10,
        total_blocks: 10,
        damaged_blocks: vec![],
    };

    assert_eq!(result.file_name, "test.txt");
    assert_eq!(result.status, FileStatus::Present);
    assert_eq!(result.blocks_available, 10);
    assert_eq!(result.total_blocks, 10);
    assert!(result.damaged_blocks.is_empty());
}

#[test]
fn test_file_verification_result_damaged() {
    let file_id = FileId::new([6; 16]);

    let result = FileVerificationResult {
        file_name: "damaged.bin".to_string(),
        file_id,
        status: FileStatus::Corrupted,
        blocks_available: 8,
        total_blocks: 10,
        damaged_blocks: vec![3, 7],
    };

    assert_eq!(result.status, FileStatus::Corrupted);
    assert_eq!(result.blocks_available, 8);
    assert_eq!(result.total_blocks, 10);
    assert_eq!(result.damaged_blocks, vec![3, 7]);
}

#[test]
fn test_file_verification_result_clone() {
    let file_id = FileId::new([7; 16]);
    let result1 = FileVerificationResult {
        file_name: "clone_test.txt".to_string(),
        file_id,
        status: FileStatus::Missing,
        blocks_available: 0,
        total_blocks: 5,
        damaged_blocks: vec![0, 1, 2, 3, 4],
    };

    let result2 = result1.clone();
    assert_eq!(result1.file_name, result2.file_name);
    assert_eq!(result1.status, result2.status);
    assert_eq!(result1.damaged_blocks, result2.damaged_blocks);
}

// VerificationResults tests
#[test]
fn test_verification_results_all_ok() {
    let results = VerificationResults {
        files: vec![],
        blocks: vec![],
        present_file_count: 5,
        renamed_file_count: 0,
        corrupted_file_count: 0,
        missing_file_count: 0,
        available_block_count: 100,
        missing_block_count: 0,
        total_block_count: 100,
        recovery_blocks_available: 20,
        repair_possible: false,
        blocks_needed_for_repair: 0,
    };

    let display = results.to_string();
    assert!(display.contains("5 file(s) are ok."));
    assert!(display.contains("All files are correct"));
}

#[test]
fn test_verification_results_repair_needed_possible() {
    let results = VerificationResults {
        files: vec![],
        blocks: vec![],
        present_file_count: 3,
        renamed_file_count: 0,
        corrupted_file_count: 1,
        missing_file_count: 1,
        available_block_count: 80,
        missing_block_count: 20,
        total_block_count: 100,
        recovery_blocks_available: 30,
        repair_possible: true,
        blocks_needed_for_repair: 20,
    };

    let display = results.to_string();
    assert!(display.contains("3 file(s) are ok."));
    assert!(display.contains("1 file(s) exist but are damaged."));
    assert!(display.contains("1 file(s) are missing."));
    assert!(display.contains("Repair is possible."));
    assert!(display.contains("80 out of 100 data blocks available"));
    assert!(display.contains("30 recovery blocks available"));
    assert!(display.contains("excess of 10 recovery blocks"));
    assert!(display.contains("20 recovery blocks will be used"));
}

#[test]
fn test_verification_results_repair_not_possible() {
    let results = VerificationResults {
        files: vec![],
        blocks: vec![],
        present_file_count: 2,
        renamed_file_count: 0,
        corrupted_file_count: 2,
        missing_file_count: 0,
        available_block_count: 70,
        missing_block_count: 30,
        total_block_count: 100,
        recovery_blocks_available: 10,
        repair_possible: false,
        blocks_needed_for_repair: 30,
    };

    let display = results.to_string();
    assert!(display.contains("Repair is not possible."));
    assert!(display.contains("20 more recovery blocks"));
}

#[test]
fn test_verification_results_renamed_files() {
    let results = VerificationResults {
        files: vec![],
        blocks: vec![],
        present_file_count: 3,
        renamed_file_count: 2,
        corrupted_file_count: 0,
        missing_file_count: 0,
        available_block_count: 100,
        missing_block_count: 0,
        total_block_count: 100,
        recovery_blocks_available: 0,
        repair_possible: false,
        blocks_needed_for_repair: 0,
    };

    let display = results.to_string();
    assert!(display.contains("3 file(s) are ok."));
    assert!(display.contains("2 file(s) have the wrong name."));
}

#[test]
fn test_verification_results_exact_recovery_blocks() {
    let results = VerificationResults {
        files: vec![],
        blocks: vec![],
        present_file_count: 1,
        renamed_file_count: 0,
        corrupted_file_count: 1,
        missing_file_count: 0,
        available_block_count: 90,
        missing_block_count: 10,
        total_block_count: 100,
        recovery_blocks_available: 10,
        repair_possible: true,
        blocks_needed_for_repair: 10,
    };

    let display = results.to_string();
    assert!(display.contains("Repair is possible."));
    assert!(display.contains("10 recovery blocks will be used"));
    // Should not mention excess
    assert!(!display.contains("excess"));
}

#[test]
fn test_verification_results_no_recovery_blocks() {
    let results = VerificationResults {
        files: vec![],
        blocks: vec![],
        present_file_count: 5,
        renamed_file_count: 0,
        corrupted_file_count: 0,
        missing_file_count: 0,
        available_block_count: 100,
        missing_block_count: 0,
        total_block_count: 100,
        recovery_blocks_available: 0,
        repair_possible: false,
        blocks_needed_for_repair: 0,
    };

    let display = results.to_string();
    // Should not mention recovery blocks when there are none
    assert!(!display.contains("recovery blocks available"));
}

#[test]
fn test_verification_results_clone() {
    let results1 = VerificationResults {
        files: vec![],
        blocks: vec![],
        present_file_count: 1,
        renamed_file_count: 0,
        corrupted_file_count: 0,
        missing_file_count: 0,
        available_block_count: 10,
        missing_block_count: 0,
        total_block_count: 10,
        recovery_blocks_available: 5,
        repair_possible: false,
        blocks_needed_for_repair: 0,
    };

    let results2 = results1.clone();
    assert_eq!(results1.present_file_count, results2.present_file_count);
    assert_eq!(results1.total_block_count, results2.total_block_count);
}

#[test]
fn test_verification_results_debug() {
    let results = VerificationResults {
        files: vec![],
        blocks: vec![],
        present_file_count: 1,
        renamed_file_count: 0,
        corrupted_file_count: 0,
        missing_file_count: 0,
        available_block_count: 10,
        missing_block_count: 0,
        total_block_count: 10,
        recovery_blocks_available: 5,
        repair_possible: false,
        blocks_needed_for_repair: 0,
    };

    let debug_str = format!("{:?}", results);
    assert!(debug_str.contains("VerificationResults"));
}

#[test]
fn test_verification_results_with_file_data() {
    let file_id = FileId::new([8; 16]);
    let file_result = FileVerificationResult {
        file_name: "data.bin".to_string(),
        file_id,
        status: FileStatus::Present,
        blocks_available: 5,
        total_blocks: 5,
        damaged_blocks: vec![],
    };

    let block_result = BlockVerificationResult {
        block_number: 0,
        file_id,
        is_valid: true,
        expected_hash: None,
        expected_crc: None,
    };

    let results = VerificationResults {
        files: vec![file_result],
        blocks: vec![block_result],
        present_file_count: 1,
        renamed_file_count: 0,
        corrupted_file_count: 0,
        missing_file_count: 0,
        available_block_count: 5,
        missing_block_count: 0,
        total_block_count: 5,
        recovery_blocks_available: 2,
        repair_possible: false,
        blocks_needed_for_repair: 0,
    };

    assert_eq!(results.files.len(), 1);
    assert_eq!(results.blocks.len(), 1);
    assert_eq!(results.files[0].file_name, "data.bin");
}
