//! Global block table for efficient block verification and repair
//!
//! This module implements a global block verification table similar to par2cmdline's
//! VerificationHashTable. It provides efficient lookup and verification of blocks
//! across all files in a recovery set.

use crate::domain::{Crc32Value, FileId, Md5Hash};
use rustc_hash::FxHashMap as HashMap;
use std::collections::hash_map::Entry;

/// Global position of a block within the recovery set
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlobalBlockPosition {
    /// File ID that contains this block
    pub file_id: FileId,
    /// Block number within the file (0-based)
    pub block_number: u32,
    /// Whether this is the first block of the file
    pub is_first_block: bool,
}

/// Block checksums for fast lookup and verification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockChecksums {
    /// MD5 hash of the block content
    pub md5_hash: Md5Hash,
    /// CRC32 checksum of the block content
    pub crc32: Crc32Value,
}

/// Entry in the global block table
#[derive(Debug, Clone)]
pub struct GlobalBlockEntry {
    /// Position of this block
    pub position: GlobalBlockPosition,
    /// Checksums for verification
    pub checksums: BlockChecksums,
    /// Size of the block in bytes
    pub block_size: u64,
    /// Next entry with same checksums (for handling duplicates)
    pub next_duplicate: Option<Box<GlobalBlockEntry>>,
}

impl GlobalBlockEntry {
    /// Create a new block entry
    pub fn new(
        file_id: FileId,
        block_number: u32,
        is_first_block: bool,
        md5_hash: Md5Hash,
        crc32: Crc32Value,
        block_size: u64,
    ) -> Self {
        Self {
            position: GlobalBlockPosition {
                file_id,
                block_number,
                is_first_block,
            },
            checksums: BlockChecksums { md5_hash, crc32 },
            block_size,
            next_duplicate: None,
        }
    }

    /// Add a duplicate entry (same checksums, different position)
    pub fn add_duplicate(&mut self, other: GlobalBlockEntry) {
        match self.next_duplicate.as_mut() {
            Some(next) => next.add_duplicate(other),
            None => self.next_duplicate = Some(Box::new(other)),
        }
    }

    /// Iterate over all entries with the same checksums (including self)
    pub fn iter_duplicates(&self) -> impl Iterator<Item = &GlobalBlockEntry> {
        std::iter::successors(Some(self), |entry| {
            entry.next_duplicate.as_ref().map(|b| b.as_ref())
        })
    }
}

/// Statistics about the global block table
#[derive(Debug, Clone, Default)]
pub struct GlobalTableStats {
    /// Total number of blocks in the table
    pub total_blocks: usize,
    /// Number of unique block checksums
    pub unique_checksums: usize,
    /// Number of duplicate blocks (same content)
    pub duplicate_blocks: usize,
    /// Number of files represented in the table
    pub file_count: usize,
}

/// Global block verification table
///
/// This table provides fast lookup of blocks by their checksums, similar to
/// par2cmdline's VerificationHashTable. It supports:
/// - O(1) lookup by CRC32 for initial filtering
/// - Full MD5 verification for confirmed matches
/// - Handling of duplicate blocks (same content in multiple locations)
/// - Efficient iteration over all blocks
pub struct GlobalBlockTable {
    /// Primary hash table indexed by CRC32 for fast lookup
    crc_table: HashMap<Crc32Value, Vec<GlobalBlockEntry>>,
    /// Secondary index by MD5 for exact matching
    md5_table: HashMap<Md5Hash, Vec<GlobalBlockEntry>>,
    /// Statistics about the table
    stats: GlobalTableStats,
    /// Block size for this recovery set
    block_size: u64,
}

impl GlobalBlockTable {
    /// Create a new global block table
    pub fn new(block_size: u64) -> Self {
        Self {
            crc_table: HashMap::default(),
            md5_table: HashMap::default(),
            stats: GlobalTableStats::default(),
            block_size,
        }
    }

