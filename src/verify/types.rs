//! Type definitions for verification operations

use crate::domain::{Crc32Value, FileId, Md5Hash};
use std::fmt;
use std::path::PathBuf;

/// Position within a file buffer (in bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BufferPosition(usize);

impl BufferPosition {
    #[allow(dead_code)]
    pub fn new(pos: usize) -> Self {
        Self(pos)
    }

    pub fn zero() -> Self {
        Self(0)
    }

    pub fn as_usize(&self) -> usize {
        self.0
    }

    pub fn advance_by(&mut self, bytes: usize) {
        self.0 += bytes;
    }

    pub fn can_fit_block(&self, buffer_size: BufferSize, block_size: BlockSize) -> bool {
        self.0 + block_size.as_usize() <= buffer_size.as_usize()
    }

    /// Get the range for a block starting at this position
    pub fn block_range(&self, block_size: BlockSize) -> std::ops::Range<usize> {
        self.0..self.0 + block_size.as_usize()
    }
}
/// Size of data buffer in bytes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferSize(usize);

impl BufferSize {
    pub fn new(size: usize) -> Self {
        Self(size)
    }

    pub fn as_usize(&self) -> usize {
        self.0
    }

    pub fn has_at_least(&self, block_size: BlockSize) -> bool {
        self.0 >= block_size.as_usize()
    }

    pub fn has_at_least_n_blocks(&self, n: usize, block_size: BlockSize) -> bool {
        self.0 >= n * block_size.as_usize()
    }

    pub fn remainder_from(&self, pos: BufferPosition) -> usize {
        self.0.saturating_sub(pos.as_usize())
    }

    /// Calculate bytes to keep after sliding window by one block
    pub fn bytes_after_slide(&self, block_size: BlockSize) -> usize {
        self.0.saturating_sub(block_size.as_usize())
    }

    /// Get the range to slide buffer contents when moving forward by one block
    pub fn slide_range(&self, block_size: BlockSize) -> std::ops::Range<usize> {
        block_size.as_usize()..self.0
    }

    /// Create new buffer size from kept bytes plus newly read bytes
    pub fn from_slide(bytes_kept: usize, bytes_read: usize) -> Self {
        Self(bytes_kept + bytes_read)
    }
}

/// Size of a PAR2 block in bytes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockSize(usize);

impl BlockSize {
    pub fn new(size: usize) -> Self {
        Self(size)
    }

    pub fn as_usize(&self) -> usize {
        self.0
    }

    pub fn doubled(&self) -> usize {
        self.0 * 2
    }
}

/// Bytes processed through a file (for progress tracking)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BytesProcessed(u64);

impl BytesProcessed {
    pub fn zero() -> Self {
        Self(0)
    }

    pub fn advance_by(&mut self, block_size: BlockSize) {
        self.0 += block_size.as_usize() as u64;
    }

    #[allow(dead_code)]
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    pub fn as_usize(&self) -> usize {
        self.0 as usize
    }

    pub fn progress_fraction(&self, total: u64) -> f64 {
        if total == 0 {
            0.0
        } else {
            self.0 as f64 / total as f64
        }
    }
}
/// Scanning phase - replacing boolean flags with explicit state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanPhase {
    /// First buffer of the file - can try aligned block optimization
    FirstBuffer,
    /// Subsequent buffers - already past file start
    SubsequentBuffer,
}

impl ScanPhase {
    pub fn mark_advanced(&mut self) {
        *self = ScanPhase::SubsequentBuffer;
    }
}

/// File size in bytes
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileSize(u64);

impl FileSize {
    pub fn new(size: u64) -> Self {
        Self(size)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }

    pub fn total_blocks(&self, block_size: BlockSize) -> BlockCount {
        let count = self.0.div_ceil(block_size.as_usize() as u64) as usize;
        BlockCount::new(count)
    }

    #[allow(dead_code)]
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }
}
/// Number of blocks in a file
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlockCount(usize);

impl BlockCount {
    pub fn new(count: usize) -> Self {
        Self(count)
    }

    pub fn zero() -> Self {
        Self(0)
    }

    pub fn as_usize(&self) -> usize {
        self.0
    }

    pub fn increment(&mut self) {
        self.0 += 1;
    }

    /// Check if all blocks are available
    pub fn is_complete(&self, total: BlockCount) -> bool {
        *self == total
    }

    /// Check if no blocks are available
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Iterate over block numbers from 0 to count-1
    pub fn iter_block_numbers(&self) -> impl Iterator<Item = BlockNumber> {
        (0..self.0).map(BlockNumber::new)
    }
}

/// Block number within a file (0-indexed)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockNumber(usize);

impl BlockNumber {
    pub fn new(num: usize) -> Self {
        Self(num)
    }

