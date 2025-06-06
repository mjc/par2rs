//! Tests for file_verification module
//!
//! This module tests file verification, MD5 calculation,
//! and verification result handling functionality.

use par2rs::file_verification::*;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;

#[test]
fn test_format_display_name() {
    // Test normal filename
    let short_name = format_display_name("testfile.txt");
    assert_eq!(short_name, "testfile.txt");

    // Test very long filename (should be truncated)
    let long_name = "a".repeat(60);
    let formatted = format_display_name(&long_name);
    assert!(formatted.len() <= 50);
    assert!(formatted.ends_with("..."));

    // Test path with directory
    let path_name = format_display_name("/path/to/testfile.txt");
    assert_eq!(path_name, "testfile.txt");

    // Test edge case - exactly 50 characters
    let exact_name = "a".repeat(50);
    let formatted = format_display_name(&exact_name);
    assert_eq!(formatted.len(), 50);
    assert!(!formatted.ends_with("..."));
}

#[test]
fn test_calculate_file_md5() {
    let test_file = Path::new("tests/fixtures/testfile");

    let md5_result = calculate_file_md5(test_file);
    assert!(md5_result.is_ok());

    let md5_hash = md5_result.unwrap();
    assert_eq!(md5_hash.len(), 16);
    assert_ne!(md5_hash, [0; 16]); // Should not be all zeros
}

#[test]
fn test_calculate_file_md5_nonexistent() {
    let nonexistent_file = Path::new("tests/fixtures/nonexistent_file");

    let md5_result = calculate_file_md5(nonexistent_file);
    assert!(md5_result.is_err());
}

#[test]
fn test_verify_single_file_existing() {
    let test_file = Path::new("tests/fixtures/testfile");

    // Calculate the actual MD5 of the test file
    let expected_md5 = calculate_file_md5(test_file).unwrap();

    // Verification should succeed with correct MD5
    assert!(verify_single_file("tests/fixtures/testfile", expected_md5));

    // Verification should fail with incorrect MD5
    let wrong_md5 = [0; 16];
    assert!(!verify_single_file("tests/fixtures/testfile", wrong_md5));
}

#[test]
fn test_verify_single_file_nonexistent() {
    let dummy_md5 = [0; 16];

    // Should return false for nonexistent file
    assert!(!verify_single_file(
        "tests/fixtures/nonexistent_file",
        dummy_md5
    ));
}

#[test]
fn test_verify_files_and_collect_results() {
    // Create test file info
    let test_file = Path::new("tests/fixtures/testfile");
    let actual_md5 = calculate_file_md5(test_file).unwrap();

    let mut file_info = HashMap::new();
    let file_id = [1; 16];
    let file_length = 1048576u64;

    file_info.insert(
        "tests/fixtures/testfile".to_string(),
        (file_id, actual_md5, file_length),
    );

    // Add a nonexistent file
    file_info.insert(
        "tests/fixtures/nonexistent".to_string(),
        ([2; 16], [0; 16], 1000u64),
    );

    let results = verify_files_and_collect_results(&file_info, false);

    assert_eq!(results.len(), 2);

    // Find results for each file
    let existing_result = results
        .iter()
        .find(|r| r.file_name == "tests/fixtures/testfile")
        .unwrap();
    let missing_result = results
        .iter()
        .find(|r| r.file_name == "tests/fixtures/nonexistent")
        .unwrap();

    // Existing file should be valid
    assert!(existing_result.is_valid);
    assert!(existing_result.exists);
    assert_eq!(existing_result.file_id, file_id);
    assert_eq!(existing_result.expected_md5, actual_md5);

    // Missing file should be invalid
    assert!(!missing_result.is_valid);
    assert!(!missing_result.exists);
}

#[test]
fn test_verify_files_with_progress() {
    let mut file_info = HashMap::new();
    let test_file = Path::new("tests/fixtures/testfile");
    let actual_md5 = calculate_file_md5(test_file).unwrap();

    file_info.insert(
        "tests/fixtures/testfile".to_string(),
        ([1; 16], actual_md5, 1048576u64),
    );

    // Test with progress enabled (should not panic)
    let results = verify_files_and_collect_results(&file_info, true);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_valid);
}

#[test]
fn test_find_broken_file_descriptors() {
    // Create mock packets
    let main_file = Path::new("tests/fixtures/testfile.par2");
    let mut file = fs::File::open(main_file).unwrap();
    let packets = par2rs::parse_packets(&mut file);

    // Get a real file ID from the packets
    let file_id = if let Some(par2rs::Packet::FileDescription(fd)) = packets
        .iter()
        .find(|p| matches!(p, par2rs::Packet::FileDescription(_)))
    {
        fd.file_id
    } else {
        panic!("No FileDescription packet found");
    };

    let broken_file_ids = vec![file_id];

    let broken_descriptors = find_broken_file_descriptors(packets, &broken_file_ids);

    // Should find the broken file descriptor
    assert_eq!(broken_descriptors.len(), 1);
    assert!(matches!(
        broken_descriptors[0],
        par2rs::Packet::FileDescription(_)
    ));
}

#[test]
fn test_find_broken_file_descriptors_none_broken() {
    let main_file = Path::new("tests/fixtures/testfile.par2");
    let mut file = fs::File::open(main_file).unwrap();
    let packets = par2rs::parse_packets(&mut file);

    let broken_file_ids = vec![[255; 16]]; // Non-existent file ID

    let broken_descriptors = find_broken_file_descriptors(packets, &broken_file_ids);

    // Should find no broken descriptors
    assert!(broken_descriptors.is_empty());
}

#[test]
fn test_file_verification_result_struct() {
    let result = FileVerificationResult {
        file_name: "test.txt".to_string(),
        file_id: [1; 16],
        expected_md5: [2; 16],
        is_valid: true,
        exists: true,
    };

    // Test that struct can be cloned and debugged
    let cloned_result = result.clone();
    assert_eq!(result.file_name, cloned_result.file_name);
    assert_eq!(result.is_valid, cloned_result.is_valid);

    let debug_output = format!("{:?}", result);
    assert!(debug_output.contains("test.txt"));
}

#[test]
fn test_verify_files_empty_input() {
    let file_info = HashMap::new();

    let results = verify_files_and_collect_results(&file_info, false);

    assert!(results.is_empty());
}

#[test]
fn test_md5_consistency() {
    let test_file = Path::new("tests/fixtures/testfile");

    // Calculate MD5 multiple times
    let md5_1 = calculate_file_md5(test_file).unwrap();
    let md5_2 = calculate_file_md5(test_file).unwrap();

    // Should be identical
    assert_eq!(md5_1, md5_2);
}

#[test]
fn test_verify_file_with_corrupted_content() {
    // Create a temporary file with known content
    let temp_file = std::env::temp_dir().join("test_corrupted.txt");

    // Write initial content
    let original_content = b"Hello, World!";
    fs::write(&temp_file, original_content).unwrap();

    // Calculate MD5 of original content
    let original_md5 = calculate_file_md5(&temp_file).unwrap();

    // Verify with correct MD5
    assert!(verify_single_file(
        temp_file.to_str().unwrap(),
        original_md5
    ));

    // Modify the file
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&temp_file)
        .unwrap();
    file.write_all(b"Modified content").unwrap();
    drop(file);

    // Verify with original MD5 (should fail)
    assert!(!verify_single_file(
        temp_file.to_str().unwrap(),
        original_md5
    ));

    // Clean up
    let _ = fs::remove_file(&temp_file);
}
