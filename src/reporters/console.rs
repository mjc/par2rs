//! Console reporters for PAR2 operations
//!
//! Provides par2cmdline-style output formatting for verification and repair
//! progress and results.

use super::{RepairReporter, Reporter, VerificationReporter};
use crate::verify::{FileStatus, VerificationResults};
use std::collections::HashSet;
use std::sync::Mutex;

/// Console implementation for verification operations
/// Uses internal mutex for thread-safe output from parallel operations
pub struct ConsoleVerificationReporter {
    /// Mutex to ensure atomic printing from multiple threads
    /// Reference: par2cmdline-turbo uses output_lock for thread-safe console output
    output_lock: Mutex<()>,
}

/// Concise verification output used for a single `-q`, matching
/// par2cmdline-turbo's quiet-but-not-silent mode.
pub struct ConciseVerificationReporter {
    output_lock: Mutex<()>,
    reported_files: Mutex<HashSet<String>>,
}

impl Default for ConciseVerificationReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ConciseVerificationReporter {
    pub fn new() -> Self {
        Self {
            output_lock: Mutex::new(()),
            reported_files: Mutex::new(HashSet::new()),
        }
    }
}

impl Default for ConsoleVerificationReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleVerificationReporter {
    pub fn new() -> Self {
        Self {
            output_lock: Mutex::new(()),
        }
    }
}

/// Console implementation for repair operations
pub struct ConsoleRepairReporter {
    /// Mutex to ensure atomic printing from multiple threads
    output_lock: Mutex<()>,
}

impl Default for ConsoleRepairReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleRepairReporter {
    pub fn new() -> Self {
        Self {
            output_lock: Mutex::new(()),
        }
    }
}

// Base Reporter implementation for ConsoleVerificationReporter
impl Reporter for ConsoleVerificationReporter {
    fn report_progress(&self, message: &str, progress: f64) {
        let _lock = self.output_lock.lock().unwrap();
        println!("{} ({:.1}%)", message, progress * 100.0);
    }

    fn report_error(&self, error: &str) {
        let _lock = self.output_lock.lock().unwrap();
        eprintln!("Error: {}", error);
    }

    fn report_complete(&self, message: &str) {
        let _lock = self.output_lock.lock().unwrap();
        println!("{}", message);
    }
}

impl VerificationReporter for ConsoleVerificationReporter {
    fn report_verification_start(&self, _parallel: bool) {
        // par2cmdline doesn't print this
    }

    fn report_files_found(&self, _count: usize) {
        // par2cmdline doesn't print this
    }

    fn report_verifying_file(&self, _file_name: &str) {
        // par2cmdline doesn't print individual file verification start
    }

    fn report_file_status(&self, file_name: &str, status: FileStatus) {
        let _lock = self.output_lock.lock().unwrap();
        match status {
            FileStatus::Present => println!("Target: \"{}\" - found.", file_name),
            FileStatus::Missing => println!("Target: \"{}\" - missing.", file_name),
            FileStatus::Corrupted => {
                // Note: block counts will be reported separately via report_damaged_blocks
                // This matches par2cmdline output style
            }
            FileStatus::Renamed => println!("Target: \"{}\" - renamed.", file_name),
        }
    }

    fn report_damaged_blocks(
        &self,
        file_name: &str,
        damaged_blocks: &[u32],
        available_blocks: usize,
        total_blocks: usize,
    ) {
        let _lock = self.output_lock.lock().unwrap();
        if !damaged_blocks.is_empty() {
            println!(
                "Target: \"{}\" - damaged. Found {} of {} data blocks.",
                file_name, available_blocks, total_blocks
            );
        }
    }

    fn report_verification_results(&self, results: &VerificationResults) {
        let _lock = self.output_lock.lock().unwrap();
        // Use the Display implementation for main summary
        // par2cmdline doesn't print detailed block lists in normal mode
        print!("{}", results);
    }

    fn report_scanning_progress(&self, fraction: f64) {
        let _lock = self.output_lock.lock().unwrap();
        // Match par2cmdline-turbo's format: "Scanning: X.X%\r"
        // The \r returns to start of line so next update overwrites
        use std::io::{self, Write};
        let percent = (fraction * 1000.0) as u32;
        print!("Scanning: {}.{}%\r", percent / 10, percent % 10);
        let _ = io::stdout().flush();
    }
}

