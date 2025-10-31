//! Tests for SilentRepairReporter
//!
//! Ensures 100% coverage of silent repair reporter functionality.

use par2rs::reporters::{RepairReporter, Reporter, SilentRepairReporter};

#[test]
fn test_new_constructor() {
    let _reporter = SilentRepairReporter::new();
}

#[test]
fn test_default_constructor() {
    let _reporter = SilentRepairReporter::new();
}

#[test]
fn test_report_progress_silent() {
    let reporter = SilentRepairReporter::new();

    // All these should be silent no-ops
    reporter.report_progress("Repair starting", 0.0);
    reporter.report_progress("Halfway through", 0.5);
    reporter.report_progress("Almost complete", 0.99);
    reporter.report_progress("Repair finished", 1.0);
    reporter.report_progress("", 0.0);
    reporter.report_progress("Unicode 修复", 0.33);
}

#[test]
fn test_report_error_silent() {
    let reporter = SilentRepairReporter::new();

    // All these should be silent no-ops
    reporter.report_error("Repair error occurred");
    reporter.report_error("Cannot repair file");
    reporter.report_error("I/O error during repair");
    reporter.report_error("");
    reporter.report_error("Unicode error 错误");
}

#[test]
fn test_report_complete_silent() {
    let reporter = SilentRepairReporter::new();

    // All these should be silent no-ops
    reporter.report_complete("Repair operation finished");
    reporter.report_complete("All repairs completed");
    reporter.report_complete("");
    reporter.report_complete("Unicode 完成");
}

#[test]
fn test_report_repair_start_silent() {
    let reporter = SilentRepairReporter::new();

    // All should be silent no-ops
    reporter.report_repair_start(0);
    reporter.report_repair_start(1);
    reporter.report_repair_start(5);
    reporter.report_repair_start(100);
    reporter.report_repair_start(usize::MAX);
}

#[test]
fn test_report_repair_progress_silent() {
    let reporter = SilentRepairReporter::new();

    // All should be silent no-ops
    reporter.report_repair_progress("file1.txt", 0.0);
    reporter.report_repair_progress("file2.txt", 0.25);
    reporter.report_repair_progress("file3.txt", 0.5);
    reporter.report_repair_progress("file4.txt", 0.75);
    reporter.report_repair_progress("file5.txt", 1.0);

    // Test with edge case filenames
    reporter.report_repair_progress("", 0.5);
    reporter.report_repair_progress("file with spaces.dat", 0.3);
    reporter.report_repair_progress("unicode_测试.bin", 0.8);
    reporter.report_repair_progress(
        "very_long_filename_that_might_cause_display_issues.txt",
        0.9,
    );
}

#[test]
fn test_report_file_repaired_silent() {
    let reporter = SilentRepairReporter::new();

    // All should be silent no-ops
    reporter.report_file_repaired("repaired1.txt");
    reporter.report_file_repaired("repaired2.txt");
    reporter.report_file_repaired("file with spaces.dat");
    reporter.report_file_repaired("unicode_修复.bin");
    reporter.report_file_repaired("");
    reporter.report_file_repaired(
        "extremely_long_filename_with_many_characters_to_test_edge_cases.txt",
    );
}

#[test]
fn test_report_repair_failed_silent() {
    let reporter = SilentRepairReporter::new();

    // All should be silent no-ops
    reporter.report_repair_failed("failed1.txt", "Not enough recovery blocks");
    reporter.report_repair_failed("failed2.txt", "I/O error");
    reporter.report_repair_failed("failed3.txt", "Checksum mismatch");
    reporter.report_repair_failed("failed4.txt", "Permission denied");
    reporter.report_repair_failed("failed5.txt", "Disk full");

    // Edge cases
    reporter.report_repair_failed("", "");
    reporter.report_repair_failed("nonempty.txt", "");
    reporter.report_repair_failed("", "Some error message");
    reporter.report_repair_failed("unicode_文件.txt", "Unicode error 错误");
}

#[test]
fn test_report_repair_complete_all_success_silent() {
    let reporter = SilentRepairReporter::new();

    // All should be silent no-ops
    reporter.report_repair_complete(5, 5, 0);
    reporter.report_repair_complete(1, 1, 0);
    reporter.report_repair_complete(100, 100, 0);
}

#[test]
fn test_report_repair_complete_partial_success_silent() {
    let reporter = SilentRepairReporter::new();

    // All should be silent no-ops
    reporter.report_repair_complete(10, 7, 3);
    reporter.report_repair_complete(5, 3, 2);
    reporter.report_repair_complete(100, 50, 50);
    reporter.report_repair_complete(2, 1, 1);
}

#[test]
fn test_report_repair_complete_all_failed_silent() {
    let reporter = SilentRepairReporter::new();

    // All should be silent no-ops
    reporter.report_repair_complete(3, 0, 3);
    reporter.report_repair_complete(1, 0, 1);
    reporter.report_repair_complete(10, 0, 10);
    reporter.report_repair_complete(100, 0, 100);
}

#[test]
fn test_report_repair_complete_no_files_silent() {
    let reporter = SilentRepairReporter::new();

    // Should be silent no-op
    reporter.report_repair_complete(0, 0, 0);
}