    /// Add a block to the table
    pub fn add_block(
        &mut self,
        file_id: FileId,
        block_number: u32,
        md5_hash: Md5Hash,
        crc32: Crc32Value,
        block_size: u64,
    ) {
        let is_first_block = block_number == 0;
        let entry = GlobalBlockEntry::new(
            file_id,
            block_number,
            is_first_block,
            md5_hash,
            crc32,
            block_size,
        );

        // Update statistics
        self.stats.total_blocks += 1;

        // Add to CRC table
        match self.crc_table.entry(crc32) {
            Entry::Occupied(mut occupied) => {
                // Check if we already have this exact block (MD5 match)
                let existing_entries = occupied.get_mut();
                let mut found_duplicate = false;

                for existing in existing_entries.iter_mut() {
                    if existing.checksums.md5_hash == md5_hash {
                        // Found duplicate - add to chain
                        existing.add_duplicate(entry.clone());
                        self.stats.duplicate_blocks += 1;
                        found_duplicate = true;
                        break;
                    }
                }

                if !found_duplicate {
                    existing_entries.push(entry);
                    self.stats.unique_checksums += 1;
                }
            }
            Entry::Vacant(vacant) => {
                vacant.insert(vec![entry]);
                self.stats.unique_checksums += 1;
            }
        }

        // Add to MD5 table for direct MD5 lookups
        self.md5_table
            .entry(md5_hash)
            .or_default()
            .push(GlobalBlockEntry::new(
                file_id,
                block_number,
                is_first_block,
                md5_hash,
                crc32,
                block_size,
            ));
    }

    /// Find blocks matching the given CRC32
    #[inline(always)]
    pub fn find_by_crc32(&self, crc32: Crc32Value) -> Option<&[GlobalBlockEntry]> {
        // Direct HashMap lookup - FxHashMap is very fast for integer keys
        self.crc_table.get(&crc32).map(|v| v.as_slice())
    }

    /// Find blocks matching the given MD5 hash
    pub fn find_by_md5(&self, md5_hash: &Md5Hash) -> Option<&[GlobalBlockEntry]> {
        self.md5_table.get(md5_hash).map(|v| v.as_slice())
    }

    /// Find exact block match by both CRC32 and MD5
    pub fn find_exact_match(
        &self,
        md5_hash: &Md5Hash,
        crc32: Crc32Value,
    ) -> Option<&GlobalBlockEntry> {
        self.crc_table.get(&crc32).and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry.checksums.md5_hash == *md5_hash)
        })
    }

    /// Get all entries for a specific file
    pub fn get_file_blocks(&self, file_id: FileId) -> Vec<&GlobalBlockEntry> {
        let mut file_blocks = Vec::new();

        for entries in self.crc_table.values() {
            for entry in entries {
                for duplicate in entry.iter_duplicates() {
                    if duplicate.position.file_id == file_id {
                        file_blocks.push(duplicate);
                    }
                }
            }
        }

        // Sort by block number for consistent ordering
        file_blocks.sort_by_key(|entry| entry.position.block_number);
        file_blocks
    }

    /// Get statistics about the table
    pub fn stats(&self) -> &GlobalTableStats {
        &self.stats
    }

    /// Get the block size for this table
    pub fn block_size(&self) -> u64 {
        self.block_size
    }

    /// Iterate over all unique blocks in the table
    pub fn iter_blocks(&self) -> impl Iterator<Item = &GlobalBlockEntry> {
        self.crc_table.values().flatten()
    }

    /// Check if the table contains any blocks for verification
    pub fn is_empty(&self) -> bool {
        self.crc_table.is_empty()
    }

    /// Update file count statistics
    pub fn update_file_count(&mut self, count: usize) {
        self.stats.file_count = count;
    }
}

/// Builder for constructing a global block table from packet data
pub struct GlobalBlockTableBuilder {
    table: GlobalBlockTable,
    file_count: usize,
}

impl GlobalBlockTableBuilder {
    /// Create a new builder
    pub fn new(block_size: u64) -> Self {
        Self {
            table: GlobalBlockTable::new(block_size),
            file_count: 0,
        }
    }

