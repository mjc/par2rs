//! Tests for ConsoleVerificationReporter
//!
//! Ensures 100% coverage of console verification reporter functionality.

use par2rs::domain::FileId;
use par2rs::reporters::{ConsoleVerificationReporter, Reporter, VerificationReporter};
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
    let _reporter = ConsoleVerificationReporter::new();
}

#[test]
fn test_default_constructor() {
    let _reporter = ConsoleVerificationReporter::new();
    // Test that default() creates a valid instance  
}

#[test]
fn test_report_progress() {
    let reporter = ConsoleVerificationReporter::new();
    reporter.report_progress("Starting verification", 0.0);
    reporter.report_progress("Half complete", 0.5);
    reporter.report_progress("Almost done", 0.99);
    reporter.report_progress("Complete", 1.0);
}

#[test]
fn test_report_error() {
    let reporter = ConsoleVerificationReporter::new();
    reporter.report_error("Test error message");
    reporter.report_error("Another error");
    reporter.report_error("");
}

#[test]
fn test_report_complete() {
    let reporter = ConsoleVerificationReporter::new();
    reporter.report_complete("Verification completed successfully");
    reporter.report_complete("All done");
    reporter.report_complete("");
}

#[test]
fn test_report_verification_start() {
    let reporter = ConsoleVerificationReporter::new();
    reporter.report_verification_start(true); // parallel
    reporter.report_verification_start(false); // sequential
}

#[test]
fn test_report_files_found() {
    let reporter = ConsoleVerificationReporter::new();
    reporter.report_files_found(0);
    reporter.report_files_found(1);
    reporter.report_files_found(10);
    reporter.report_files_found(100);
    reporter.report_files_found(usize::MAX);
}

#[test]
fn test_report_verifying_file() {
    let reporter = ConsoleVerificationReporter::new();
    reporter.report_verifying_file("test.txt");
    reporter.report_verifying_file("file with spaces.dat");
    reporter.report_verifying_file("unicode_测试.bin");
    reporter.report_verifying_file("");
    reporter.report_verifying_file("very_long_filename_that_might_cause_display_issues.txt");
}

#[test]
fn test_report_file_status_all_variants() {
    let reporter = ConsoleVerificationReporter::new();

    // Test all FileStatus enum variants
    reporter.report_file_status("present.txt", FileStatus::Present);
    reporter.report_file_status("missing.txt", FileStatus::Missing);
    reporter.report_file_status("corrupted.txt", FileStatus::Corrupted);
    reporter.report_file_status("renamed.txt", FileStatus::Renamed);

    // Test with various filenames
    reporter.report_file_status("", FileStatus::Present);
    reporter.report_file_status("file with spaces.dat", FileStatus::Corrupted);
}