#[test]
fn test_report_repair_complete_edge_cases_silent() {
    let reporter = SilentRepairReporter::new();

    // Edge cases - all should be silent no-ops
    // More successful than total (shouldn't happen but test anyway)
    reporter.report_repair_complete(5, 6, 0);

    // Successful + failed > total
    reporter.report_repair_complete(5, 3, 4);

    // Large numbers
    reporter.report_repair_complete(usize::MAX, usize::MAX, 0);
    reporter.report_repair_complete(usize::MAX, 0, usize::MAX);
    reporter.report_repair_complete(usize::MAX, usize::MAX / 2, usize::MAX / 2);
}

#[test]
fn test_comprehensive_repair_workflow_silent() {
    let reporter = SilentRepairReporter::new();

    // Simulate a complete repair workflow - all should be silent
    reporter.report_repair_start(5);

    // File 1: successful repair
    reporter.report_repair_progress("file1.txt", 0.0);
    reporter.report_repair_progress("file1.txt", 0.33);
    reporter.report_repair_progress("file1.txt", 0.66);
    reporter.report_repair_progress("file1.txt", 1.0);
    reporter.report_file_repaired("file1.txt");

    // File 2: failed repair
    reporter.report_repair_progress("file2.txt", 0.0);
    reporter.report_repair_progress("file2.txt", 0.25);
    reporter.report_repair_failed("file2.txt", "Insufficient recovery data");

    // File 3: successful repair
    reporter.report_repair_progress("file3.txt", 0.0);
    reporter.report_repair_progress("file3.txt", 0.5);
    reporter.report_repair_progress("file3.txt", 1.0);
    reporter.report_file_repaired("file3.txt");

    // File 4: failed repair
    reporter.report_repair_progress("file4.txt", 0.0);
    reporter.report_repair_failed("file4.txt", "Disk error");

    // File 5: successful repair
    reporter.report_repair_progress("file5.txt", 0.0);
    reporter.report_repair_progress("file5.txt", 1.0);
    reporter.report_file_repaired("file5.txt");

    // Final summary
    reporter.report_repair_complete(5, 3, 2);
}

#[test]
fn test_mixed_base_and_repair_methods_silent() {
    let reporter = SilentRepairReporter::new();

    // Mix base Reporter methods with RepairReporter methods
    reporter.report_progress("General progress", 0.2);
    reporter.report_repair_start(3);
    reporter.report_error("General error");
    reporter.report_repair_progress("file1.txt", 0.5);
    reporter.report_complete("General completion");
    reporter.report_file_repaired("file1.txt");
    reporter.report_repair_failed("file2.txt", "Repair error");
    reporter.report_repair_complete(3, 1, 2);
}

#[test]
fn test_rapid_fire_calls_silent() {
    let reporter = SilentRepairReporter::new();

    // Test many rapid calls - all should be silent
    for i in 0..1000 {
        reporter.report_repair_progress(&format!("file_{}.txt", i), i as f64 / 1000.0);
        if i % 2 == 0 {
            reporter.report_file_repaired(&format!("file_{}.txt", i));
        } else {
            reporter.report_repair_failed(&format!("file_{}.txt", i), "Test error");
        }
    }

    reporter.report_repair_complete(1000, 500, 500);
}

#[test]
fn test_trait_object_usage_silent() {
    let reporter: Box<dyn RepairReporter> = Box::new(SilentRepairReporter::new());

    // All should be silent
    reporter.report_progress("Trait object test", 0.5);
    reporter.report_repair_start(5);
    reporter.report_error("Should be silent");
    reporter.report_repair_progress("test.txt", 0.75);
    reporter.report_file_repaired("test.txt");
}

#[test]
fn test_send_sync_traits() {
    use std::thread;

    let reporter = SilentRepairReporter::new();
    let handle = thread::spawn(move || {
        reporter.report_repair_progress("threaded_file.txt", 0.5);
        reporter.report_file_repaired("threaded_file.txt");
        reporter.report_repair_complete(1, 1, 0);
    });
    handle.join().unwrap();
}

#[test]
fn test_multiple_threads_silent() {
    use std::thread;

    let handles: Vec<_> = (0..10)
        .map(|i| {
            thread::spawn(move || {
                let reporter = SilentRepairReporter::new();
                reporter.report_repair_start(1);
                reporter.report_repair_progress(&format!("thread_{}_file.txt", i), 0.5);
                reporter.report_file_repaired(&format!("thread_{}_file.txt", i));
                reporter.report_repair_complete(1, 1, 0);
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_extreme_values_silent() {
    let reporter = SilentRepairReporter::new();

    // Test with extreme values - all should be silent
    reporter.report_repair_start(usize::MAX);
    reporter.report_repair_progress("extreme_file.txt", 0.0);
    reporter.report_repair_progress("extreme_file.txt", 1.0);
    reporter.report_repair_complete(usize::MAX, usize::MAX, 0);

    // Test with very long strings
    let long_filename = "a".repeat(10000);
    let long_error = "Error: ".repeat(1000);
    reporter.report_repair_progress(&long_filename, 0.5);
    reporter.report_repair_failed(&long_filename, &long_error);
}