    #[allow(dead_code)]
    pub fn as_usize(&self) -> usize {
        self.0
    }

    pub fn as_u32(&self) -> u32 {
        self.0 as u32
    }
}
impl From<usize> for BlockNumber {
    fn from(num: usize) -> Self {
        Self(num)
    }
}

/// Unified file verification status used by both verify and repair operations
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FileStatus {
    /// File is perfect match
    Present,
    /// File exists but has wrong name (verify only)
    Renamed,
    /// File exists but is corrupted
    Corrupted,
    /// File is completely missing
    Missing,
}

impl FileStatus {
    /// Returns true if the file needs repair (missing or corrupted)
    pub fn needs_repair(&self) -> bool {
        matches!(self, FileStatus::Missing | FileStatus::Corrupted)
    }
}

impl fmt::Display for FileStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileStatus::Present => write!(f, "present"),
            FileStatus::Renamed => write!(f, "renamed"),
            FileStatus::Corrupted => write!(f, "corrupted"),
            FileStatus::Missing => write!(f, "missing"),
        }
    }
}

/// Block verification result
#[derive(Debug, Clone)]
pub struct BlockVerificationResult {
    pub block_number: u32,
    pub file_id: FileId,
    pub is_valid: bool,
    pub expected_hash: Option<Md5Hash>,
    pub expected_crc: Option<Crc32Value>,
}

/// Comprehensive verification results
#[derive(Debug, Clone)]
pub struct VerificationResults {
    pub files: Vec<FileVerificationResult>,
    pub blocks: Vec<BlockVerificationResult>,
    pub present_file_count: usize,
    pub renamed_file_count: usize,
    pub corrupted_file_count: usize,
    pub missing_file_count: usize,
    pub available_block_count: usize,
    pub missing_block_count: usize,
    pub total_block_count: usize,
    pub recovery_blocks_available: usize,
    pub repair_possible: bool,
    pub blocks_needed_for_repair: usize,
}

impl VerificationResults {
    /// Create verification results by aggregating file and block results
    /// Reference: par2cmdline-turbo/src/par2repairer.cpp:1853-1863 (post-scan validation)
    pub fn from_file_results(
        file_results: Vec<FileVerificationResult>,
        block_results: Vec<BlockVerificationResult>,
        recovery_blocks_available: usize,
    ) -> Self {
        let mut present_count = 0;
        let mut renamed_count = 0;
        let mut corrupted_count = 0;
        let mut missing_count = 0;
        let mut available_blocks = 0;
        let mut missing_blocks = 0;
        let mut total_blocks = 0;

        for file_result in &file_results {
            total_blocks += file_result.total_blocks;
            available_blocks += file_result.blocks_available;
            missing_blocks += file_result.damaged_blocks.len();

            match file_result.status {
                FileStatus::Present => present_count += 1,
                FileStatus::Renamed => renamed_count += 1,
                FileStatus::Corrupted => corrupted_count += 1,
                FileStatus::Missing => missing_count += 1,
            }
        }

        Self {
            files: file_results,
            blocks: block_results,
            present_file_count: present_count,
            renamed_file_count: renamed_count,
            corrupted_file_count: corrupted_count,
            missing_file_count: missing_count,
            available_block_count: available_blocks,
            missing_block_count: missing_blocks,
            total_block_count: total_blocks,
            recovery_blocks_available,
            repair_possible: recovery_blocks_available >= missing_blocks,
            blocks_needed_for_repair: missing_blocks,
        }
    }
}

impl fmt::Display for VerificationResults {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // par2cmdline prints "Scanning extra files:" after verification
        writeln!(f, "Scanning extra files:")?;
        writeln!(f)?;
        writeln!(f)?;

        // Print repair status first if repair is needed
        if self.missing_block_count > 0 {
            writeln!(f, "Repair is required.")?;
        }

        // Functional file status reporting
        [
            (self.present_file_count, "file(s) are ok."),
            (self.renamed_file_count, "file(s) have the wrong name."),
            (self.corrupted_file_count, "file(s) exist but are damaged."),
            (self.missing_file_count, "file(s) are missing."),
        ]
        .iter()
        .filter(|(count, _)| *count > 0)
        .try_for_each(|(count, message)| writeln!(f, "{} {}", count, message))?;

        writeln!(
            f,
            "You have {} out of {} data blocks available.",
            self.available_block_count, self.total_block_count
        )?;

        // Recovery blocks message (functional approach)
        if self.recovery_blocks_available > 0 {
            writeln!(
                f,
                "You have {} recovery blocks available.",
                self.recovery_blocks_available
            )?;
        }

