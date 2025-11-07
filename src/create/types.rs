//! Type definitions for PAR2 creation

use std::path::PathBuf;

/// Recovery file scheme determines how recovery blocks are distributed across files
///
/// Reference: par2cmdline-turbo/src/commandline.h Scheme enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecoveryFileScheme {
    /// Create recovery files of uniform size
    /// Example: 100 blocks -> 10 files of 10 blocks each
    Uniform,

    /// Create recovery files with variable sizes (powers of 2)
    /// Example: 100 blocks -> 1, 2, 4, 8, 16, 32, 37 blocks
    /// This allows efficient recovery of different amounts of damage
    #[default]
    Variable,

    /// Create limited number of recovery files
    /// Distributes blocks as evenly as possible across specified file count
    Limited(u32),
}

/// Configuration for PAR2 creation
#[derive(Debug, Clone)]
pub struct CreateConfig {
    /// Base name for output PAR2 files
    pub output_name: String,

    /// List of source files to protect
    pub source_files: Vec<PathBuf>,

    /// Block size in bytes (if None, will be auto-calculated)
    pub block_size: Option<u64>,

    /// Number of recovery blocks to create (if None, calculated from redundancy_percentage)
    pub recovery_block_count: Option<u32>,

    /// Redundancy percentage (1-100, typically 5-10)
    /// Used to calculate recovery_block_count if not specified
    pub redundancy_percentage: Option<u32>,

    /// Recovery file distribution scheme
    pub recovery_file_scheme: RecoveryFileScheme,

    /// Number of recovery files to create (for Limited scheme)
    pub recovery_file_count: Option<u32>,

    /// Memory limit for processing (bytes)
    /// If None, uses reasonable default based on available memory
    pub memory_limit: Option<usize>,

    /// Number of threads for computation (0 = auto-detect)
    pub thread_count: u32,

    /// First recovery block exponent (typically 0)
    /// Advanced option for compatibility
    pub first_recovery_block: u32,
}

impl Default for CreateConfig {
    fn default() -> Self {
        CreateConfig {
            output_name: String::new(),
            source_files: Vec::new(),
            block_size: None,
            recovery_block_count: None,
            redundancy_percentage: Some(5), // 5% is typical default
            recovery_file_scheme: RecoveryFileScheme::default(),
            recovery_file_count: None,
            memory_limit: None,
            thread_count: 0, // Auto-detect
            first_recovery_block: 0,
        }
    }
}

impl CreateConfig {
    /// Validate the configuration
    pub fn validate(&self) -> Result<(), super::CreateError> {
        use super::CreateError;

        if self.output_name.is_empty() {
            return Err(CreateError::Other("Output name not specified".to_string()));
        }

        if self.source_files.is_empty() {
            return Err(CreateError::NoSourceFiles);
        }

        // Validate redundancy percentage if specified
        if let Some(pct) = self.redundancy_percentage {
            if !(1..=100).contains(&pct) {
                return Err(CreateError::InvalidRedundancy(pct));
            }
        }

        // Validate block size if specified
        if let Some(bs) = self.block_size {
            if bs < 1 || bs % 4 != 0 {
                return Err(CreateError::InvalidBlockSize(
                    "Block size must be a multiple of 4".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Get effective thread count (resolve 0 to actual CPU count)
    pub fn effective_threads(&self) -> usize {
        if self.thread_count == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        } else {
            self.thread_count as usize
        }
    }
}
