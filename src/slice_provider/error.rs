//! Error types for slice provider operations

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during slice provider operations
#[derive(Error, Debug)]
pub enum SliceProviderError {
    /// Slice index not found in provider
    #[error("Slice {index} not found")]
    SliceNotFound { index: usize },

    /// Chunk offset exceeds logical slice size
    #[error("Chunk offset {offset} exceeds logical slice size {slice_size}")]
    InvalidChunkOffset { offset: usize, slice_size: usize },

    /// Failed to open file for reading
    #[error("Failed to open file {path}: {source}")]
    FileOpenError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to seek to position in file
    #[error("Failed to seek to offset {offset} in {path}: {source}")]
    FileSeekError {
        path: PathBuf,
        offset: u64,
        #[source]
        source: std::io::Error,
    },

    /// Failed to read from file
    #[error("Failed to read from {path}: {source}")]
    FileReadError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Recovery slice not found
    #[error("Recovery slice with exponent {exponent} not found")]
    RecoverySliceNotFound { exponent: usize },

    /// Failed to load recovery chunk
    #[error("Failed to load recovery chunk at offset {offset}: {source}")]
    RecoveryChunkLoadError {
        offset: usize,
        #[source]
        source: std::io::Error,
    },
}

/// Result type for slice provider operations
pub type Result<T> = std::result::Result<T, SliceProviderError>;
