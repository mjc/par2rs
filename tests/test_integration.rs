//! Integration tests for shared modules
//!
//! These tests verify that the refactored modules work correctly together
//! and provide the same functionality as the original code.

use par2rs::{analysis, file_ops, file_verification};
use std::path::Path;

#[test]
fn test_complete_verification_workflow() {
    let main_file = Path::new("tests/fixtures/testfile.par2");

    // Step 1: Collect PAR2 files
    let par2_files = file_ops::collect_par2_files(main_file);
    assert!(!par2_files.is_empty());

    // Step 2: Load all packets
    let (packets, recovery_blocks) = file_ops::load_all_par2_packets(&par2_files, false);
    assert!(!packets.is_empty());
    assert!(recovery_blocks > 0);

    // Step 3: Calculate statistics
    let stats = analysis::calculate_par2_stats(&packets, recovery_blocks);
    assert_eq!(stats.file_count, 1);
    assert_eq!(stats.block_size, 528);
    assert_eq!(stats.total_size, 1048576);

    // Step 4: Collect file information
    let file_info = analysis::collect_file_info_from_packets(&packets);
    assert_eq!(file_info.len(), 1);
    assert!(file_info.contains_key("testfile"));

    // Step 5: Verify files
    let base_dir = Path::new("tests/fixtures");
    let verification_results = file_verification::verify_files_and_collect_results_with_base_dir(
        &file_info,
        false,
        Some(base_dir),
    );
    assert_eq!(verification_results.len(), 1);

    let result = &verification_results[0];
    assert_eq!(result.file_name, "testfile");
    assert!(result.is_valid);
    assert!(result.exists);
}

#[test]
fn test_verification_with_missing_file() {
    let main_file = Path::new("tests/fixtures/testfile.par2");

    // Load packets
    let par2_files = file_ops::collect_par2_files(main_file);
    let (packets, _) = file_ops::load_all_par2_packets(&par2_files, false);

    // Create file info with a non-existent file
    let mut file_info = analysis::collect_file_info_from_packets(&packets);
    file_info.insert(
        "nonexistent_file".to_string(),
        ([255; 16], [0; 16], 1000u64),
    );

    // Verify files
    let base_dir = Path::new("tests/fixtures");
    let verification_results = file_verification::verify_files_and_collect_results_with_base_dir(
        &file_info,
        false,
        Some(base_dir),
    );

    // Should have results for both existing and missing files
    assert_eq!(verification_results.len(), 2);

    let existing_result = verification_results
        .iter()
        .find(|r| r.file_name == "testfile")
        .unwrap();
    let missing_result = verification_results
        .iter()
        .find(|r| r.file_name == "nonexistent_file")
        .unwrap();

    assert!(existing_result.is_valid);
    assert!(!missing_result.is_valid);
    assert!(!missing_result.exists);
}

#[test]
fn test_packet_deduplication_across_files() {
    let main_file = Path::new("tests/fixtures/testfile.par2");
    let par2_files = file_ops::collect_par2_files(main_file);

    // Load packets with deduplication
    let (packets, _) = file_ops::load_all_par2_packets(&par2_files, false);

    // Count unique packets by hash
    let mut unique_hashes = std::collections::HashSet::new();
    for packet in &packets {
        let hash = file_ops::get_packet_hash(packet);
        assert!(unique_hashes.insert(hash), "Found duplicate packet");
    }

    // Should have main packet, file description, and recovery slices
    assert!(packets.iter().any(|p| matches!(p, par2rs::Packet::Main(_))));
    assert!(packets
        .iter()
        .any(|p| matches!(p, par2rs::Packet::FileDescription(_))));
    assert!(packets
        .iter()
        .any(|p| matches!(p, par2rs::Packet::RecoverySlice(_))));
}

#[test]
fn test_statistics_match_original_implementation() {
    let main_file = Path::new("tests/fixtures/testfile.par2");
    let par2_files = file_ops::collect_par2_files(main_file);
    let (packets, recovery_blocks) = file_ops::load_all_par2_packets(&par2_files, false);

    // Calculate statistics using the new modular approach
    let stats = analysis::calculate_par2_stats(&packets, recovery_blocks);

    // These values should match what the original par2verify outputs
    assert_eq!(stats.file_count, 1);
    assert_eq!(stats.block_size, 528);
    assert_eq!(stats.total_blocks, 1986);
    assert_eq!(stats.total_size, 1048576);

    // The recovery blocks should match the sum from all volume files
    let expected_recovery_blocks: usize = [1, 2, 4, 8, 16, 32, 36].iter().sum();
    assert_eq!(stats.recovery_blocks, expected_recovery_blocks);
}