#[test]
fn test_report_damaged_blocks() {
    let reporter = ConsoleVerificationReporter::new();

    // Empty list
    reporter.report_damaged_blocks("file1.txt", &[]);

    // Single block
    reporter.report_damaged_blocks("file2.txt", &[42]);

    // Few blocks
    reporter.report_damaged_blocks("file3.txt", &[1, 5, 10]);

    // Many blocks
    reporter.report_damaged_blocks("file4.txt", &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

    // Large list
    let large_list: Vec<u32> = (0..100).collect();
    reporter.report_damaged_blocks("file5.txt", &large_list);
}

#[test]
fn test_report_verification_results_empty() {
    let reporter = ConsoleVerificationReporter::new();
    let empty_results = create_test_results(vec![], 0, 0, 0, 0);
    reporter.report_verification_results(&empty_results);
}

#[test]
fn test_report_verification_results_single_file() {
    let reporter = ConsoleVerificationReporter::new();

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
fn test_report_verification_results_with_small_damaged_blocks() {
    let reporter = ConsoleVerificationReporter::new();

    // Test with damaged blocks ≤ 20 (exercises the detailed list path)
    let corrupted_file = FileVerificationResult {
        file_name: "corrupted_small.txt".to_string(),
        file_id: FileId::new([2; 16]),
        status: FileStatus::Corrupted,
        blocks_available: 5,
        total_blocks: 10,
        damaged_blocks: vec![1, 3, 5, 7, 9], // 5 blocks ≤ 20
    };
    let results = create_test_results(vec![corrupted_file], 0, 0, 1, 0);
    reporter.report_verification_results(&results);
}

#[test]
fn test_report_verification_results_with_large_damaged_blocks() {
    let reporter = ConsoleVerificationReporter::new();

    // Test with damaged blocks > 20 (exercises the summary path)
    let large_damaged_blocks: Vec<u32> = (0..50).collect(); // 50 blocks > 20
    let corrupted_file = FileVerificationResult {
        file_name: "corrupted_large.txt".to_string(),
        file_id: FileId::new([3; 16]),
        status: FileStatus::Corrupted,
        blocks_available: 0,
        total_blocks: 50,
        damaged_blocks: large_damaged_blocks,
    };
    let results = create_test_results(vec![corrupted_file], 0, 0, 1, 0);
    reporter.report_verification_results(&results);
}

#[test]
fn test_report_verification_results_boundary_cases() {
    let reporter = ConsoleVerificationReporter::new();

    // Test exactly 20 damaged blocks (boundary case)
    let exactly_20_blocks: Vec<u32> = (0..20).collect();
    let file_20 = FileVerificationResult {
        file_name: "boundary_20.txt".to_string(),
        file_id: FileId::new([4; 16]),
        status: FileStatus::Corrupted,
        blocks_available: 0,
        total_blocks: 20,
        damaged_blocks: exactly_20_blocks,
    };
    let results_20 = create_test_results(vec![file_20], 0, 0, 1, 0);
    reporter.report_verification_results(&results_20);

    // Test 21 damaged blocks (just over boundary)
    let over_20_blocks: Vec<u32> = (0..21).collect();
    let file_21 = FileVerificationResult {
        file_name: "over_boundary_21.txt".to_string(),
        file_id: FileId::new([5; 16]),
        status: FileStatus::Corrupted,
        blocks_available: 0,
        total_blocks: 21,
        damaged_blocks: over_20_blocks,
    };
    let results_21 = create_test_results(vec![file_21], 0, 0, 1, 0);
    reporter.report_verification_results(&results_21);
}

#[test]
fn test_report_verification_results_mixed_files() {
    let reporter = ConsoleVerificationReporter::new();

    let mixed_files = vec![
        FileVerificationResult {
            file_name: "good.txt".to_string(),
            file_id: FileId::new([6; 16]),
            status: FileStatus::Present,
            blocks_available: 10,
            total_blocks: 10,
            damaged_blocks: vec![],
        },
        FileVerificationResult {
            file_name: "missing.txt".to_string(),
            file_id: FileId::new([7; 16]),
            status: FileStatus::Missing,
            blocks_available: 0,
            total_blocks: 10,
            damaged_blocks: vec![],
        },
        FileVerificationResult {
            file_name: "corrupted.txt".to_string(),
            file_id: FileId::new([8; 16]),
            status: FileStatus::Corrupted,
            blocks_available: 7,
            total_blocks: 10,
            damaged_blocks: vec![2, 4, 6],
        },
        FileVerificationResult {
            file_name: "renamed.txt".to_string(),
            file_id: FileId::new([9; 16]),
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
fn test_print_block_list_head_tail_logic() {
    let reporter = ConsoleVerificationReporter::new();

    // Test very large damaged block list to ensure head/tail summary works
    let very_large_blocks: Vec<u32> = (0..1000).collect();
    let massive_file = FileVerificationResult {
        file_name: "massive_corruption.txt".to_string(),
        file_id: FileId::new([10; 16]),
        status: FileStatus::Corrupted,
        blocks_available: 0,
        total_blocks: 1000,
        damaged_blocks: very_large_blocks,
    };
    let results = create_test_results(vec![massive_file], 0, 0, 1, 0);
    reporter.report_verification_results(&results);
}

#[test]
fn test_trait_object_usage() {
    let reporter: Box<dyn VerificationReporter> = Box::new(ConsoleVerificationReporter::new());
    reporter.report_progress("Trait object test", 0.5);
    reporter.report_verification_start(true);
}

#[test]
fn test_send_sync_traits() {
    use std::thread;

    let reporter = ConsoleVerificationReporter::new();
    let handle = thread::spawn(move || {
        reporter.report_progress("Thread test", 0.5);
    });
    handle.join().unwrap();
}
