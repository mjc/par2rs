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

#[cfg(test)]
mod tests {
    use super::{VerificationError, VerificationResult};

    #[test]
    fn display_messages_match_error_variant() {
        let cases = [
            (
                VerificationError::Io("disk failed".to_string()),
                "I/O error: disk failed",
            ),
            (
                VerificationError::ChecksumCalculation("bad md5".to_string()),
                "Checksum calculation error: bad md5",
            ),
            (
                VerificationError::InvalidMetadata("missing packet".to_string()),
                "Invalid metadata: missing packet",
            ),
            (
                VerificationError::CorruptedData("wrong crc".to_string()),
                "Corrupted data: wrong crc",
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn io_errors_convert_and_type_alias_is_usable() {
        let error = VerificationError::from(std::io::Error::other("boom"));
        assert!(matches!(error, VerificationError::Io(message) if message.contains("boom")));

        let result: VerificationResult<u32> = Ok(7);
        assert_eq!(result.unwrap(), 7);
    }
}