        // Repair status using functional pattern matching
        match (self.missing_block_count, self.repair_possible) {
            (0, _) => writeln!(f, "All files are correct, repair is not required.")?,
            (missing, true) => {
                writeln!(f, "Repair is possible.")?;
                if self.recovery_blocks_available > missing {
                    writeln!(
                        f,
                        "You have an excess of {} recovery blocks.",
                        self.recovery_blocks_available - missing
                    )?;
                }
                writeln!(f, "{} recovery blocks will be used to repair.", missing)?;
            }
            (missing, false) => {
                writeln!(f, "Repair is not possible.")?;
                writeln!(
                    f,
                    "You need {} more recovery blocks to be able to repair.",
                    missing - self.recovery_blocks_available
                )?;
            }
        }

        Ok(())
    }
}

/// Individual file verification result  
#[derive(Debug, Clone)]
pub struct FileVerificationResult {
    pub file_name: String,
    pub file_id: FileId,
    pub status: FileStatus,
    pub blocks_available: usize,
    pub total_blocks: usize,
    pub damaged_blocks: Vec<u32>,
    /// Positions where blocks were found during scanning
    /// Maps block_number -> file_offset where that block was found
    pub block_positions: rustc_hash::FxHashMap<u32, usize>,
    /// Wrong-name path that exactly matched this protected file.
    ///
    /// This is populated only for exact extra-file rename matches where
    /// `status == FileStatus::Renamed`.
    pub matched_path: Option<PathBuf>,
}

/// Buffer for scanning file data
pub struct ScanBuffer(Vec<u8>);

impl ScanBuffer {
    /// Create a new scan buffer with the given capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self(vec![0u8; capacity])
    }

    /// Get the underlying buffer as a slice
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Get a block at the given position
    pub fn block_at(&self, pos: BufferPosition, block_size: BlockSize) -> &[u8] {
        let range = pos.block_range(block_size);
        &self.0[range]
    }

    /// Get the first block from the buffer
    /// Get a slice from position to the end
    pub fn slice_from(&self, pos: BufferPosition, size: BufferSize) -> &[u8] {
        &self.0[pos.as_usize()..size.as_usize()]
    }

    /// Slide buffer window forward by one block
    pub fn slide_window(&mut self, size: BufferSize, block_size: BlockSize) {
        let range = size.slide_range(block_size);
        self.0.copy_within(range, 0);
    }

    /// Read data from a reader into the buffer
    pub fn read_from<R: std::io::Read>(&mut self, reader: &mut R) -> std::io::Result<usize> {
        reader.read(&mut self.0)
    }

    /// Read data from a reader into a specific slice of the buffer
    pub fn read_into_slice<R: std::io::Read>(
        &mut self,
        reader: &mut R,
        start: usize,
    ) -> std::io::Result<usize> {
        reader.read(&mut self.0[start..])
    }

    /// Slide window and read more data from a reader
    /// Returns number of bytes read, or error
    pub fn slide_and_read<R: std::io::Read>(
        &mut self,
        reader: &mut R,
        bytes_in_buffer: BufferSize,
        block_size: BlockSize,
    ) -> std::io::Result<usize> {
        let bytes_to_keep = bytes_in_buffer.bytes_after_slide(block_size);

        // Slide buffer contents
        self.slide_window(bytes_in_buffer, block_size);

        // Read more data
        self.read_into_slice(reader, bytes_to_keep)
    }

    /// Fill buffer with a value (test-only utility)
    #[cfg(test)]
    pub fn fill(&mut self, value: u8) {
        self.0.fill(value);
    }

    /// Get mutable iterator over buffer contents (test-only utility)
    #[cfg(test)]
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, u8> {
        self.0.iter_mut()
    }

    /// Get a slice at a specific range (test-only for CRC computation)
    #[cfg(test)]
    pub fn slice(&self, range: std::ops::Range<usize>) -> &[u8] {
        &self.0[range]
    }

    /// Get a mutable slice at a specific range (test-only for fixture setup)
    #[cfg(test)]
    pub fn slice_mut(&mut self, range: std::ops::Range<usize>) -> &mut [u8] {
        &mut self.0[range]
    }
}

/// Metadata collected during file scanning
#[derive(Debug, Clone, Default)]
pub struct FileScanMetadata {
    /// Whether the first block found was at offset 0
    pub first_block_at_offset_zero: bool,
    /// Whether all blocks were found in sequence
    pub blocks_in_sequence: bool,
    /// Actual MD5 hash of the scanned file
    pub actual_file_hash: Option<Md5Hash>,
    /// Actual MD5 hash of the scanned file's first 16 KiB
    pub actual_file_hash_16k: Option<Md5Hash>,
    /// Actual scanned file size
    pub actual_file_size: Option<u64>,
    /// Blocks found during scan with their file offsets
    pub found_blocks: Vec<(usize, FileId, u32)>, // (file_offset, file_id, block_number)
}

