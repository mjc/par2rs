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

    /// Report detailed scanning progress with percentage (like par2cmdline)
    fn report_scanning_progress(&self, file_name: &str, bytes_processed: u64, total_bytes: u64);

    /// Clear scanning progress line
    fn clear_scanning(&self, file_name: &str);

    /// Report recovery block availability
    fn report_recovery_info(&self, available: usize, needed: usize);

    /// Report that repair is not possible
    fn report_insufficient_recovery(&self, available: usize, needed: usize);

    /// Report repair header
    fn report_repair_header(&self);

    /// Report loading PAR2 files progress
    fn report_loading_progress(&self, files_loaded: usize, total_files: usize);

    /// Report constructing Reed-Solomon matrix
    fn report_constructing(&self);

    /// Report Reed-Solomon computation progress
    fn report_computing_progress(&self, blocks_processed: usize, total_blocks: usize);

    /// Report repair starting for a specific file
    fn report_repair_start(&self, file_name: &str);

    /// Report file writing progress
    fn report_writing_progress(&self, file_name: &str, bytes_written: u64, total_bytes: u64);

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
        println!("Scanning: \"{}\"", file_name);
        std::io::Write::flush(&mut std::io::stdout()).unwrap_or(());
    }

    fn report_scanning_progress(&self, file_name: &str, bytes_processed: u64, total_bytes: u64) {
        if self.quiet || total_bytes == 0 {
            return;
        }

        // Calculate percentage with higher precision: (10000 * progress / total) for 0.01% precision
        let percentage_100x = ((10000 * bytes_processed) / total_bytes) as u32;
        let percentage = percentage_100x as f64 / 100.0;

        // Format as "Scanning: "filename": XX.XX%\r" with two decimal places
        let truncated_name = if file_name.len() > 45 {
            format!("{}...", &file_name[..42])
        } else {
            file_name.to_string()
        };

        print!("Scanning: \"{}\": {:.2}%\r", truncated_name, percentage);
        std::io::Write::flush(&mut std::io::stdout()).unwrap_or(());
    }

    fn clear_scanning(&self, _file_name: &str) {
        if !self.quiet {
            // No longer needed since we use println! instead of print! with \r
            // Each scanning line is already on its own line
        }
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
        if !self.quiet {
            // Don't print a separate header - sabnzbd expects only "Repairing: XX.X%" format
            // The first progress update will show the repair status
        }
    }

    fn report_loading_progress(&self, files_loaded: usize, total_files: usize) {
        if self.quiet {
            return;
        }
        if files_loaded == 1 {
            print!("Loading PAR2 files");
            std::io::Write::flush(&mut std::io::stdout()).unwrap_or(());
        }
        if files_loaded < total_files {
            print!(".");
            std::io::Write::flush(&mut std::io::stdout()).unwrap_or(());
        } else {
            println!();
        }
    }

    fn report_constructing(&self) {
        if self.quiet {
            return;
        }
        println!("Constructing: done.");
    }

    fn report_computing_progress(&self, blocks_processed: usize, total_blocks: usize) {
        if self.quiet {
            return;
        }
        let percentage = (blocks_processed as f64 / total_blocks as f64) * 100.0;
        // Output format compatible with sabnzbd: "Repairing: XX.X%"
        print!("\rRepairing: {:.1}%", percentage);
        std::io::Write::flush(&mut std::io::stdout()).unwrap_or(());
        if blocks_processed == total_blocks {
            println!();
        }
    }

    fn report_writing_progress(&self, file_name: &str, bytes_written: u64, total_bytes: u64) {
        if self.quiet || total_bytes == 0 {
            return;
        }
        let percentage = (bytes_written as f64 / total_bytes as f64) * 100.0;
        let truncated_name = if file_name.len() > 45 {
            format!("{}...", &file_name[..42])
        } else {
            file_name.to_string()
        };
        print!("\rWriting: \"{}\": {:.1}%", truncated_name, percentage);
        std::io::Write::flush(&mut std::io::stdout()).unwrap_or(());
        if bytes_written == total_bytes {
            println!();
        }
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
    fn report_scanning_progress(&self, _file_name: &str, _bytes_processed: u64, _total_bytes: u64) {
    }
    fn clear_scanning(&self, _file_name: &str) {}
    fn report_recovery_info(&self, _available: usize, _needed: usize) {}
    fn report_insufficient_recovery(&self, _available: usize, _needed: usize) {}
    fn report_repair_header(&self) {}
    fn report_loading_progress(&self, _files_loaded: usize, _total_files: usize) {}
    fn report_constructing(&self) {}
    fn report_computing_progress(&self, _blocks_processed: usize, _total_blocks: usize) {}
    fn report_repair_start(&self, _file_name: &str) {}
    fn report_writing_progress(&self, _file_name: &str, _bytes_written: u64, _total_bytes: u64) {}
    fn report_repair_complete(&self, _file_name: &str, _repaired: bool) {}
    fn report_repair_failed(&self, _file_name: &str, _error: &str) {}
    fn report_verification_header(&self) {}
    fn report_verification(&self, _file_name: &str, _result: VerificationResult) {}
    fn report_final_result(&self, _result: &RepairResult) {}
}
