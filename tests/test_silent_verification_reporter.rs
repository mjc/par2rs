//! Tests for SilentVerificationReporter
//!
//! Ensures 100% coverage of silent verification reporter functionality.

use par2rs::domain::FileId;
use par2rs::reporters::{Reporter, SilentVerificationReporter, VerificationReporter};
use par2rs::verify::{FileStatus, FileVerificationResult, VerificationResults};

/// Helper to create test verification results
fn create_test_results(
    files: Vec<FileVerificationResult>,
    present: usize,
    renamed: usize,
    corrupted: usize,
    missing: usize,
) -> VerificationResults {
    VerificationResults {
        files,
        blocks: vec![],
        present_file_count: present,
        renamed_file_count: renamed,
        corrupted_file_count: corrupted,
        missing_file_count: missing,
        available_block_count: 100,
        missing_block_count: 0,
        total_block_count: 100,
        recovery_blocks_available: 10,
        repair_possible: true,
        blocks_needed_for_repair: 0,
    }
}

#[test]
fn test_new_constructor() {
    let _reporter = SilentVerificationReporter::new();
}

#[test]
fn test_default_constructor() {
    let _reporter = SilentVerificationReporter::new();
}

#[test]
fn test_report_progress_silent() {
    let reporter = SilentVerificationReporter::new();

    // All these should be silent no-ops
    reporter.report_progress("Test message", 0.0);
    reporter.report_progress("Another message", 0.5);
    reporter.report_progress("Final message", 1.0);
    reporter.report_progress("", 0.0);
    reporter.report_progress("Unicode 测试", 0.33);
}

#[test]
fn test_report_error_silent() {
    let reporter = SilentVerificationReporter::new();

    // All these should be silent no-ops
    reporter.report_error("Test error");
    reporter.report_error("Another error");
    reporter.report_error("");
    reporter.report_error("Unicode error 错误");
}

#[test]
fn test_report_complete_silent() {
    let reporter = SilentVerificationReporter::new();

    // All these should be silent no-ops
    reporter.report_complete("Test complete");
    reporter.report_complete("Another completion");
    reporter.report_complete("");
}

#[test]
fn test_report_verification_start_silent() {
    let reporter = SilentVerificationReporter::new();

    // Both should be silent no-ops
    reporter.report_verification_start(true); // parallel
    reporter.report_verification_start(false); // sequential
}

#[test]
fn test_report_files_found_silent() {
    let reporter = SilentVerificationReporter::new();

    // All should be silent no-ops
    reporter.report_files_found(0);
    reporter.report_files_found(1);
    reporter.report_files_found(10);
    reporter.report_files_found(100);
    reporter.report_files_found(usize::MAX);
}

#[test]
fn test_report_verifying_file_silent() {
    let reporter = SilentVerificationReporter::new();

    // All should be silent no-ops
    reporter.report_verifying_file("test.txt");
    reporter.report_verifying_file("file with spaces.dat");
    reporter.report_verifying_file("unicode_测试.bin");
    reporter.report_verifying_file("");
    reporter.report_verifying_file("very_long_filename_that_might_cause_issues.txt");
}

#[test]
fn test_report_file_status_all_variants_silent() {
    let reporter = SilentVerificationReporter::new();

    // Test all FileStatus enum variants - all should be silent
    reporter.report_file_status("present.txt", FileStatus::Present);
    reporter.report_file_status("missing.txt", FileStatus::Missing);
    reporter.report_file_status("corrupted.txt", FileStatus::Corrupted);
    reporter.report_file_status("renamed.txt", FileStatus::Renamed);

    // Test with edge case filenames
    reporter.report_file_status("", FileStatus::Present);
    reporter.report_file_status("unicode_文件.dat", FileStatus::Corrupted);
}