#[test]
fn test_file_verification_matches_expected_behavior() {
    let main_file = Path::new("tests/fixtures/testfile.par2");
    let par2_files = file_ops::collect_par2_files(main_file);
    let (packets, _) = file_ops::load_all_par2_packets(&par2_files, false);

    let file_info = analysis::collect_file_info_from_packets(&packets);
    let base_dir = Path::new("tests/fixtures");
    let verification_results = file_verification::verify_files_and_collect_results_with_base_dir(
        &file_info,
        false,
        Some(base_dir),
    );

    // Should verify exactly one file
    assert_eq!(verification_results.len(), 1);

    let result = &verification_results[0];
    assert_eq!(result.file_name, "testfile");
    assert!(result.is_valid);
    assert!(result.exists);

    // The file ID and MD5 should be non-zero
    assert_ne!(result.file_id, [0; 16]);
    assert_ne!(result.expected_md5, [0; 16]);
}

#[test]
fn test_broken_file_identification() {
    let main_file = Path::new("tests/fixtures/testfile.par2");
    let par2_files = file_ops::collect_par2_files(main_file);
    let (packets, _) = file_ops::load_all_par2_packets(&par2_files, false);

    // Create a scenario with broken files
    let mut file_info = analysis::collect_file_info_from_packets(&packets);
    file_info.insert("broken_file".to_string(), ([123; 16], [255; 16], 5000u64));

    let base_dir = Path::new("tests/fixtures");
    let verification_results = file_verification::verify_files_and_collect_results_with_base_dir(
        &file_info,
        false,
        Some(base_dir),
    );

    // Collect broken file IDs
    let broken_file_ids: Vec<[u8; 16]> = verification_results
        .iter()
        .filter(|result| !result.is_valid)
        .map(|result| result.file_id)
        .collect();

    // Find broken file descriptors
    let broken_descriptors =
        file_verification::find_broken_file_descriptors(packets, &broken_file_ids);

    // Should not find any descriptors since our broken file doesn't exist in packets
    assert!(broken_descriptors.is_empty());
}

#[test]
fn test_module_apis_are_consistent() {
    let main_file = Path::new("tests/fixtures/testfile.par2");

    // Test that all module functions handle the same data types consistently
    let par2_files = file_ops::collect_par2_files(main_file);
    let (packets, recovery_blocks) = file_ops::load_all_par2_packets(&par2_files, false);

    // All analysis functions should work with the same packet slice
    let _filenames = analysis::extract_unique_filenames(&packets);
    let _stats = analysis::extract_main_packet_stats(&packets);
    let _total_size = analysis::calculate_total_size(&packets);
    let _file_info = analysis::collect_file_info_from_packets(&packets);
    let _par2_stats = analysis::calculate_par2_stats(&packets, recovery_blocks);

    // File operations should work consistently
    let _recovery_count = file_ops::count_recovery_blocks(&packets);

    // All functions should handle empty inputs gracefully
    let empty_packets = vec![];
    let _empty_filenames = analysis::extract_unique_filenames(&empty_packets);
    let _empty_stats = analysis::extract_main_packet_stats(&empty_packets);
    let _empty_size = analysis::calculate_total_size(&empty_packets);
    let _empty_recovery = file_ops::count_recovery_blocks(&empty_packets);
}

#[test]
fn test_end_to_end_verification_like_original() {
    // This test mimics the original par2verify workflow
    let main_file = Path::new("tests/fixtures/testfile.par2");

    // 1. Collect PAR2 files (like collect_par2_files)
    let par2_files = file_ops::collect_par2_files(main_file);

    // 2. Load all packets (like load_all_par2_packets)
    let (all_packets, total_recovery_blocks) = file_ops::load_all_par2_packets(&par2_files, false);

    // 3. Show summary statistics (like show_summary_stats)
    let stats = analysis::calculate_par2_stats(&all_packets, total_recovery_blocks);
    // analysis::print_summary_stats(&stats); // Would print in real usage

    // 4. Verify source files (like verify_source_files_with_progress)
    let file_info = analysis::collect_file_info_from_packets(&all_packets);
    let base_dir = Path::new("tests/fixtures");
    let verification_results = file_verification::verify_files_and_collect_results_with_base_dir(
        &file_info,
        false,
        Some(base_dir),
    );

    // 5. Handle verification results (like handle_verification_results)
    let broken_file_ids: Vec<[u8; 16]> = verification_results
        .iter()
        .filter(|result| !result.is_valid)
        .map(|result| result.file_id)
        .collect();

    let broken_descriptors =
        file_verification::find_broken_file_descriptors(all_packets, &broken_file_ids);

    // For the test fixture, all files should be valid
    assert!(broken_descriptors.is_empty());
    assert_eq!(stats.file_count, 1);
    assert!(stats.recovery_blocks > 0);
}
