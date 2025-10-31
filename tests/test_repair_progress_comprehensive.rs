// Test repair progress reporters
// We can't easily test console output, but we can test that methods don't panic
// and that the trait is properly implemented

use par2rs::domain::RecoverySetId;
use par2rs::repair::{ConsoleReporter, ProgressReporter, SilentReporter};
use par2rs::repair::{FileStatus, RecoverySetInfo, VerificationResult};
use rustc_hash::FxHashMap;

fn create_test_recovery_set() -> RecoverySetInfo {
    RecoverySetInfo {
        set_id: RecoverySetId::new([0; 16]),
        slice_size: 16384,
        files: vec![],
        recovery_slices_metadata: vec![],
        file_slice_checksums: FxHashMap::default(),
    }
}

// ConsoleReporter tests (quiet=false)
#[test]
fn test_console_reporter_new() {
    let reporter = ConsoleReporter::new(false);
    // Should create successfully
    drop(reporter);
}

#[test]
fn test_console_reporter_new_quiet() {
    let reporter = ConsoleReporter::new(true);
    // Should create successfully
    drop(reporter);
}

#[test]
fn test_console_reporter_report_statistics() {
    let reporter = ConsoleReporter::new(false);
    let recovery_set = create_test_recovery_set();
    reporter.report_statistics(&recovery_set);
    // Should not panic
}

#[test]
fn test_console_reporter_report_statistics_quiet() {
    let reporter = ConsoleReporter::new(true);
    let recovery_set = create_test_recovery_set();
    reporter.report_statistics(&recovery_set);
    // Should not panic (and produce no output)
}

#[test]
fn test_console_reporter_report_file_opening() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_file_opening("test.txt");
    // Should not panic
}

#[test]
fn test_console_reporter_report_file_opening_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_file_opening("test.txt");
    // Should not panic
}

#[test]
fn test_console_reporter_report_file_status_present() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_file_status("file.bin", FileStatus::Present);
    // Should not panic
}

#[test]
fn test_console_reporter_report_file_status_missing() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_file_status("missing.txt", FileStatus::Missing);
    // Should not panic
}

#[test]
fn test_console_reporter_report_file_status_corrupted() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_file_status("damaged.bin", FileStatus::Corrupted);
    // Should not panic
}

#[test]
fn test_console_reporter_report_file_status_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_file_status("file.bin", FileStatus::Present);
    // Should not panic
}

#[test]
fn test_console_reporter_report_scanning() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_scanning("large_file.dat");
    // Should not panic
}

#[test]
fn test_console_reporter_report_scanning_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_scanning("large_file.dat");
    // Should not panic
}

#[test]
fn test_console_reporter_report_scanning_progress() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_scanning_progress("file.bin", 5000, 10000);
    // Should not panic
}

#[test]
fn test_console_reporter_report_scanning_progress_zero_total() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_scanning_progress("file.bin", 0, 0);
    // Should not panic (should handle division by zero)
}

#[test]
fn test_console_reporter_report_scanning_progress_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_scanning_progress("file.bin", 5000, 10000);
    // Should not panic
}

#[test]
fn test_console_reporter_report_scanning_progress_long_filename() {
    let reporter = ConsoleReporter::new(false);
    let long_name = "a".repeat(100);
    reporter.report_scanning_progress(&long_name, 1000, 10000);
    // Should not panic (should truncate filename)
}

#[test]
fn test_console_reporter_clear_scanning() {
    let reporter = ConsoleReporter::new(false);
    reporter.clear_scanning("file.bin");
    // Should not panic
}

#[test]
fn test_console_reporter_clear_scanning_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.clear_scanning("file.bin");
    // Should not panic
}

#[test]
fn test_console_reporter_report_recovery_info_sufficient() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_recovery_info(30, 20);
    // Should not panic
}

#[test]
fn test_console_reporter_report_recovery_info_insufficient() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_recovery_info(10, 20);
    // Should not panic
}

#[test]
fn test_console_reporter_report_recovery_info_exact() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_recovery_info(20, 20);
    // Should not panic
}

#[test]
fn test_console_reporter_report_recovery_info_zero_needed() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_recovery_info(20, 0);
    // Should not panic (should not print recovery info)
}