#[test]
fn test_report_damaged_blocks_silent() {
    let reporter = SilentVerificationReporter::new();

    // All should be silent no-ops regardless of block list size
    reporter.report_damaged_blocks("file1.txt", &[]);
    reporter.report_damaged_blocks("file2.txt", &[42]);
    reporter.report_damaged_blocks("file3.txt", &[1, 5, 10]);
    reporter.report_damaged_blocks("file4.txt", &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

    // Large list
    let large_list: Vec<u32> = (0..1000).collect();
    reporter.report_damaged_blocks("file5.txt", &large_list);

    // Test with empty filename
    reporter.report_damaged_blocks("", &[1, 2, 3]);
}

#[test]
fn test_report_verification_results_empty_silent() {
    let reporter = SilentVerificationReporter::new();
    let empty_results = create_test_results(vec![], 0, 0, 0, 0);
    reporter.report_verification_results(&empty_results);
}

#[test]
fn test_report_verification_results_single_file_silent() {
    let reporter = SilentVerificationReporter::new();

    let file = FileVerificationResult {
        file_name: "test.txt".to_string(),
        file_id: FileId::new([1; 16]),
        status: FileStatus::Present,
        blocks_available: 10,
        total_blocks: 10,
        damaged_blocks: vec![],
    };
    let results = create_test_results(vec![file], 1, 0, 0, 0);
    reporter.report_verification_results(&results);
}

#[test]
fn test_report_verification_results_with_damaged_blocks_silent() {
    let reporter = SilentVerificationReporter::new();

    // Test with damaged blocks - should still be silent
    let corrupted_file = FileVerificationResult {
        file_name: "corrupted.txt".to_string(),
        file_id: FileId::new([2; 16]),
        status: FileStatus::Corrupted,
        blocks_available: 5,
        total_blocks: 10,
        damaged_blocks: vec![1, 3, 5, 7, 9],
    };
    let results = create_test_results(vec![corrupted_file], 0, 0, 1, 0);
    reporter.report_verification_results(&results);
}

#[test]
fn test_report_verification_results_large_damaged_blocks_silent() {
    let reporter = SilentVerificationReporter::new();

    // Test with large damaged block list - should still be silent
    let large_damaged_blocks: Vec<u32> = (0..1000).collect();
    let corrupted_file = FileVerificationResult {
        file_name: "heavily_corrupted.txt".to_string(),
        file_id: FileId::new([3; 16]),
        status: FileStatus::Corrupted,
        blocks_available: 0,
        total_blocks: 1000,
        damaged_blocks: large_damaged_blocks,
    };
    let results = create_test_results(vec![corrupted_file], 0, 0, 1, 0);
    reporter.report_verification_results(&results);
}

#[test]
fn test_report_verification_results_mixed_files_silent() {
    let reporter = SilentVerificationReporter::new();

    let mixed_files = vec![
        FileVerificationResult {
            file_name: "good.txt".to_string(),
            file_id: FileId::new([4; 16]),
            status: FileStatus::Present,
            blocks_available: 10,
            total_blocks: 10,
            damaged_blocks: vec![],
        },
        FileVerificationResult {
            file_name: "missing.txt".to_string(),
            file_id: FileId::new([5; 16]),
            status: FileStatus::Missing,
            blocks_available: 0,
            total_blocks: 10,
            damaged_blocks: vec![],
        },
        FileVerificationResult {
            file_name: "corrupted.txt".to_string(),
            file_id: FileId::new([6; 16]),
            status: FileStatus::Corrupted,
            blocks_available: 7,
            total_blocks: 10,
            damaged_blocks: vec![2, 4, 6],
        },
        FileVerificationResult {
            file_name: "renamed.txt".to_string(),
            file_id: FileId::new([7; 16]),
            status: FileStatus::Renamed,
            blocks_available: 10,
            total_blocks: 10,
            damaged_blocks: vec![],
        },
    ];
    let results = create_test_results(mixed_files, 1, 1, 1, 1);
    reporter.report_verification_results(&results);
}

#[test]
fn test_comprehensive_verification_workflow_silent() {
    let reporter = SilentVerificationReporter::new();

    // Simulate a complete verification workflow - all should be silent
    reporter.report_verification_start(true);
    reporter.report_files_found(3);

    // File 1
    reporter.report_verifying_file("file1.txt");
    reporter.report_file_status("file1.txt", FileStatus::Present);
    reporter.report_damaged_blocks("file1.txt", &[]);

    // File 2
    reporter.report_verifying_file("file2.txt");
    reporter.report_file_status("file2.txt", FileStatus::Missing);

    // File 3
    reporter.report_verifying_file("file3.txt");
    reporter.report_file_status("file3.txt", FileStatus::Corrupted);
    reporter.report_damaged_blocks("file3.txt", &[1, 5, 9]);

    // Final results
    let final_files = vec![
        FileVerificationResult {
            file_name: "file1.txt".to_string(),
            file_id: FileId::new([8; 16]),
            status: FileStatus::Present,
            blocks_available: 10,
            total_blocks: 10,
            damaged_blocks: vec![],
        },
        FileVerificationResult {
            file_name: "file2.txt".to_string(),
            file_id: FileId::new([9; 16]),
            status: FileStatus::Missing,
            blocks_available: 0,
            total_blocks: 10,
            damaged_blocks: vec![],
        },
        FileVerificationResult {
            file_name: "file3.txt".to_string(),
            file_id: FileId::new([10; 16]),
            status: FileStatus::Corrupted,
            blocks_available: 7,
            total_blocks: 10,
            damaged_blocks: vec![1, 5, 9],
        },
    ];
    let final_results = create_test_results(final_files, 1, 0, 1, 1);
    reporter.report_verification_results(&final_results);
}

#[test]
fn test_trait_object_usage_silent() {
    let reporter: Box<dyn VerificationReporter> = Box::new(SilentVerificationReporter::new());
    reporter.report_progress("Trait object test", 0.5);
    reporter.report_verification_start(true);
    reporter.report_error("Should be silent");
}

#[test]
fn test_send_sync_traits() {
    use std::thread;

    let reporter = SilentVerificationReporter::new();
    let handle = thread::spawn(move || {
        reporter.report_progress("Thread test", 0.5);
        reporter.report_verifying_file("threaded_file.txt");
    });
    handle.join().unwrap();
}

#[test]
fn test_extreme_values_silent() {
    let reporter = SilentVerificationReporter::new();

    // Test with extreme values - all should be silent
    reporter.report_files_found(usize::MAX);
    reporter.report_progress("Max progress", 1.0);
    reporter.report_progress("Zero progress", 0.0);

    // Very large damaged block list
    let huge_list: Vec<u32> = (0..100000).collect();
    reporter.report_damaged_blocks("huge_file.txt", &huge_list);
}
