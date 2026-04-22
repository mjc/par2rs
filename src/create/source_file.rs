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

    /// PAR2 packet name, normalized once during source discovery
    pub packet_name: String,

    /// File size in bytes
    pub size: u64,

    /// MD5 hash of entire file
    pub hash: Md5Hash,

    /// MD5 hash of first 16KB of file (for quick matching)
    pub hash_16k: Md5Hash,

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
        let packet_name = packet_name_from_path(&path);
        Self::new_with_packet_name(path, packet_name, size, index)
    }

    /// Create a new source file info with a precomputed PAR2 packet name
    pub fn new_with_packet_name(
        path: PathBuf,
        packet_name: String,
        size: u64,
        index: usize,
    ) -> Self {
        SourceFileInfo {
            file_id: FileId::new([0u8; 16]), // Will be computed during hashing
            path,
            packet_name,
            size,
            hash: Md5Hash::new([0u8; 16]), // Will be computed during hashing
            hash_16k: Md5Hash::new([0u8; 16]), // Will be computed during hashing
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
        self.packet_name.clone()
    }

    /// Get the normalized PAR2 packet name without allocating.
    pub fn packet_name(&self) -> &str {
        &self.packet_name
    }
}

pub(crate) fn packet_name_from_path(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string()
}

pub(crate) fn normalize_packet_path(path: &std::path::Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // --- new() ---

    #[test]
    fn new_sets_fields_correctly() {
        let path = PathBuf::from("/tmp/test.dat");
        let info = SourceFileInfo::new(path.clone(), 1024, 3);

        assert_eq!(info.path, path);
        assert_eq!(info.packet_name(), "test.dat");
        assert_eq!(info.size, 1024);
        assert_eq!(info.index, 3);
        assert_eq!(info.block_count, 0);
        assert_eq!(info.global_block_offset, 0);
        assert!(info.block_checksums.is_empty());
    }

    // --- calculate_block_count() ---

    #[test]
    fn block_count_exact_multiple() {
        let info = SourceFileInfo::new(PathBuf::from("a.dat"), 1024, 0);
        assert_eq!(info.calculate_block_count(512), 2);
    }

    #[test]
    fn block_count_rounds_up() {
        let info = SourceFileInfo::new(PathBuf::from("a.dat"), 1025, 0);
        assert_eq!(info.calculate_block_count(512), 3);
    }

    #[test]
    fn block_count_smaller_than_block_size() {
        let info = SourceFileInfo::new(PathBuf::from("a.dat"), 100, 0);
        assert_eq!(info.calculate_block_count(512), 1);
    }

    #[test]
    fn block_count_zero_size_file() {
        let info = SourceFileInfo::new(PathBuf::from("a.dat"), 0, 0);
        assert_eq!(info.calculate_block_count(512), 0);
    }

    #[test]
    fn block_count_exactly_one_block() {
        let info = SourceFileInfo::new(PathBuf::from("a.dat"), 512, 0);
        assert_eq!(info.calculate_block_count(512), 1);
    }

    // --- filename() ---

    #[test]
    fn filename_returns_just_name() {
        let info = SourceFileInfo::new(PathBuf::from("/some/dir/file.dat"), 0, 0);
        assert_eq!(info.filename(), "file.dat");
    }

    #[test]
    fn filename_bare_name() {
        let info = SourceFileInfo::new(PathBuf::from("bare.txt"), 0, 0);
        assert_eq!(info.filename(), "bare.txt");
    }

    #[test]
    fn new_with_packet_name_uses_precomputed_name() {
        let info = SourceFileInfo::new_with_packet_name(
            PathBuf::from("/base/nested/file.dat"),
            "nested/file.dat".to_string(),
            0,
            0,
        );
        assert_eq!(info.packet_name(), "nested/file.dat");
        assert_eq!(info.filename(), "nested/file.dat");
    }
}