    /// Add all blocks from a file's slice checksums
    pub fn add_file_blocks(&mut self, file_id: FileId, slice_checksums: &[(Md5Hash, Crc32Value)]) {
        self.file_count += 1;

        for (block_number, (md5_hash, crc32)) in slice_checksums.iter().enumerate() {
            self.table.add_block(
                file_id,
                block_number as u32,
                *md5_hash,
                *crc32,
                self.table.block_size,
            );
        }
    }

    /// Build the final global block table
    pub fn build(mut self) -> GlobalBlockTable {
        self.table.update_file_count(self.file_count);
        self.table
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_block_table_creation() {
        let table = GlobalBlockTable::new(1024);
        assert_eq!(table.block_size(), 1024);
        assert!(table.is_empty());
        assert_eq!(table.stats().total_blocks, 0);
    }

    #[test]
    fn test_add_and_find_block() {
        let mut table = GlobalBlockTable::new(1024);
        let file_id = FileId::new([1; 16]);
        let md5_hash = Md5Hash::new([2; 16]);
        let crc32 = Crc32Value::new(12345);

        table.add_block(file_id, 0, md5_hash, crc32, 1024);

        // Test CRC32 lookup
        let crc_results = table.find_by_crc32(crc32);
        assert!(crc_results.is_some());
        assert_eq!(crc_results.unwrap().len(), 1);

        // Test MD5 lookup
        let md5_results = table.find_by_md5(&md5_hash);
        assert!(md5_results.is_some());
        assert_eq!(md5_results.unwrap().len(), 1);

        // Test exact match
        let exact_match = table.find_exact_match(&md5_hash, crc32);
        assert!(exact_match.is_some());
        assert_eq!(exact_match.unwrap().position.file_id, file_id);
        assert_eq!(exact_match.unwrap().position.block_number, 0);
        assert!(exact_match.unwrap().position.is_first_block);
    }

    #[test]
    fn test_duplicate_blocks() {
        let mut table = GlobalBlockTable::new(1024);
        let file_id1 = FileId::new([1; 16]);
        let file_id2 = FileId::new([2; 16]);
        let md5_hash = Md5Hash::new([3; 16]);
        let crc32 = Crc32Value::new(54321);

        // Add same block content to two different files
        table.add_block(file_id1, 0, md5_hash, crc32, 1024);
        table.add_block(file_id2, 5, md5_hash, crc32, 1024);

        let stats = table.stats();
        assert_eq!(stats.total_blocks, 2);
        assert_eq!(stats.unique_checksums, 1);
        assert_eq!(stats.duplicate_blocks, 1);

        // Both should be findable
        let crc_results = table.find_by_crc32(crc32).unwrap();
        assert_eq!(crc_results.len(), 1);

        // Check duplicate chain
        let entry = &crc_results[0];
        let duplicates: Vec<_> = entry.iter_duplicates().collect();
        assert_eq!(duplicates.len(), 2);

        let file2_blocks = table.get_file_blocks(file_id2);
        assert_eq!(file2_blocks.len(), 1);
        assert_eq!(file2_blocks[0].position.file_id, file_id2);
        assert_eq!(file2_blocks[0].position.block_number, 5);
    }

    #[test]
    fn test_builder() {
        let mut builder = GlobalBlockTableBuilder::new(2048);
        let file_id = FileId::new([4; 16]);
        let checksums = vec![
            (Md5Hash::new([5; 16]), Crc32Value::new(111)),
            (Md5Hash::new([6; 16]), Crc32Value::new(222)),
            (Md5Hash::new([7; 16]), Crc32Value::new(333)),
        ];

        builder.add_file_blocks(file_id, &checksums);
        let table = builder.build();

        assert_eq!(table.stats().total_blocks, 3);
        assert_eq!(table.stats().unique_checksums, 3);
        assert_eq!(table.stats().file_count, 1);

        let file_blocks = table.get_file_blocks(file_id);
        assert_eq!(file_blocks.len(), 3);

        // Check they're sorted by block number
        for (i, block) in file_blocks.iter().enumerate() {
            assert_eq!(block.position.block_number, i as u32);
            assert_eq!(block.position.file_id, file_id);
        }
    }
}
