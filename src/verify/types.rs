//! Type definitions for verification operations

use crate::domain::{Crc32Value, FileId, Md5Hash};
use std::fmt;

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

    /// Get the byte at the position before this one (for rolling CRC)
    pub fn byte_before(&self, buffer: &[u8]) -> u8 {
        buffer[self.0 - 1]
    }

    /// Get the byte at position + offset (for rolling CRC window)
    pub fn byte_at_offset(&self, buffer: &[u8], offset: usize) -> u8 {
        buffer[self.0 + offset]
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

    pub fn is_empty(&self) -> bool {
        self.0 == 0
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

    /// Get a slice from a position to the end of the buffer
    pub fn slice_from<'a>(&self, pos: BufferPosition, buffer: &'a [u8]) -> &'a [u8] {
        &buffer[pos.as_usize()..self.0]
    }

    /// Get a slice from start for a given block size
    pub fn slice_first_block<'a>(&self, block_size: BlockSize, buffer: &'a [u8]) -> &'a [u8] {
        &buffer[0..block_size.as_usize()]
    }

    /// Try to get an aligned block at the given index (0 or 1)
    /// Returns Some(slice) if the block fits, None otherwise
    pub fn try_aligned_block<'a>(
        &self,
        block_idx: usize,
        block_size: BlockSize,
        buffer: &'a [u8],
    ) -> Option<&'a [u8]> {
        let start = block_idx * block_size.as_usize();
        let end = start + block_size.as_usize();
        if end <= self.0 {
            Some(&buffer[start..end])
        } else {
            None
        }
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
    pub fn is_first_buffer(&self) -> bool {
        matches!(self, ScanPhase::FirstBuffer)
    }

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
#[derive(Debug, Clone, PartialEq, Eq)]
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

impl fmt::Display for VerificationResults {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Verification Results:")?;
        writeln!(f, "====================")?;

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
}
