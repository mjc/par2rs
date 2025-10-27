//! Progress reporting for repair operations
//!
//! Provides traits and implementations for reporting progress during file repair.
//! This allows the repair logic to be decoupled from output formatting.

use super::types::{FileStatus, RecoverySetInfo, RepairResult, VerificationResult};

/// Trait for reporting repair progress
///
/// Implementations can provide different output formats (console, JSON, silent, etc.)
pub trait ProgressReporter: Send + Sync {
    /// Report statistics about the recovery set before starting repair
    fn report_statistics(&self, recovery_set: &RecoverySetInfo);

    /// Report the status of a file being checked
    fn report_file_opening(&self, file_name: &str);

    /// Report the determined status of a file
    fn report_file_status(&self, file_name: &str, status: FileStatus);

    /// Report scanning progress for large files
    fn report_scanning(&self, file_name: &str);

    /// Clear scanning progress line
    fn clear_scanning(&self, file_name: &str);

    /// Report recovery block availability
    fn report_recovery_info(&self, available: usize, needed: usize);

    /// Report that repair is not possible
    fn report_insufficient_recovery(&self, available: usize, needed: usize);

    /// Report repair header
    fn report_repair_header(&self);

    /// Report repair starting for a specific file
    fn report_repair_start(&self, file_name: &str);

    /// Report repair completion for a file
    fn report_repair_complete(&self, file_name: &str, repaired: bool);

    /// Report repair failure for a file
    fn report_repair_failed(&self, file_name: &str, error: &str);

    /// Report verification header
    fn report_verification_header(&self);

    /// Report file verification result
    fn report_verification(&self, file_name: &str, result: VerificationResult);

    /// Report final repair result
    fn report_final_result(&self, result: &RepairResult);
}

/// Console reporter - standard par2cmdline-style output
pub struct ConsoleReporter {
    quiet: bool,
}

impl ConsoleReporter {
    /// Create a new console reporter
    pub fn new(quiet: bool) -> Self {
        Self { quiet }
    }
}

impl ProgressReporter for ConsoleReporter {
    fn report_statistics(&self, recovery_set: &RecoverySetInfo) {
        if self.quiet {
            return;
        }

        println!(
            "There are {} recoverable files and {} recovery blocks.",
            recovery_set.files.len(),
            recovery_set.recovery_slices_metadata.len()
        );
        println!("The block size used was {} bytes.", recovery_set.slice_size);
        println!();
    }

    fn report_file_opening(&self, file_name: &str) {
        if self.quiet {
            return;
        }
        println!("Opening: \"{}\"", file_name);
    }

    fn report_file_status(&self, file_name: &str, status: FileStatus) {
        if self.quiet {
            return;
        }
        let status_str = match status {
            FileStatus::Present => "found.",
            FileStatus::Missing => "missing.",
            FileStatus::Corrupted => "damaged.",
        };
        println!("Target: \"{}\" - {}", file_name, status_str);
    }

    fn report_scanning(&self, file_name: &str) {
        if self.quiet {
            return;
        }
        print!("Scanning: \"{}\"", file_name);
        std::io::Write::flush(&mut std::io::stdout()).unwrap_or(());
    }

    fn clear_scanning(&self, file_name: &str) {
        if self.quiet {
            return;
        }
        print!("\r");
        for _ in 0..(file_name.len() + 12) {
            print!(" ");
        }
        print!("\r");
        std::io::Write::flush(&mut std::io::stdout()).unwrap_or(());
    }

    fn report_recovery_info(&self, available: usize, needed: usize) {
        if self.quiet {
            return;
        }

        println!();
        if needed > 0 {
            println!("You have {} recovery blocks available.", available);
            if needed > available {
                println!("Repair is not possible.");
                println!(
                    "You need {} more recovery blocks to be able to repair.",
                    needed - available
                );
            } else {
                println!("Repair is possible.");
                if available > needed {
                    println!(
                        "You have an excess of {} recovery blocks.",
                        available - needed
                    );
                }
                println!("{} recovery blocks will be used to repair.", needed);
            }
        }
    }

    fn report_insufficient_recovery(&self, available: usize, needed: usize) {
        if self.quiet {
            return;
        }
        println!();
        println!("You have {} recovery blocks available.", available);
        println!("Repair is not possible.");
        println!(
            "You need {} more recovery blocks to be able to repair.",
            needed - available
        );
    }

    fn report_repair_header(&self) {
        if self.quiet {
            return;
        }
        println!();
        println!("Repairing files:");
        println!();
    }

    fn report_repair_start(&self, file_name: &str) {
        if self.quiet {
            return;
        }
        print!("Repairing \"{}\"... ", file_name);
        std::io::Write::flush(&mut std::io::stdout()).unwrap_or(());
    }

    fn report_repair_complete(&self, _file_name: &str, repaired: bool) {
        if self.quiet {
            return;
        }
        if repaired {
            println!("done.");
        } else {
            println!("already valid.");
        }
    }

    fn report_repair_failed(&self, _file_name: &str, error: &str) {
        if self.quiet {
            return;
        }
        println!("FAILED: {}", error);
    }

    fn report_verification_header(&self) {
        if self.quiet {
            return;
        }
        println!();
        println!("Verifying repaired files:");
        println!();
    }

    fn report_verification(&self, file_name: &str, result: VerificationResult) {
        if self.quiet {
            return;
        }
        match result {
            VerificationResult::Verified => {
                println!("Target: \"{}\" - found.", file_name);
            }
            VerificationResult::HashMismatch => {
                println!("Target: \"{}\" - FAILED (MD5 mismatch).", file_name);
            }
            VerificationResult::SizeMismatch { expected, actual } => {
                println!(
                    "Target: \"{}\" - FAILED (size mismatch: expected {}, got {}).",
                    file_name, expected, actual
                );
            }
        }
    }

    fn report_final_result(&self, _result: &RepairResult) {
        // Final result is typically handled by the caller
        // This could print a summary if needed
    }
}

/// Silent reporter - produces no output
pub struct SilentReporter;

impl SilentReporter {
    /// Create a new silent reporter
    pub fn new() -> Self {
        Self
    }
}

impl Default for SilentReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressReporter for SilentReporter {
    fn report_statistics(&self, _recovery_set: &RecoverySetInfo) {}
    fn report_file_opening(&self, _file_name: &str) {}
    fn report_file_status(&self, _file_name: &str, _status: FileStatus) {}
    fn report_scanning(&self, _file_name: &str) {}
    fn clear_scanning(&self, _file_name: &str) {}
    fn report_recovery_info(&self, _available: usize, _needed: usize) {}
    fn report_insufficient_recovery(&self, _available: usize, _needed: usize) {}
    fn report_repair_header(&self) {}
    fn report_repair_start(&self, _file_name: &str) {}
    fn report_repair_complete(&self, _file_name: &str, _repaired: bool) {}
    fn report_repair_failed(&self, _file_name: &str, _error: &str) {}
    fn report_verification_header(&self) {}
    fn report_verification(&self, _file_name: &str, _result: VerificationResult) {}
    fn report_final_result(&self, _result: &RepairResult) {}
}
