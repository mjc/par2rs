//! Error types for verification operations

use std::fmt;

/// Custom error types for file verification operations
#[derive(Debug, Clone)]
pub enum VerificationError {
    /// I/O error when accessing files
    Io(String),
    /// Error calculating checksums
    ChecksumCalculation(String),
    /// Invalid file metadata
    InvalidMetadata(String),
    /// Corrupted or invalid file data
    CorruptedData(String),
}

impl fmt::Display for VerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerificationError::Io(msg) => write!(f, "I/O error: {}", msg),
            VerificationError::ChecksumCalculation(msg) => {
                write!(f, "Checksum calculation error: {}", msg)
            }
            VerificationError::InvalidMetadata(msg) => write!(f, "Invalid metadata: {}", msg),
            VerificationError::CorruptedData(msg) => write!(f, "Corrupted data: {}", msg),
        }
    }
}

impl std::error::Error for VerificationError {}

impl From<std::io::Error> for VerificationError {
    fn from(error: std::io::Error) -> Self {
        VerificationError::Io(error.to_string())
    }
}

/// Type alias for verification results
pub type VerificationResult<T> = Result<T, VerificationError>;
