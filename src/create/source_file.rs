//! Source file information for PAR2 creation

use crate::domain::{FileId, Md5Hash};
use std::path::PathBuf;

/// Information about a source file being protected
///
/// Reference: par2cmdline-turbo/src/par2creatorsourcefile.h
#[derive(Debug, Clone)]
pub struct SourceFileInfo {
    /// Unique file identifier (MD5 hash of [Hash16k, Length, Name])
    pub file_id: FileId,

    /// Path to the source file
    pub path: PathBuf,

    /// File size in bytes
    pub size: u64,

    /// MD5 hash of entire file
    pub hash: Md5Hash,

    /// Index of this file in the recovery set
    pub index: usize,

    /// Block checksums for this file's blocks
    pub block_checksums: Vec<BlockChecksum>,

    /// Global offset of this file's first block in recovery set
    pub global_block_offset: u32,

    /// Number of blocks in this file
    pub block_count: u32,
}

/// Checksum information for a single block
#[derive(Debug, Clone)]
pub struct BlockChecksum {
    /// CRC32 of block data
    pub crc32: u32,

    /// MD5 hash of block data
    pub hash: Md5Hash,

    /// Global block index in recovery set
    pub global_index: u32,
}

impl SourceFileInfo {
    /// Create a new source file info with basic metadata
    pub fn new(path: PathBuf, size: u64, index: usize) -> Self {
        SourceFileInfo {
            file_id: FileId::new([0u8; 16]), // Will be computed during hashing
            path,
            size,
            hash: Md5Hash::new([0u8; 16]), // Will be computed during hashing
            index,
            block_checksums: Vec::new(),
            global_block_offset: 0,
            block_count: 0,
        }
    }

    /// Calculate number of blocks for this file given a block size
    pub fn calculate_block_count(&self, block_size: u64) -> u32 {
        if self.size == 0 {
            0
        } else {
            self.size.div_ceil(block_size) as u32
        }
    }

    /// Get the filename
    pub fn filename(&self) -> String {
        self.path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    }
}
