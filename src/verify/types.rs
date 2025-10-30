//! Type definitions for verification operations

use crate::domain::{Crc32Value, FileId, Md5Hash};
use std::fmt;

/// Unified file verification status used by both verify and repair operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    /// File is perfect match
    Present,
    /// File exists but has wrong name (verify only)
    Renamed,
    /// File exists but is corrupted
    Corrupted,
    /// File is completely missing
    Missing,
}

impl FileStatus {
    /// Returns true if the file needs repair (missing or corrupted)
    pub fn needs_repair(&self) -> bool {
        matches!(self, FileStatus::Missing | FileStatus::Corrupted)
    }
}

impl fmt::Display for FileStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileStatus::Present => write!(f, "present"),
            FileStatus::Renamed => write!(f, "renamed"),
            FileStatus::Corrupted => write!(f, "corrupted"),
            FileStatus::Missing => write!(f, "missing"),
        }
    }
}

/// Block verification result
#[derive(Debug, Clone)]
pub struct BlockVerificationResult {
    pub block_number: u32,
    pub file_id: FileId,
    pub is_valid: bool,
    pub expected_hash: Option<Md5Hash>,
    pub expected_crc: Option<Crc32Value>,
}

/// Comprehensive verification results
#[derive(Debug, Clone)]
pub struct VerificationResults {
    pub files: Vec<FileVerificationResult>,
    pub blocks: Vec<BlockVerificationResult>,
    pub present_file_count: usize,
    pub renamed_file_count: usize,
    pub corrupted_file_count: usize,
    pub missing_file_count: usize,
    pub available_block_count: usize,
    pub missing_block_count: usize,
    pub total_block_count: usize,
    pub recovery_blocks_available: usize,
    pub repair_possible: bool,
    pub blocks_needed_for_repair: usize,
}

impl fmt::Display for VerificationResults {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Verification Results:")?;
        writeln!(f, "====================")?;

        // Functional file status reporting
        [
            (self.present_file_count, "file(s) are ok."),
            (self.renamed_file_count, "file(s) have the wrong name."),
            (self.corrupted_file_count, "file(s) exist but are damaged."),
            (self.missing_file_count, "file(s) are missing."),
        ]
        .iter()
        .filter(|(count, _)| *count > 0)
        .try_for_each(|(count, message)| writeln!(f, "{} {}", count, message))?;

        writeln!(
            f,
            "You have {} out of {} data blocks available.",
            self.available_block_count, self.total_block_count
        )?;

        // Recovery blocks message (functional approach)
        if self.recovery_blocks_available > 0 {
            writeln!(
                f,
                "You have {} recovery blocks available.",
                self.recovery_blocks_available
            )?;
        }

        // Repair status using functional pattern matching
        match (self.missing_block_count, self.repair_possible) {
            (0, _) => writeln!(f, "All files are correct, repair is not required.")?,
            (missing, true) => {
                writeln!(f, "Repair is possible.")?;
                if self.recovery_blocks_available > missing {
                    writeln!(
                        f,
                        "You have an excess of {} recovery blocks.",
                        self.recovery_blocks_available - missing
                    )?;
                }
                writeln!(f, "{} recovery blocks will be used to repair.", missing)?;
            }
            (missing, false) => {
                writeln!(f, "Repair is not possible.")?;
                writeln!(
                    f,
                    "You need {} more recovery blocks to be able to repair.",
                    missing - self.recovery_blocks_available
                )?;
            }
        }

        Ok(())
    }
}

/// Individual file verification result  
#[derive(Debug, Clone)]
pub struct FileVerificationResult {
    pub file_name: String,
    pub file_id: FileId,
    pub status: FileStatus,
    pub blocks_available: usize,
    pub total_blocks: usize,
    pub damaged_blocks: Vec<u32>,
}
