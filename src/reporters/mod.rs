//! Progress and output reporting for PAR2 operations
//!
//! This module provides traits and implementations for reporting progress and results
//! across different PAR2 operations (verification, repair). It allows the core logic
//! to be decoupled from output formatting.

mod console;
mod silent;

pub use console::{ConsoleRepairReporter, ConsoleVerificationReporter};
pub use silent::{SilentRepairReporter, SilentVerificationReporter};

use crate::verify::{FileStatus, VerificationResults};

/// Base trait for all reporters
///
/// Provides common functionality that all reporters should have, regardless of
/// the specific operation (verification, repair, etc.)
pub trait Reporter: Send + Sync {
    /// Report general progress with a message and completion percentage
    fn report_progress(&self, message: &str, progress: f64);

    /// Report an error that occurred during operation
    fn report_error(&self, error: &str);

    /// Report successful completion of an operation
    fn report_complete(&self, message: &str);
}

/// Trait for reporting verification progress and results
///
/// Extends the base Reporter trait with verification-specific methods
pub trait VerificationReporter: Reporter {
    /// Report starting verification with configuration
    fn report_verification_start(&self, parallel: bool);

    /// Report the number of files found to verify
    fn report_files_found(&self, count: usize);

    /// Report verifying a specific file
    fn report_verifying_file(&self, file_name: &str);

    /// Report the determined status of a file
    fn report_file_status(&self, file_name: &str, status: FileStatus);

    /// Report detailed block damage information for a file
    fn report_damaged_blocks(&self, file_name: &str, damaged_blocks: &[u32]);

    /// Report final verification results summary
    fn report_verification_results(&self, results: &VerificationResults);
}

/// Trait for reporting repair progress and results
///
/// Extends the base Reporter trait with repair-specific methods
pub trait RepairReporter: Reporter {
    /// Report starting repair operation
    fn report_repair_start(&self, files_to_repair: usize);

    /// Report progress repairing a specific file
    fn report_repair_progress(&self, file_name: &str, progress: f64);

    /// Report that a file has been successfully repaired
    fn report_file_repaired(&self, file_name: &str);

    /// Report that repair failed for a file
    fn report_repair_failed(&self, file_name: &str, error: &str);

    /// Report final repair results summary
    fn report_repair_complete(&self, total_files: usize, successful: usize, failed: usize);
}