impl FileScanMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_perfect_match(&self) -> bool {
        self.first_block_at_offset_zero && self.blocks_in_sequence
    }

    /// Record that a block was found at a specific file offset
    pub fn record_block_found(&mut self, file_offset: usize, file_id: FileId, block_number: u32) {
        self.found_blocks.push((file_offset, file_id, block_number));
    }

    /// Analyze found blocks to determine if they're at offset 0 and in sequence
    /// Should be called after scanning is complete
    pub fn analyze_block_positions(&mut self, target_file_id: FileId) {
        // Filter and collect blocks for the target file, sorted by offset
        let target_blocks: Vec<_> = self
            .found_blocks
            .iter()
            .filter(|(_, fid, _)| *fid == target_file_id)
            .map(|(offset, _, block_num)| (*offset, *block_num))
            .collect();

        log::debug!(
            "analyze_block_positions: file_id={:?}, found {} blocks, first 5: {:?}",
            target_file_id,
            target_blocks.len(),
            &target_blocks[..target_blocks.len().min(5)]
        );

        // Delegate to pure function for easier testing
        let (first_at_zero, in_sequence) = Self::analyze_sorted_blocks(&target_blocks);
        self.first_block_at_offset_zero = first_at_zero;
        self.blocks_in_sequence = in_sequence;
    }

    /// Pure function: Analyze a sequence of (offset, block_number) pairs
    /// Returns (first_block_at_offset_zero, blocks_in_sequence)
    fn analyze_sorted_blocks(blocks: &[(usize, u32)]) -> (bool, bool) {
        if blocks.is_empty() {
            // Empty files are considered perfectly aligned
            return (true, true);
        }

        // Deduplicate using FxHashSet (sliding window scanning can find the same block multiple times)
        // Then sort by offset to get physical order
        use rustc_hash::FxHashSet;
        let unique: FxHashSet<_> = blocks.iter().copied().collect();
        let mut sorted: Vec<_> = unique.into_iter().collect();
        sorted.sort_by_key(|(offset, _)| *offset);

        // Check if BLOCK 0 is at offset 0
        let first_at_zero = sorted[0] == (0, 0);

        // Check if blocks are in perfect sequence: 0, 1, 2, 3, ...
        let in_sequence = sorted
            .iter()
            .enumerate()
            .all(|(expected_num, &(_, actual_num))| expected_num as u32 == actual_num);

        (first_at_zero, in_sequence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_sorted_blocks_empty() {
        let blocks = [];
        let (first_at_zero, in_sequence) = FileScanMetadata::analyze_sorted_blocks(&blocks);
        assert!(first_at_zero);
        assert!(in_sequence);
    }

    #[test]
    fn test_analyze_sorted_blocks_perfect_sequence() {
        let blocks = [(0, 0), (1024, 1), (2048, 2)];
        let (first_at_zero, in_sequence) = FileScanMetadata::analyze_sorted_blocks(&blocks);
        assert!(first_at_zero);
        assert!(in_sequence);
    }

    #[test]
    fn test_analyze_sorted_blocks_unsorted_input() {
        // Function should sort by offset
        let blocks = [(2048, 2), (0, 0), (1024, 1)];
        let (first_at_zero, in_sequence) = FileScanMetadata::analyze_sorted_blocks(&blocks);
        assert!(first_at_zero);
        assert!(in_sequence);
    }

    #[test]
    fn test_analyze_sorted_blocks_missing_block() {
        let blocks = [(0, 0), (2048, 2)]; // Missing block 1
        let (first_at_zero, in_sequence) = FileScanMetadata::analyze_sorted_blocks(&blocks);
        assert!(first_at_zero);
        assert!(!in_sequence);
    }

    #[test]
    fn test_analyze_sorted_blocks_wrong_start_offset() {
        let blocks = [(1024, 0), (2048, 1)];
        let (first_at_zero, in_sequence) = FileScanMetadata::analyze_sorted_blocks(&blocks);
        assert!(!first_at_zero);
        assert!(in_sequence); // Blocks ARE in sequence (0, 1), just not at offset 0
    }

    #[test]
    fn test_analyze_sorted_blocks_duplicate() {
        // Duplicates are deduplicated - same block found multiple times is still valid
        let blocks = [(0, 0), (0, 0)];
        let (first_at_zero, in_sequence) = FileScanMetadata::analyze_sorted_blocks(&blocks);
        assert!(first_at_zero);
        assert!(in_sequence); // Deduped to just [(0, 0)]
    }

    #[test]
    fn test_analyze_sorted_blocks_starts_with_non_zero_block() {
        let blocks = [(0, 1), (1024, 2)]; // Starts with block 1
        let (first_at_zero, in_sequence) = FileScanMetadata::analyze_sorted_blocks(&blocks);
        assert!(!first_at_zero);
        assert!(!in_sequence);
    }
}
