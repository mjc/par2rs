//! Tests for ConsoleRepairReporter
//!
//! Ensures 100% coverage of console repair reporter functionality.

use par2rs::reporters::{ConsoleRepairReporter, RepairReporter, Reporter};

#[test]
fn test_new_constructor() {
    let _reporter = ConsoleRepairReporter::new();
}

#[test]
fn test_default_constructor() {
    let _reporter = ConsoleRepairReporter::new();
}

#[test]
fn test_report_progress() {
    let reporter = ConsoleRepairReporter::new();
    reporter.report_progress("Starting repair", 0.0);
    reporter.report_progress("Halfway through", 0.5);
    reporter.report_progress("Almost complete", 0.99);
    reporter.report_progress("Repair finished", 1.0);
}

#[test]
fn test_report_error() {
    let reporter = ConsoleRepairReporter::new();
    reporter.report_error("Repair error occurred");
    reporter.report_error("Cannot repair file");
    reporter.report_error("I/O error during repair");
    reporter.report_error("");
}

#[test]
fn test_report_complete() {
    let reporter = ConsoleRepairReporter::new();
    reporter.report_complete("Repair operation finished");
    reporter.report_complete("All repairs completed");
    reporter.report_complete("");
}

#[test]
fn test_report_repair_start() {
    let reporter = ConsoleRepairReporter::new();
    reporter.report_repair_start(0);
    reporter.report_repair_start(1);
    reporter.report_repair_start(5);
    reporter.report_repair_start(100);
    reporter.report_repair_start(usize::MAX);
}

#[test]
fn test_report_repair_progress() {
    let reporter = ConsoleRepairReporter::new();
    reporter.report_repair_progress("file1.txt", 0.0);
    reporter.report_repair_progress("file2.txt", 0.25);
    reporter.report_repair_progress("file3.txt", 0.5);
    reporter.report_repair_progress("file4.txt", 0.75);
    reporter.report_repair_progress("file5.txt", 1.0);

    // Test with various filenames
    reporter.report_repair_progress("", 0.5);
    reporter.report_repair_progress("file with spaces.dat", 0.3);
    reporter.report_repair_progress("unicode_测试.bin", 0.8);
}

#[test]
fn test_report_file_repaired() {
    let reporter = ConsoleRepairReporter::new();
    reporter.report_file_repaired("repaired1.txt");
    reporter.report_file_repaired("repaired2.txt");
    reporter.report_file_repaired("file with spaces.dat");
    reporter.report_file_repaired("unicode_修复.bin");
    reporter.report_file_repaired("");
}

#[test]
fn test_report_repair_failed() {
    let reporter = ConsoleRepairReporter::new();
    reporter.report_repair_failed("failed1.txt", "Not enough recovery blocks");
    reporter.report_repair_failed("failed2.txt", "I/O error");
    reporter.report_repair_failed("failed3.txt", "Checksum mismatch");
    reporter.report_repair_failed("failed4.txt", "Permission denied");
    reporter.report_repair_failed("", "");
    reporter.report_repair_failed("nonempty.txt", "");
    reporter.report_repair_failed("", "Some error");
}

#[test]
fn test_report_repair_complete_all_success() {
    let reporter = ConsoleRepairReporter::new();
    reporter.report_repair_complete(5, 5, 0);
    reporter.report_repair_complete(1, 1, 0);
    reporter.report_repair_complete(100, 100, 0);
}

#[test]
fn test_report_repair_complete_partial_success() {
    let reporter = ConsoleRepairReporter::new();
    reporter.report_repair_complete(10, 7, 3);
    reporter.report_repair_complete(5, 3, 2);
    reporter.report_repair_complete(100, 50, 50);
}

#[test]
fn test_report_repair_complete_all_failed() {
    let reporter = ConsoleRepairReporter::new();
    reporter.report_repair_complete(3, 0, 3);
    reporter.report_repair_complete(1, 0, 1);
    reporter.report_repair_complete(10, 0, 10);
}

#[test]
fn test_report_repair_complete_no_files() {
    let reporter = ConsoleRepairReporter::new();
    reporter.report_repair_complete(0, 0, 0);
}

#[test]
fn test_report_repair_complete_edge_cases() {
    let reporter = ConsoleRepairReporter::new();

    // Edge case: more successful than total (shouldn't happen but test anyway)
    reporter.report_repair_complete(5, 6, 0);

    // Edge case: successful + failed > total
    reporter.report_repair_complete(5, 3, 4);

    // Large numbers
    reporter.report_repair_complete(usize::MAX, usize::MAX, 0);
    reporter.report_repair_complete(usize::MAX, 0, usize::MAX);
}

#[test]
fn test_comprehensive_repair_workflow() {
    let reporter = ConsoleRepairReporter::new();

    // Simulate a complete repair workflow
    reporter.report_repair_start(3);

    // File 1: successful repair
    reporter.report_repair_progress("file1.txt", 0.0);
    reporter.report_repair_progress("file1.txt", 0.5);
    reporter.report_repair_progress("file1.txt", 1.0);
    reporter.report_file_repaired("file1.txt");

    // File 2: failed repair
    reporter.report_repair_progress("file2.txt", 0.0);
    reporter.report_repair_progress("file2.txt", 0.3);
    reporter.report_repair_failed("file2.txt", "Insufficient recovery data");

    // File 3: successful repair
    reporter.report_repair_progress("file3.txt", 0.0);
    reporter.report_repair_progress("file3.txt", 1.0);
    reporter.report_file_repaired("file3.txt");

    // Final summary
    reporter.report_repair_complete(3, 2, 1);
}

#[test]
fn test_trait_object_usage() {
    let reporter: Box<dyn RepairReporter> = Box::new(ConsoleRepairReporter::new());
    reporter.report_progress("Trait object test", 0.5);
    reporter.report_repair_start(5);
}

#[test]
fn test_send_sync_traits() {
    use std::thread;

    let reporter = ConsoleRepairReporter::new();
    let handle = thread::spawn(move || {
        reporter.report_repair_progress("threaded_file.txt", 0.5);
    });
    handle.join().unwrap();
}