#[test]
fn test_console_reporter_report_recovery_info_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_recovery_info(30, 20);
    // Should not panic
}

#[test]
fn test_console_reporter_report_insufficient_recovery() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_insufficient_recovery(5, 10);
    // Should not panic
}

#[test]
fn test_console_reporter_report_insufficient_recovery_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_insufficient_recovery(5, 10);
    // Should not panic
}

#[test]
fn test_console_reporter_report_repair_header() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_repair_header();
    // Should not panic
}

#[test]
fn test_console_reporter_report_repair_header_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_repair_header();
    // Should not panic
}

#[test]
fn test_console_reporter_report_loading_progress_first() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_loading_progress(1, 10);
    // Should not panic
}

#[test]
fn test_console_reporter_report_loading_progress_middle() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_loading_progress(5, 10);
    // Should not panic
}

#[test]
fn test_console_reporter_report_loading_progress_last() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_loading_progress(10, 10);
    // Should not panic
}

#[test]
fn test_console_reporter_report_loading_progress_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_loading_progress(5, 10);
    // Should not panic
}

#[test]
fn test_console_reporter_report_constructing() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_constructing();
    // Should not panic
}

#[test]
fn test_console_reporter_report_constructing_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_constructing();
    // Should not panic
}

#[test]
fn test_console_reporter_report_computing_progress_start() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_computing_progress(0, 100);
    // Should not panic
}

#[test]
fn test_console_reporter_report_computing_progress_middle() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_computing_progress(50, 100);
    // Should not panic
}

#[test]
fn test_console_reporter_report_computing_progress_complete() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_computing_progress(100, 100);
    // Should not panic
}

#[test]
fn test_console_reporter_report_computing_progress_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_computing_progress(50, 100);
    // Should not panic
}

#[test]
fn test_console_reporter_report_repair_start() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_repair_start("file.txt");
    // Should not panic
}

#[test]
fn test_console_reporter_report_repair_start_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_repair_start("file.txt");
    // Should not panic
}

#[test]
fn test_console_reporter_report_writing_progress() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_writing_progress("file.bin", 5000, 10000);
    // Should not panic
}

#[test]
fn test_console_reporter_report_writing_progress_zero_total() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_writing_progress("file.bin", 0, 0);
    // Should not panic
}

#[test]
fn test_console_reporter_report_writing_progress_complete() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_writing_progress("file.bin", 10000, 10000);
    // Should not panic
}

#[test]
fn test_console_reporter_report_writing_progress_long_filename() {
    let reporter = ConsoleReporter::new(false);
    let long_name = "b".repeat(100);
    reporter.report_writing_progress(&long_name, 1000, 10000);
    // Should not panic
}

#[test]
fn test_console_reporter_report_writing_progress_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_writing_progress("file.bin", 5000, 10000);
    // Should not panic
}

#[test]
fn test_console_reporter_report_repair_complete_repaired() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_repair_complete("file.txt", true);
    // Should not panic
}

#[test]
fn test_console_reporter_report_repair_complete_already_valid() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_repair_complete("file.txt", false);
    // Should not panic
}

#[test]
fn test_console_reporter_report_repair_complete_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_repair_complete("file.txt", true);
    // Should not panic
}

#[test]
fn test_console_reporter_report_repair_failed() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_repair_failed("file.txt", "I/O error");
    // Should not panic
}

#[test]
fn test_console_reporter_report_repair_failed_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_repair_failed("file.txt", "I/O error");
    // Should not panic
}

#[test]
fn test_console_reporter_report_verification_header() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_verification_header();
    // Should not panic
}

#[test]
fn test_console_reporter_report_verification_header_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_verification_header();
    // Should not panic
}

#[test]
fn test_console_reporter_report_verification_verified() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_verification("file.txt", VerificationResult::Verified);
    // Should not panic
}

#[test]
fn test_console_reporter_report_verification_hash_mismatch() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_verification("file.txt", VerificationResult::HashMismatch);
    // Should not panic
}

#[test]
fn test_console_reporter_report_verification_size_mismatch() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_verification(
        "file.txt",
        VerificationResult::SizeMismatch {
            expected: 1000,
            actual: 900,
        },
    );
    // Should not panic
}

