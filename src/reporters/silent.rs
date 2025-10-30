//! Silent reporters for PAR2 operations
//!
//! Provides no-output implementations for testing or when quiet operation is desired.

use super::{RepairReporter, Reporter, VerificationReporter};
use crate::verify::{FileStatus, VerificationResults};

/// Silent implementation for verification operations
#[derive(Default)]
pub struct SilentVerificationReporter;

impl SilentVerificationReporter {
    pub fn new() -> Self {
        Self
    }
}

/// Silent implementation for repair operations
#[derive(Default)]
pub struct SilentRepairReporter;

impl SilentRepairReporter {
    pub fn new() -> Self {
        Self
    }
}

// Base Reporter implementation for SilentVerificationReporter
impl Reporter for SilentVerificationReporter {
    fn report_progress(&self, _message: &str, _progress: f64) {}
    fn report_error(&self, _error: &str) {}
    fn report_complete(&self, _message: &str) {}
}

impl VerificationReporter for SilentVerificationReporter {
    fn report_verification_start(&self, _parallel: bool) {}
    fn report_files_found(&self, _count: usize) {}
    fn report_verifying_file(&self, _file_name: &str) {}
    fn report_file_status(&self, _file_name: &str, _status: FileStatus) {}
    fn report_damaged_blocks(&self, _file_name: &str, _damaged_blocks: &[u32]) {}
    fn report_verification_results(&self, _results: &VerificationResults) {}
}

// Base Reporter implementation for SilentRepairReporter
impl Reporter for SilentRepairReporter {
    fn report_progress(&self, _message: &str, _progress: f64) {}
    fn report_error(&self, _error: &str) {}
    fn report_complete(&self, _message: &str) {}
}

impl RepairReporter for SilentRepairReporter {
    fn report_repair_start(&self, _files_to_repair: usize) {}
    fn report_repair_progress(&self, _file_name: &str, _progress: f64) {}
    fn report_file_repaired(&self, _file_name: &str) {}
    fn report_repair_failed(&self, _file_name: &str, _error: &str) {}
    fn report_repair_complete(&self, _total_files: usize, _successful: usize, _failed: usize) {}
}