impl Reporter for ConciseVerificationReporter {
    fn report_progress(&self, _message: &str, _progress: f64) {}

    fn report_error(&self, error: &str) {
        let _lock = self.output_lock.lock().unwrap();
        eprintln!("Error: {}", error);
    }

    fn report_complete(&self, message: &str) {
        let _lock = self.output_lock.lock().unwrap();
        println!("{}", message);
    }
}

impl VerificationReporter for ConciseVerificationReporter {
    fn report_verification_start(&self, _parallel: bool) {}

    fn report_files_found(&self, _count: usize) {}

    fn report_verifying_file(&self, _file_name: &str) {}

    fn report_file_status(&self, file_name: &str, status: FileStatus) {
        let _lock = self.output_lock.lock().unwrap();
        self.reported_files
            .lock()
            .unwrap()
            .insert(file_name.to_string());
        match status {
            FileStatus::Present => println!("Target: \"{}\" - found.", file_name),
            FileStatus::Missing => println!("Target: \"{}\" - missing.", file_name),
            FileStatus::Corrupted => {}
            FileStatus::Renamed => println!("Target: \"{}\" - renamed.", file_name),
        }
    }

    fn report_damaged_blocks(
        &self,
        file_name: &str,
        damaged_blocks: &[u32],
        available_blocks: usize,
        total_blocks: usize,
    ) {
        let _lock = self.output_lock.lock().unwrap();
        if !damaged_blocks.is_empty() {
            self.reported_files
                .lock()
                .unwrap()
                .insert(file_name.to_string());
            println!(
                "Target: \"{}\" - damaged. Found {} of {} data blocks.",
                file_name, available_blocks, total_blocks
            );
        }
    }

    fn report_verification_results(&self, results: &VerificationResults) {
        let _lock = self.output_lock.lock().unwrap();
        let mut reported_files = self.reported_files.lock().unwrap();
        for file in &results.files {
            if !reported_files.insert(file.file_name.clone()) {
                continue;
            }

            match file.status {
                FileStatus::Present => println!("Target: \"{}\" - found.", file.file_name),
                FileStatus::Missing => println!("Target: \"{}\" - missing.", file.file_name),
                FileStatus::Corrupted => println!(
                    "Target: \"{}\" - damaged. Found {} of {} data blocks.",
                    file.file_name, file.blocks_available, file.total_blocks
                ),
                FileStatus::Renamed => println!("Target: \"{}\" - renamed.", file.file_name),
            }
        }
        drop(reported_files);

        println!();

        match (results.missing_block_count, results.repair_possible) {
            (0, _) => println!("All files are correct, repair is not required."),
            (_, true) => {
                println!("Repair is required.");
                println!("Repair is possible.");
            }
            (missing, false) => {
                println!("Repair is required.");
                println!("Repair is not possible.");
                println!(
                    "You need {} more recovery blocks to be able to repair.",
                    missing - results.recovery_blocks_available
                );
            }
        }
    }

    fn report_scanning_progress(&self, _fraction: f64) {}
}

// Base Reporter implementation for ConsoleRepairReporter
impl Reporter for ConsoleRepairReporter {
    fn report_progress(&self, message: &str, progress: f64) {
        let _lock = self.output_lock.lock().unwrap();
        println!("{} ({:.1}%)", message, progress * 100.0);
    }

    fn report_error(&self, error: &str) {
        let _lock = self.output_lock.lock().unwrap();
        eprintln!("Error: {}", error);
    }

    fn report_complete(&self, message: &str) {
        let _lock = self.output_lock.lock().unwrap();
        println!("{}", message);
    }
}

impl RepairReporter for ConsoleRepairReporter {
    fn report_repair_start(&self, files_to_repair: usize) {
        let _lock = self.output_lock.lock().unwrap();
        println!("Starting repair operation for {} files...", files_to_repair);
    }

    fn report_repair_progress(&self, file_name: &str, progress: f64) {
        let _lock = self.output_lock.lock().unwrap();
        println!("Repairing \"{}\": {:.1}%", file_name, progress * 100.0);
    }

    fn report_file_repaired(&self, file_name: &str) {
        let _lock = self.output_lock.lock().unwrap();
        println!("Target: \"{}\" - repaired successfully.", file_name);
    }