#[test]
fn test_console_reporter_report_verification_quiet() {
    let reporter = ConsoleReporter::new(true);
    reporter.report_verification("file.txt", VerificationResult::Verified);
    // Should not panic
}

#[test]
fn test_console_reporter_report_final_result() {
    use par2rs::repair::RepairResult;

    let reporter = ConsoleReporter::new(false);
    let result = RepairResult::NoRepairNeeded {
        files_verified: 5,
        verified_files: vec!["test.txt".to_string()],
        message: "All files OK".to_string(),
    };
    reporter.report_final_result(&result);
    // Should not panic
}

// SilentReporter tests
#[test]
fn test_silent_reporter_new() {
    let reporter = SilentReporter::new();
    drop(reporter);
}

#[test]
fn test_silent_reporter_default() {
    let reporter = SilentReporter::default();
    drop(reporter);
}

#[test]
fn test_silent_reporter_all_methods() {
    use par2rs::repair::RepairResult;

    let reporter = SilentReporter::new();
    let recovery_set = create_test_recovery_set();

    // Call all methods to ensure they don't panic
    reporter.report_statistics(&recovery_set);
    reporter.report_file_opening("test.txt");
    reporter.report_file_status("test.txt", FileStatus::Present);
    reporter.report_scanning("test.txt");
    reporter.report_scanning_progress("test.txt", 100, 1000);
    reporter.clear_scanning("test.txt");
    reporter.report_recovery_info(10, 5);
    reporter.report_insufficient_recovery(5, 10);
    reporter.report_repair_header();
    reporter.report_loading_progress(1, 10);
    reporter.report_constructing();
    reporter.report_computing_progress(50, 100);
    reporter.report_repair_start("test.txt");
    reporter.report_writing_progress("test.txt", 500, 1000);
    reporter.report_repair_complete("test.txt", true);
    reporter.report_repair_failed("test.txt", "error");
    reporter.report_verification_header();
    reporter.report_verification("test.txt", VerificationResult::Verified);

    let result = RepairResult::NoRepairNeeded {
        files_verified: 1,
        verified_files: vec![],
        message: String::new(),
    };
    reporter.report_final_result(&result);

    // All calls should succeed silently
}

// Test trait object usage
#[test]
fn test_progress_reporter_trait_object() {
    let console: Box<dyn ProgressReporter> = Box::new(ConsoleReporter::new(true));
    console.report_file_opening("test.txt");

    let silent: Box<dyn ProgressReporter> = Box::new(SilentReporter::new());
    silent.report_file_opening("test.txt");

    // Should work with trait objects
}

#[test]
fn test_progress_reporter_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ConsoleReporter>();
    assert_send_sync::<SilentReporter>();
}

// Edge case tests
#[test]
fn test_console_reporter_empty_filename() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_file_opening("");
    reporter.report_scanning("");
    reporter.clear_scanning("");
}

#[test]
fn test_console_reporter_unicode_filename() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_file_opening("тест_文件.txt");
    reporter.report_scanning("測試_файл.bin");
}

#[test]
fn test_console_reporter_special_chars_filename() {
    let reporter = ConsoleReporter::new(false);
    reporter.report_file_opening("file with spaces.txt");
    reporter.report_file_opening("file\twith\ttabs.txt");
}

#[test]
fn test_console_reporter_progress_percentages() {
    let reporter = ConsoleReporter::new(false);

    // Test various percentage values
    reporter.report_scanning_progress("file.bin", 0, 1000); // 0%
    reporter.report_scanning_progress("file.bin", 100, 1000); // 10%
    reporter.report_scanning_progress("file.bin", 500, 1000); // 50%
    reporter.report_scanning_progress("file.bin", 999, 1000); // 99.9%
    reporter.report_scanning_progress("file.bin", 1000, 1000); // 100%
}

#[test]
fn test_console_reporter_computing_progress_edge_cases() {
    let reporter = ConsoleReporter::new(false);

    reporter.report_computing_progress(0, 1); // Single block
    reporter.report_computing_progress(1, 1); // Complete
    reporter.report_computing_progress(0, 1000); // Large number
}
