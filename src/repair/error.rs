//! Error types for PAR2 repair operations

use thiserror::Error;

/// Errors that can occur during PAR2 repair operations
#[derive(Debug, Error)]
pub enum RepairError {
    /// No main packet found in PAR2 file
    #[error("No main packet found in PAR2 file")]
    NoMainPacket,

    /// No file description packets found
    #[error("No file description packets found")]
    NoFileDescriptions,

    /// File ID from main packet not found in file descriptions
    #[error("File ID {0:?} in main packet not found in file descriptions")]
    MissingFileDescription(String),

    /// No valid PAR2 packets found
    #[error("No valid PAR2 packets found")]
    NoValidPackets,

    /// Failed to create repair context
    #[error("Failed to create repair context: {0}")]
    ContextCreation(String),

    /// Insufficient recovery blocks available
    #[error(
        "Cannot repair: {missing} missing slices but only {available} recovery slices available"
    )]
    InsufficientRecovery { missing: usize, available: usize },

    /// File validation cache not found
    #[error("No validation cache for file {0}")]
    NoValidationCache(String),

    /// File MD5 mismatch despite valid slices
    #[error("File MD5 mismatch despite all slices being valid")]
    Md5MismatchWithValidSlices,

    /// Repaired file failed verification
    #[error("Repaired file failed verification: {0:?}")]
    VerificationFailed(super::VerificationResult),

    /// Reconstruction failed
    #[error("Reconstruction failed: {0}")]
    ReconstructionFailed(String),

    /// Slice marked valid but source file not available
    #[error("Slice {0} marked valid but source file not available")]
    ValidSliceMissingSource(usize),

    /// Slice neither reconstructed nor valid
    #[error("Slice {0} neither reconstructed nor valid")]
    SliceNotAvailable(usize),

    /// Written bytes don't match expected file length
    #[error("Byte count mismatch: wrote {written} bytes, expected {expected}")]
    ByteCountMismatch { written: u64, expected: u64 },

    /// File does not exist
    #[error("File does not exist: {0}")]
    FileNotFound(String),

    /// I/O error occurred
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Type alias for Result with RepairError
pub type Result<T> = std::result::Result<T, RepairError>;