    fn report_repair_failed(&self, file_name: &str, error: &str) {
        let _lock = self.output_lock.lock().unwrap();
        println!("Target: \"{}\" - repair failed: {}", file_name, error);
    }

    fn report_repair_complete(&self, total_files: usize, successful: usize, failed: usize) {
        let _lock = self.output_lock.lock().unwrap();
        println!("\nRepair operation complete:");
        println!("  Total files: {}", total_files);
        println!("  Successfully repaired: {}", successful);
        println!("  Failed to repair: {}", failed);

        if failed == 0 {
            println!("All files repaired successfully!");
        } else if successful > 0 {
            println!("Partial repair completed.");
        } else {
            println!("No files could be repaired.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_console_reporter_thread_safe() {
        // Test that multiple threads can safely use the reporter
        let reporter = Arc::new(ConsoleVerificationReporter::new());
        let mut handles = vec![];

        for i in 0..10 {
            let reporter_clone = Arc::clone(&reporter);
            let handle = thread::spawn(move || {
                reporter_clone.report_progress(&format!("Thread {}", i), 0.5);
                reporter_clone.report_file_status(&format!("file{}.txt", i), FileStatus::Present);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }
        // If this completes without panic, the reporter is thread-safe
    }

    #[test]
    fn test_repair_reporter_thread_safe() {
        let reporter = Arc::new(ConsoleRepairReporter::new());
        let mut handles = vec![];

        for i in 0..10 {
            let reporter_clone = Arc::clone(&reporter);
            let handle = thread::spawn(move || {
                reporter_clone.report_file_repaired(&format!("file{}.txt", i));
                reporter_clone.report_repair_progress(&format!("file{}.txt", i), 0.75);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_reporter_multiple_operations() {
        // Test various operations work correctly
        let reporter = ConsoleVerificationReporter::new();

        reporter.report_progress("Testing progress", 0.25);
        reporter.report_file_status("test.txt", FileStatus::Present);
        reporter.report_file_status("missing.txt", FileStatus::Missing);
        reporter.report_file_status("corrupt.txt", FileStatus::Corrupted);
        reporter.report_damaged_blocks("damaged.txt", &[1, 2, 3], 97, 100);
        reporter.report_scanning_progress(0.5);
        reporter.report_error("Test error");
        reporter.report_complete("Test complete");
    }

    #[test]
    fn test_repair_reporter_operations() {
        let reporter = ConsoleRepairReporter::new();

        reporter.report_repair_start(5);
        reporter.report_repair_progress("file1.txt", 0.5);
        reporter.report_file_repaired("file1.txt");
        reporter.report_repair_failed("file2.txt", "corruption too severe");
        reporter.report_repair_complete(5, 4, 1);
    }

    #[test]
    fn test_mutex_prevents_interleaving() {
        // Test that mutex actually prevents message interleaving
        // We can't easily test the actual output, but we can verify
        // that the lock is acquired and released properly
        let reporter = Arc::new(ConsoleVerificationReporter::new());

        // Spawn multiple threads that all try to report at once
        let handles: Vec<_> = (0..50)
            .map(|i| {
                let reporter = Arc::clone(&reporter);
                thread::spawn(move || {
                    // Multiple operations in sequence should be atomic
                    reporter.report_file_status(&format!("file{}.txt", i), FileStatus::Present);
                    reporter.report_progress(&format!("Progress {}", i), 0.1 * i as f64);
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_damaged_blocks_reporting() {
        let reporter = ConsoleVerificationReporter::new();

        // Test with few blocks
        reporter.report_damaged_blocks("test1.txt", &[1, 2, 3], 97, 100);

        // Test with many blocks (should use summary format)
        let many_blocks: Vec<u32> = (0..50).collect();
        reporter.report_damaged_blocks("test2.txt", &many_blocks, 50, 100);

        // Test with empty blocks
        reporter.report_damaged_blocks("test3.txt", &[], 100, 100);
    }

    #[test]
    fn test_scanning_progress() {
        let reporter = ConsoleVerificationReporter::new();

        // Test various progress values
        for i in 0..=10 {
            reporter.report_scanning_progress(i as f64 / 10.0);
        }
    }
}
