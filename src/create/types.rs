//! Type definitions for PAR2 creation

use crate::domain::SourceBlockCount;
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
    Limited,
}

/// Configuration for PAR2 creation
#[derive(Debug, Clone)]
pub struct CreateConfig {
    /// Base name for output PAR2 files
    pub output_name: String,

    /// List of source files to protect
    pub source_files: Vec<PathBuf>,

    /// Base path used to derive packet names stored in PAR2 metadata
    pub base_path: Option<PathBuf>,

    /// Block size in bytes (if None, will be auto-calculated from source_block_count)
    /// Reference: par2cmdline -s option
    pub block_size: Option<u64>,

    /// Target number of source blocks (if None, defaults to 2000)
    /// Reference: par2cmdline -b option
    /// If block_size is set, this is ignored. If neither is set, defaults to 2000.
    pub source_block_count: Option<SourceBlockCount>,

    /// Number of recovery blocks to create (if None, calculated from redundancy_percentage)
    pub recovery_block_count: Option<u32>,

    /// Redundancy percentage (positive, typically 5-10)
    /// Used to calculate recovery_block_count if not specified
    pub redundancy_percentage: Option<u32>,

    /// Target total recovery data size in bytes (-rk/-rm/-rg)
    pub recovery_target_size: Option<u64>,

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
            base_path: None,
            block_size: None,
            source_block_count: None,
            recovery_block_count: None,
            redundancy_percentage: Some(5), // 5% is typical default
            recovery_target_size: None,
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
            if pct == 0 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn valid_config() -> CreateConfig {
        CreateConfig {
            output_name: "out.par2".to_string(),
            source_files: vec![PathBuf::from("a.dat")],
            ..CreateConfig::default()
        }
    }

    // --- Default ---

    #[test]
    fn default_has_expected_values() {
        let c = CreateConfig::default();
        assert!(c.output_name.is_empty());
        assert!(c.source_files.is_empty());
        assert_eq!(c.redundancy_percentage, Some(5));
        assert_eq!(c.thread_count, 0);
        assert_eq!(c.first_recovery_block, 0);
        assert_eq!(c.recovery_file_scheme, RecoveryFileScheme::Variable);
    }

    // --- validate() ---

    #[test]
    fn validate_rejects_empty_output_name() {
        let c = CreateConfig {
            output_name: String::new(),
            source_files: vec![PathBuf::from("a.dat")],
            ..CreateConfig::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn validate_rejects_no_source_files() {
        let c = CreateConfig {
            output_name: "out.par2".to_string(),
            ..CreateConfig::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn validate_rejects_zero_redundancy() {
        let c = CreateConfig {
            redundancy_percentage: Some(0),
            ..valid_config()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn validate_accepts_over_100_redundancy() {
        let c = CreateConfig {
            redundancy_percentage: Some(101),
            ..valid_config()
        };
        assert!(c.validate().is_ok());
    }

    #[test]
    fn validate_accepts_100_redundancy() {
        let c = CreateConfig {
            redundancy_percentage: Some(100),
            ..valid_config()
        };
        assert!(c.validate().is_ok());
    }

    #[test]
    fn validate_rejects_block_size_not_multiple_of_4() {
        let c = CreateConfig {
            block_size: Some(10),
            ..valid_config()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn validate_rejects_block_size_zero() {
        let c = CreateConfig {
            block_size: Some(0),
            ..valid_config()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn validate_accepts_valid_block_size() {
        let c = CreateConfig {
            block_size: Some(4096),
            ..valid_config()
        };
        assert!(c.validate().is_ok());
    }

    #[test]
    fn validate_ok_without_redundancy_or_block_count() {
        // Neither redundancy_percentage nor recovery_block_count — validate() doesn't
        // require one (that check is in calculate_recovery_blocks)
        let c = CreateConfig {
            redundancy_percentage: None,
            recovery_block_count: None,
            ..valid_config()
        };
        assert!(c.validate().is_ok());
    }

    // --- effective_threads() ---

    #[test]
    fn effective_threads_zero_returns_at_least_one() {
        let c = CreateConfig {
            thread_count: 0,
            ..valid_config()
        };
        assert!(c.effective_threads() >= 1);
    }

    #[test]
    fn effective_threads_nonzero_returns_exact() {
        let c = CreateConfig {
            thread_count: 4,
            ..valid_config()
        };
        assert_eq!(c.effective_threads(), 4);
    }
}
