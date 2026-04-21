//! Error types for PAR2 creation

use thiserror::Error;

/// Result type for create operations
pub type CreateResult<T> = Result<T, CreateError>;

/// Errors that can occur during PAR2 creation
#[derive(Error, Debug)]
pub enum CreateError {
    /// Failed to read source file
    #[error("Failed to read source file: {file}: {source}")]
    FileReadError {
        file: String,
        source: std::io::Error,
    },

    /// Failed to create output file
    #[error("Failed to create output file: {file}: {source}")]
    FileCreateError {
        file: String,
        source: std::io::Error,
    },

    /// Source file not found
    #[error("Source file not found: {0}")]
    FileNotFound(String),

    /// Invalid block size
    #[error("Invalid block size: {0}")]
    InvalidBlockSize(String),

    /// Invalid redundancy percentage
    #[error("Invalid redundancy percentage: {0}% (must be positive)")]
    InvalidRedundancy(u32),

    /// No source files specified
    #[error("No source files specified")]
    NoSourceFiles,

    /// Total source file size is zero
    #[error("Total source file size is zero (empty files only)")]
    EmptySourceFiles,

    /// Reed-Solomon encoding error
    #[error("Reed-Solomon encoding failed: {0}")]
    ReedSolomonError(String),

    /// Packet generation error
    #[error("Failed to generate packet: {0}")]
    PacketGenerationError(String),

    /// I/O error during creation
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Generic error
    #[error("{0}")]
    Other(String),
}
