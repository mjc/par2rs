//! Console reporters for PAR2 operations
//!
//! Provides par2cmdline-style output formatting for verification and repair
//! progress and results.

use super::{RepairReporter, Reporter, VerificationReporter};
use crate::verify::{FileStatus, VerificationResults};

/// Constants for output formatting
const MIN_BLOCKS_FOR_SUMMARY: usize = 20; // Show detailed block list if <= this many blocks
const BLOCK_SUMMARY_HEAD_TAIL: usize = 10; // Show first/last N blocks for large damaged lists

/// Console implementation for verification operations
#[derive(Default)]
pub struct ConsoleVerificationReporter;

impl ConsoleVerificationReporter {
    pub fn new() -> Self {
        Self
    }
}

/// Console implementation for repair operations
#[derive(Default)]
pub struct ConsoleRepairReporter;

impl ConsoleRepairReporter {
    pub fn new() -> Self {
        Self
    }
}

// Base Reporter implementation for ConsoleVerificationReporter
impl Reporter for ConsoleVerificationReporter {
    fn report_progress(&self, message: &str, progress: f64) {
        println!("{} ({:.1}%)", message, progress * 100.0);
    }

    fn report_error(&self, error: &str) {
        eprintln!("Error: {}", error);
    }

    fn report_complete(&self, message: &str) {
        println!("{}", message);
    }
}

impl VerificationReporter for ConsoleVerificationReporter {
    fn report_verification_start(&self, parallel: bool) {
        println!(
            "Starting comprehensive verification ({})...",
            if parallel { "parallel" } else { "sequential" }
        );
    }

    fn report_files_found(&self, count: usize) {
        println!("Found {} files to verify", count);
    }

    fn report_verifying_file(&self, file_name: &str) {
        println!("Verifying: \"{}\"", file_name);
    }

    fn report_file_status(&self, file_name: &str, status: FileStatus) {
        match status {
            FileStatus::Present => println!("Target: \"{}\" - found.", file_name),
            FileStatus::Missing => println!("Target: \"{}\" - missing.", file_name),
            FileStatus::Corrupted => println!("Target: \"{}\" - corrupted.", file_name),
            FileStatus::Renamed => println!("Target: \"{}\" - renamed.", file_name),
        }
    }

    fn report_damaged_blocks(&self, _file_name: &str, damaged_blocks: &[u32]) {
        if !damaged_blocks.is_empty() {
            println!("  {} blocks are damaged", damaged_blocks.len());
        }
    }

    fn report_verification_results(&self, results: &VerificationResults) {
        // Use the Display implementation for main summary
        print!("{}", results);

        // Print detailed block information for corrupted files
        for file_result in &results.files {
            if !file_result.damaged_blocks.is_empty() {
                println!("\nDamaged blocks in \"{}\":", file_result.file_name);
                print_block_list(&file_result.damaged_blocks);
            }
        }
    }
}

// Base Reporter implementation for ConsoleRepairReporter
impl Reporter for ConsoleRepairReporter {
    fn report_progress(&self, message: &str, progress: f64) {
        println!("{} ({:.1}%)", message, progress * 100.0);
    }

    fn report_error(&self, error: &str) {
        eprintln!("Error: {}", error);
    }

    fn report_complete(&self, message: &str) {
        println!("{}", message);
    }
}

impl RepairReporter for ConsoleRepairReporter {
    fn report_repair_start(&self, files_to_repair: usize) {
        println!("Starting repair operation for {} files...", files_to_repair);
    }

    fn report_repair_progress(&self, file_name: &str, progress: f64) {
        println!("Repairing \"{}\": {:.1}%", file_name, progress * 100.0);
    }

    fn report_file_repaired(&self, file_name: &str) {
        println!("Target: \"{}\" - repaired successfully.", file_name);
    }

    fn report_repair_failed(&self, file_name: &str, error: &str) {
        println!("Target: \"{}\" - repair failed: {}", file_name, error);
    }

    fn report_repair_complete(&self, total_files: usize, successful: usize, failed: usize) {
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

/// Print a list of block numbers, with summary for large lists
fn print_block_list(damaged_blocks: &[u32]) {
    if damaged_blocks.len() <= MIN_BLOCKS_FOR_SUMMARY {
        // Show all blocks if there are few enough
        for &block_num in damaged_blocks {
            println!("  Block {}: damaged", block_num);
        }
    } else {
        // Show first and last N blocks if there are many
        for &block_num in &damaged_blocks[..BLOCK_SUMMARY_HEAD_TAIL] {
            println!("  Block {}: damaged", block_num);
        }
        println!(
            "  ... {} more damaged blocks ...",
            damaged_blocks.len() - (2 * BLOCK_SUMMARY_HEAD_TAIL)
        );
        for &block_num in &damaged_blocks[damaged_blocks.len() - BLOCK_SUMMARY_HEAD_TAIL..] {
            println!("  Block {}: damaged", block_num);
        }
    }
}
