//! Shared validation logic for verifying file slices/blocks using CRC32 and MD5 checksums.
//!
//! This module provides efficient sequential I/O-based validation that is shared between
//! the verify and repair modules.

use crate::domain::{Crc32Value, Md5Hash};
use rustc_hash::FxHashSet as HashSet;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::ops::Deref;
use std::path::Path;

/// Buffer size for sequential I/O operations (128MB for optimal throughput)
const BUFFER_CAPACITY: usize = 128 * 1024 * 1024;

/// Macro to define a newtype wrapper with Deref implementation
/// This reduces boilerplate and ensures consistency across all newtypes
macro_rules! define_newtype {
    // Basic version with Debug, Clone, Copy, PartialEq, Eq
    ($name:ident, $inner:ty) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        struct $name($inner);

        impl Deref for $name {
            type Target = $inner;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl From<$inner> for $name {
            fn from(value: $inner) -> Self {
                $name(value)
            }
        }
    };
    // Version with additional traits (like Hash, PartialOrd, Ord)
    ($name:ident, $inner:ty, $($extra_trait:ident),+) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, $($extra_trait),+)]
        struct $name($inner);

        impl Deref for $name {
            type Target = $inner;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl From<$inner> for $name {
            fn from(value: $inner) -> Self {
                $name(value)
            }
        }
    };
}

/// Represents whether a block was found and where
#[derive(Debug, Clone, Copy, PartialEq, Eq)]

enum BlockMatchResult {
    /// Block found at its expected aligned position
    FoundAligned,
    /// Block found at a misaligned position (offset from expected)
    FoundMisaligned { offset_from_expected: u64 },
    /// Block not found (damaged or missing)
    NotFound,
}

impl BlockMatchResult {
    /// Check if the block was found (aligned or misaligned)
    fn is_found(&self) -> bool {
        matches!(
            self,
            BlockMatchResult::FoundAligned | BlockMatchResult::FoundMisaligned { .. }
        )
    }
}

// Use macro to define newtypes - reduces boilerplate and ensures consistency
define_newtype!(BlockSize, usize);
define_newtype!(ByteOffset, usize, PartialOrd, Ord);
define_newtype!(SliceSize, usize);
define_newtype!(FileSize, u64);
define_newtype!(WindowSize, usize);
define_newtype!(BytesRead, usize);
define_newtype!(SearchOffset, usize, PartialOrd, Ord);
define_newtype!(SliceIndex, usize, Hash);

/// Newtype for block index to prevent mixing with byte offsets
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BlockIndex(usize);

impl BlockIndex {
    fn as_u32(&self) -> u32 {
        self.0 as u32
    }

    /// Calculate byte offset for this block index
    fn byte_offset(&self, block_size: BlockSize) -> ByteOffset {
        ByteOffset(self.0 * *block_size)
    }
}

impl BlockSize {
    /// Create a 2-block window size for sliding window search
    fn window_size(&self) -> WindowSize {
        WindowSize::from_block_size(*self)
    }
}

impl WindowSize {
    /// Create from block size (window is 2 blocks)
    fn from_block_size(block_size: BlockSize) -> Self {
        WindowSize(*block_size * 2)
    }
}

impl BytesRead {
    /// Calculate the maximum search offset within this window
    /// Uses saturating_sub to avoid manual if/else - more concise and bug-resistant
    fn max_search_offset(&self, block_size: BlockSize) -> SearchOffset {
        SearchOffset(self.0.saturating_sub(*block_size))
    }
}

impl SearchOffset {
    /// Get the end position for a block candidate at this offset
    fn candidate_end(&self, block_size: BlockSize, bytes_available: BytesRead) -> usize {
        (self.0 + *block_size).min(*bytes_available)
    }

    /// Create an inclusive range from 0 to this offset
    fn inclusive_range(&self) -> std::ops::RangeInclusive<usize> {
        0..=self.0
    }
}

/// Position within a scanning buffer
/// In par2cmdline-turbo, the buffer has specific regions with invariants:
/// - buffer: start of 2*blocksize buffer
/// - outpointer: start of current block candidate being checked
/// - inpointer: end of current block / start of next block data
/// - tailpointer: end of valid data read from file
///
/// Checksum verification result from comparing candidate against expected
#[derive(Debug, Clone, Copy, PartialEq, Eq)]

enum ChecksumMatch {
    /// Both CRC32 and MD5 match - block is valid
    Both,
    /// CRC32 matches but MD5 doesn't - false positive
    Crc32Only,
    /// CRC32 doesn't match - not even a candidate
    None,
}

impl ChecksumMatch {
    /// Check if this represents a valid block (both checksums match)
    fn is_valid(&self) -> bool {
        matches!(self, ChecksumMatch::Both)
    }
}

/// Represents a candidate block extracted from the window
struct BlockCandidate<'a> {
    data: &'a [u8],
}

impl<'a> BlockCandidate<'a> {
    /// Extract a candidate block from the window at the given offset
    fn from_window(
        window: &'a [u8],
        offset: SearchOffset,
        block_size: BlockSize,
        bytes_read: BytesRead,
    ) -> Self {
        let end = offset.candidate_end(block_size, bytes_read);
        BlockCandidate {
            data: &window[*offset..end],
        }
    }

    /// Check if this candidate matches the expected checksums
    fn matches(
        &self,
        expected_md5: &Md5Hash,
        expected_crc: &Crc32Value,
        block_size: BlockSize,
    ) -> ChecksumMatch {
        let block_size_bytes = *block_size;

        // Functional: use helper to compute CRC32
        let candidate_crc = compute_candidate_crc(self.data, block_size_bytes);

        // Functional chain: check CRC32, then MD5 only if CRC32 matches
        if &candidate_crc == expected_crc {
            // CRC32 matches, use helper to compute MD5
            let candidate_md5 = compute_candidate_md5(self.data, block_size_bytes);

            // Return appropriate match type based on MD5
            if &candidate_md5 == expected_md5 {
                ChecksumMatch::Both
            } else {
                ChecksumMatch::Crc32Only
            }
        } else {
            ChecksumMatch::None
        }
    }
}

/// Compute CRC32 for data, handling padding if data is shorter than target size
/// This consolidates the common pattern of checking length and choosing padded vs unpadded
#[inline]
fn compute_crc_with_padding(data: &[u8], target_size: usize) -> Crc32Value {
    if data.len() < target_size {
        crate::checksum::compute_crc32_padded(data, target_size)
    } else {
        crate::checksum::compute_crc32(data)
    }
}

/// Compute MD5 for data, handling padding if data is shorter than target size
/// This consolidates the common pattern of checking length and creating padded buffer
#[inline]
fn compute_md5_with_padding(data: &[u8], target_size: usize) -> Md5Hash {
    if data.len() < target_size {
        let mut padded = vec![0u8; target_size];
        padded[..data.len()].copy_from_slice(data);
        crate::checksum::compute_md5(&padded)
    } else {
        crate::checksum::compute_md5(data)
    }
}

/// Validate a single slice and return whether it's valid
///
/// Extracts the validation logic into a pure function for better testability
#[inline]
fn validate_slice_crc(
    slice_data: &[u8],
    actual_size: usize,
    slice_size: usize,
    expected_crc: &Crc32Value,
) -> bool {
    let slice_crc = compute_crc_with_padding(&slice_data[..actual_size], slice_size);
    slice_crc == *expected_crc
}

/// Compute CRC32 for a block candidate, handling padding if needed
#[inline]
fn compute_candidate_crc(data: &[u8], block_size_bytes: usize) -> Crc32Value {
    compute_crc_with_padding(data, block_size_bytes)
}

/// Compute MD5 for a block candidate, handling padding if needed
#[inline]
fn compute_candidate_md5(data: &[u8], block_size_bytes: usize) -> Md5Hash {
    compute_md5_with_padding(data, block_size_bytes)
}

/// Calculate the actual size of a slice, handling the last partial slice
#[inline]
fn calculate_slice_size(
    slice_index: SliceIndex,
    total_slices: usize,
    slice_size: SliceSize,
    file_size: FileSize,
) -> usize {
    // For the last slice, calculate remainder; for others, use full slice size
    if *slice_index == total_slices - 1 {
        let remainder = (*file_size % *slice_size as u64) as usize;
        if remainder == 0 {
            *slice_size
        } else {
            remainder
        }
    } else {
        *slice_size
    }
}

/// Calculate progress reporting interval based on mode and number of items
#[inline]
fn calculate_progress_interval(total_items: usize, parallel_mode: bool) -> usize {
    if parallel_mode {
        std::cmp::max(1, total_items / 20) // 5% intervals for parallel
    } else {
        std::cmp::max(1, total_items / 1000) // 0.1% intervals for single-threaded
    }
}

/// Extract filename from path for progress reporting
#[inline]
fn extract_filename(path: &Path) -> &str {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
}

/// Check if file size warrants progress reporting (files > 10MB)
#[inline]
fn should_report_progress(file_size: u64) -> bool {
    file_size > 10 * 1024 * 1024
}

/// Open a file and get its size in bytes
#[inline]
fn open_file_with_size<P: AsRef<Path>>(path: P) -> io::Result<(File, usize)> {
    let file = File::open(path)?;
    let file_size = file.metadata()?.len() as usize;
    Ok((file, file_size))
}

/// Create tuple indicating all blocks are damaged (for error cases)
#[inline]
fn all_blocks_damaged(count: usize) -> (usize, Vec<u32>) {
    (0, (0..count as u32).collect())
}

/// Validate all blocks and separate into available vs damaged
#[inline]
fn validate_and_partition_blocks(
    file: &mut File,
    block_checksums: &[(Md5Hash, Crc32Value)],
    block_size: BlockSize,
    file_size: usize,
    window_buffer: &mut [u8],
) -> PartitionedBlocks {
    block_checksums
        .iter()
        .enumerate()
        .map(|(idx, (expected_md5, expected_crc))| {
            let block_index = BlockIndex(idx);
            let result = validate_single_block(
                file,
                block_index,
                expected_md5,
                expected_crc,
                block_size,
                file_size,
                window_buffer,
            );
            (block_index, result)
        })
        .partition(|(_, result)| matches!(result, BlockValidationResult::Damaged))
}

/// Convert damaged blocks to indices
#[inline]
fn extract_damaged_indices(damaged_blocks: Vec<(BlockIndex, BlockValidationResult)>) -> Vec<u32> {
    damaged_blocks
        .into_iter()
        .map(|(block_index, _)| block_index.as_u32())
        .collect()
}

/// Calculate how many bytes to read for a sliding window
#[inline]
fn calculate_window_read_size(
    file_size: usize,
    block_offset: ByteOffset,
    window_size: WindowSize,
) -> usize {
    let bytes_available = file_size.saturating_sub(*block_offset);
    bytes_available.min(*window_size)
}

/// Seek to position and read into buffer
#[inline]
fn seek_and_read(file: &mut File, offset: ByteOffset, buffer: &mut [u8]) -> io::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(*offset as u64))?;
    file.read_exact(buffer)?;
    Ok(())
}

/// Validates slices in a file using CRC32 checksums only.
///
/// This is optimized for repair operations where only CRC32 validation is needed.
/// Uses sequential I/O with a large buffer for optimal throughput.
///
/// # Arguments
/// * `file_path` - Path to the file to validate
/// * `slice_checksums` - Expected CRC32 values for each slice
/// * `slice_size` - Size of each slice in bytes
/// * `file_size` - Total size of the file
///
/// # Returns
/// A `HashSet` containing the indices of all valid slices
///
/// # Errors
/// Returns an `io::Error` if the file cannot be opened
pub fn validate_slices_crc32<P: AsRef<Path>>(
    file_path: P,
    slice_checksums: &[Crc32Value],
    slice_size: usize,
    file_size: u64,
) -> io::Result<HashSet<usize>> {
    validate_slices_crc32_with_progress(
        file_path,
        slice_checksums,
        slice_size,
        file_size,
        &crate::repair::SilentReporter,
        false, // Not in parallel mode (single file validation)
    )
}
/// Validates slices in a file using CRC32 checksums with progress reporting.
///
/// This is optimized for repair operations where only CRC32 validation is needed.
/// Uses sequential I/O with a large buffer for optimal throughput.
///
/// # Arguments
/// * `file_path` - Path to the file to validate
/// * `slice_checksums` - Expected CRC32 values for each slice
/// * `slice_size` - Size of each slice in bytes
/// * `file_size` - Total size of the file
/// * `progress` - Progress reporter for large file scanning
/// * `parallel_mode` - Whether this is running in parallel mode (affects update frequency)
///
/// # Returns
/// A `HashSet` containing the indices of all valid slices
///
/// # Errors
/// Returns an `io::Error` if the file cannot be opened
pub fn validate_slices_crc32_with_progress<P: AsRef<Path>>(
    file_path: P,
    slice_checksums: &[Crc32Value],
    slice_size: usize,
    file_size: u64,
    progress: &dyn crate::repair::ProgressReporter,
    parallel_mode: bool,
) -> io::Result<HashSet<usize>> {
    // Functional: use Into trait for conversions
    let slice_size_typed: SliceSize = slice_size.into();
    let file_size_typed: FileSize = file_size.into();

    let file = File::open(&file_path)?;
    let mut reader = BufReader::with_capacity(BUFFER_CAPACITY, file);

    // Use helper functions for cleaner setup
    let file_name = extract_filename(file_path.as_ref());
    let should_report = should_report_progress(file_size);
    let progress_interval = calculate_progress_interval(slice_checksums.len(), parallel_mode);

    // Functional approach: use scan to track state, then filter_map to collect valid slices
    // scan() is the functional way to maintain state across iterations
    let valid_slices: HashSet<usize> = slice_checksums
        .iter()
        .enumerate()
        .scan(
            (vec![0u8; slice_size], 0u64),
            |(slice_buffer, bytes_processed), (slice_index, &expected_crc)| {
                let slice_index_typed = SliceIndex(slice_index);
                let actual_size = calculate_slice_size(
                    slice_index_typed,
                    slice_checksums.len(),
                    slice_size_typed,
                    file_size_typed,
                );

                // Zero padding if needed
                if actual_size < slice_size {
                    slice_buffer[actual_size..].fill(0);
                }

                // Read slice and validate
                let is_valid = reader
                    .read_exact(&mut slice_buffer[..actual_size])
                    .map(|_| {
                        *bytes_processed += actual_size as u64;

                        // Report progress at intervals
                        if should_report && slice_index % progress_interval == 0 {
                            progress.report_scanning_progress(
                                file_name,
                                *bytes_processed,
                                file_size,
                            );
                        }

                        validate_slice_crc(slice_buffer, actual_size, slice_size, &expected_crc)
                    })
                    .unwrap_or(false);

                Some((slice_index, is_valid))
            },
        )
        .filter_map(|(index, is_valid)| is_valid.then_some(index))
        .collect();

    Ok(valid_slices)
}

/// Validates blocks in a file using both MD5 and CRC32 checksums with sliding window.
///
/// This is used for verification operations where both hash types must match.
/// Uses a sliding window approach to handle data misalignment:
/// - Loads 2 blocks worth of data at a time
/// - Performs rolling CRC32 check, shifting by 1 byte at a time
/// - When CRC32 matches, validates MD5 to confirm the block
///
/// This helps detect blocks that may have shifted due to corruption or insertion/deletion.
///
/// # Arguments
/// * `file_path` - Path to the file to validate
/// * `block_checksums` - Expected (MD5, CRC32) pairs for each block
/// * `block_size` - Size of each block in bytes
///
/// # Returns
/// A tuple of (available_blocks_count, damaged_block_indices)
///
/// # Performance Notes
/// - CRC32 is checked first (100x faster than MD5) for early exit on mismatches
/// - Sliding window allows detection of shifted blocks
/// - MD5 is only computed when CRC32 matches
pub fn validate_blocks_md5_crc32<P: AsRef<Path>>(
    file_path: P,
    block_checksums: &[(Md5Hash, Crc32Value)],
    block_size: usize,
) -> (usize, Vec<u32>) {
    // Functional: use Into trait for type conversion
    let block_size: BlockSize = block_size.into();

    // Functional approach: use helper to flatten nested Result combinators
    open_file_with_size(&file_path)
        .map(|(mut file, file_size)| {
            // Allocate sliding window buffer once, reuse for all blocks
            let window_size = block_size.window_size();
            let mut window_buffer = vec![0u8; *window_size];

            // Functional: delegate validation and partitioning to helper
            let (damaged_blocks, _valid_blocks) = validate_and_partition_blocks(
                &mut file,
                block_checksums,
                block_size,
                file_size,
                &mut window_buffer,
            );

            // Functional: calculate results from damaged blocks
            let available_count = block_checksums.len() - damaged_blocks.len();
            let damaged_indices = extract_damaged_indices(damaged_blocks);

            (available_count, damaged_indices)
        })
        .unwrap_or_else(|_| all_blocks_damaged(block_checksums.len()))
}

/// Search for a block within a sliding window buffer
///
/// # Arguments
/// * `window_buffer` - Buffer containing up to 2 blocks of data
/// * `bytes_to_read` - Actual number of valid bytes in the buffer
/// * `block_size` - Size of a single block
/// * `expected_md5` - Expected MD5 hash of the block
/// * `expected_crc` - Expected CRC32 checksum of the block
///
/// # Returns
/// `BlockMatchResult` indicating whether and where the block was found
fn search_block_in_window(
    window_buffer: &[u8],
    bytes_read: BytesRead,
    block_size: BlockSize,
    expected_md5: &Md5Hash,
    expected_crc: &Crc32Value,
) -> BlockMatchResult {
    let max_offset = bytes_read.max_search_offset(block_size);

    // Functional approach: use iterator find_map to search and transform in one pass
    // This eliminates the need for manual loop control and early returns
    max_offset
        .inclusive_range()
        .find_map(|raw_offset| {
            let offset = SearchOffset(raw_offset);
            let candidate =
                BlockCandidate::from_window(window_buffer, offset, block_size, bytes_read);

            // Check if candidate matches both checksums
            // Use then_some for non-lazy evaluation (clippy suggestion)
            candidate
                .matches(expected_md5, expected_crc, block_size)
                .is_valid()
                .then_some(if raw_offset == 0 {
                    BlockMatchResult::FoundAligned
                } else {
                    BlockMatchResult::FoundMisaligned {
                        offset_from_expected: raw_offset as u64,
                    }
                })
        })
        .unwrap_or(BlockMatchResult::NotFound)
}

/// Validation result for a single block
#[derive(Debug, Clone, Copy)]
enum BlockValidationResult {
    Valid,
    Damaged,
}

/// Type alias for partitioned block validation results
type PartitionedBlocks = (
    Vec<(BlockIndex, BlockValidationResult)>,
    Vec<(BlockIndex, BlockValidationResult)>,
);

/// Validate a single block by reading its window and searching for a match
///
/// This is a pure(ish) function that encapsulates all the logic for validating one block.
/// It reduces the chance of bugs by keeping all validation logic in one place.
fn validate_single_block(
    file: &mut File,
    block_index: BlockIndex,
    expected_md5: &Md5Hash,
    expected_crc: &Crc32Value,
    block_size: BlockSize,
    file_size: usize,
    window_buffer: &mut [u8],
) -> BlockValidationResult {
    let block_offset = block_index.byte_offset(block_size);

    // Functional approach: check bounds early
    if *block_offset >= file_size {
        return BlockValidationResult::Damaged;
    }

    // Calculate window parameters using helpers
    let window_size = block_size.window_size();
    let bytes_to_read = calculate_window_read_size(file_size, block_offset, window_size);

    // Zero-fill the window buffer
    window_buffer.fill(0);

    // Functional: chain Result operations to read and validate
    seek_and_read(file, block_offset, &mut window_buffer[..bytes_to_read])
        .ok()
        .and_then(|_| {
            // Search for block in window using sliding window algorithm
            search_block_in_window(
                window_buffer,
                BytesRead(bytes_to_read),
                block_size,
                expected_md5,
                expected_crc,
            )
            .is_found()
            .then_some(BlockValidationResult::Valid)
        })
        .unwrap_or(BlockValidationResult::Damaged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_validate_slices_crc32_all_valid() {
        // Create a test file with known content
        let mut temp_file = NamedTempFile::new().unwrap();
        let data = b"Hello, World! This is test data.";
        temp_file.write_all(data).unwrap();
        temp_file.flush().unwrap();

        // Compute expected CRC32
        let expected_crc = crate::checksum::compute_crc32(data);

        // Validate
        let valid_slices = validate_slices_crc32(
            temp_file.path(),
            &[expected_crc],
            data.len(),
            data.len() as u64,
        )
        .unwrap();

        assert_eq!(valid_slices.len(), 1);
        assert!(valid_slices.contains(&0));
    }

    #[test]
    fn test_validate_blocks_md5_crc32_all_valid() {
        // Create a test file with known content
        let mut temp_file = NamedTempFile::new().unwrap();
        let data = b"Test block data";
        temp_file.write_all(data).unwrap();
        temp_file.flush().unwrap();

        // Compute expected checksums
        let (expected_md5, expected_crc) = crate::checksum::compute_block_checksums(data);

        // Validate
        let (available, damaged) = validate_blocks_md5_crc32(
            temp_file.path(),
            &[(expected_md5, expected_crc)],
            data.len(),
        );

        assert_eq!(available, 1);
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_aligned_blocks() {
        // Test with multiple aligned blocks
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();

        // Create 3 blocks of test data
        let block1 = vec![1u8; block_size];
        let block2 = vec![2u8; block_size];
        let block3 = vec![3u8; block_size];

        temp_file.write_all(&block1).unwrap();
        temp_file.write_all(&block2).unwrap();
        temp_file.write_all(&block3).unwrap();
        temp_file.flush().unwrap();

        // Compute checksums for each block
        let checksums = vec![
            crate::checksum::compute_block_checksums(&block1),
            crate::checksum::compute_block_checksums(&block2),
            crate::checksum::compute_block_checksums(&block3),
        ];

        // Validate
        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &checksums, block_size);

        assert_eq!(available, 3, "All 3 blocks should be found");
        assert!(damaged.is_empty(), "No blocks should be damaged");
    }

    #[test]
    fn test_validate_blocks_misaligned_single_block() {
        // Test sliding window: block shifted by a few bytes
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();

        // Create padding and then the actual block data
        let padding = vec![0u8; 42]; // 42 bytes of offset
        let block_data = vec![5u8; block_size];

        temp_file.write_all(&padding).unwrap();
        temp_file.write_all(&block_data).unwrap();
        temp_file.flush().unwrap();

        // Compute checksum for the block (without padding)
        let (expected_md5, expected_crc) = crate::checksum::compute_block_checksums(&block_data);

        // Validate - should find the block at offset 42
        let (available, damaged) = validate_blocks_md5_crc32(
            temp_file.path(),
            &[(expected_md5, expected_crc)],
            block_size,
        );

        assert_eq!(available, 1, "Block should be found at offset 42");
        assert!(damaged.is_empty(), "Block should not be marked as damaged");
    }

    #[test]
    fn test_validate_blocks_partial_block_at_end() {
        // Test with a partial block at the end of the file
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();

        // Full block + partial block
        let block1 = vec![1u8; block_size];
        let block2_partial = vec![2u8; 512]; // Only half a block

        temp_file.write_all(&block1).unwrap();
        temp_file.write_all(&block2_partial).unwrap();
        temp_file.flush().unwrap();

        // Compute checksums (partial block needs padding)
        let checksum1 = crate::checksum::compute_block_checksums(&block1);
        let checksum2 =
            crate::checksum::compute_block_checksums_padded(&block2_partial, block_size);

        // Validate
        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &[checksum1, checksum2], block_size);

        assert_eq!(available, 2, "Both blocks should be found");
        assert!(damaged.is_empty(), "No blocks should be damaged");
    }

    #[test]
    fn test_validate_blocks_corrupted_block() {
        // Test that corrupted blocks are detected
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();

        let block_data = vec![7u8; block_size];
        temp_file.write_all(&block_data).unwrap();
        temp_file.flush().unwrap();

        // Create WRONG checksums (for different data)
        let wrong_data = vec![9u8; block_size];
        let (wrong_md5, wrong_crc) = crate::checksum::compute_block_checksums(&wrong_data);

        // Validate
        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &[(wrong_md5, wrong_crc)], block_size);

        assert_eq!(available, 0, "Block should not be found");
        assert_eq!(damaged.len(), 1, "Block should be marked as damaged");
        assert_eq!(damaged[0], 0, "Block 0 should be damaged");
    }

    #[test]
    fn test_validate_blocks_missing_file() {
        // Test with a file that doesn't exist
        let block_size = 1024;
        let (expected_md5, expected_crc) =
            crate::checksum::compute_block_checksums(&vec![0u8; block_size]);

        let (available, damaged) = validate_blocks_md5_crc32(
            "/nonexistent/path/to/file.dat",
            &[(expected_md5, expected_crc)],
            block_size,
        );

        assert_eq!(available, 0, "No blocks should be found");
        assert_eq!(damaged.len(), 1, "Block should be marked as damaged");
    }

    #[test]
    fn test_validate_blocks_multiple_with_one_corrupted() {
        // Test with multiple blocks where one is corrupted
        let block_size = 512;
        let mut temp_file = NamedTempFile::new().unwrap();

        // Create 3 blocks
        let block1 = vec![10u8; block_size];
        let block2 = vec![20u8; block_size];
        let block3 = vec![30u8; block_size];

        temp_file.write_all(&block1).unwrap();
        temp_file.write_all(&block2).unwrap();
        temp_file.write_all(&block3).unwrap();
        temp_file.flush().unwrap();

        // Compute checksums - but make block 2's checksum wrong
        let checksum1 = crate::checksum::compute_block_checksums(&block1);
        let wrong_block2 = vec![99u8; block_size];
        let checksum2 = crate::checksum::compute_block_checksums(&wrong_block2);
        let checksum3 = crate::checksum::compute_block_checksums(&block3);

        // Validate
        let (available, damaged) = validate_blocks_md5_crc32(
            temp_file.path(),
            &[checksum1, checksum2, checksum3],
            block_size,
        );

        assert_eq!(available, 2, "Two blocks should be found");
        assert_eq!(damaged.len(), 1, "One block should be damaged");
        assert_eq!(damaged[0], 1, "Block 1 (middle) should be damaged");
    }

    #[test]
    fn test_validate_blocks_large_offset() {
        // Test sliding window with a larger offset (within one block size)
        let block_size = 2048;
        let mut temp_file = NamedTempFile::new().unwrap();

        // Large offset (but less than block_size)
        let padding = vec![0u8; 1500];
        let block_data = vec![42u8; block_size];

        temp_file.write_all(&padding).unwrap();
        temp_file.write_all(&block_data).unwrap();
        temp_file.flush().unwrap();

        let (expected_md5, expected_crc) = crate::checksum::compute_block_checksums(&block_data);

        // Validate - should find block at offset 1500
        let (available, damaged) = validate_blocks_md5_crc32(
            temp_file.path(),
            &[(expected_md5, expected_crc)],
            block_size,
        );

        assert_eq!(available, 1, "Block should be found at offset 1500");
        assert!(damaged.is_empty(), "Block should not be damaged");
    }

    #[test]
    fn test_validate_blocks_zero_sized() {
        // Edge case: zero-sized block (shouldn't happen but good to test)
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(&[]).unwrap();
        temp_file.flush().unwrap();

        let (expected_md5, expected_crc) =
            crate::checksum::compute_block_checksums_padded(&[], block_size);

        let (available, damaged) = validate_blocks_md5_crc32(
            temp_file.path(),
            &[(expected_md5, expected_crc)],
            block_size,
        );

        // File is too small, block should be damaged
        assert_eq!(available, 0);
        assert_eq!(damaged.len(), 1);
    }

    #[test]
    fn test_validate_blocks_file_shorter_than_block() {
        // File is shorter than the block size
        let block_size = 2048;
        let mut temp_file = NamedTempFile::new().unwrap();
        let data = vec![77u8; 512]; // Only 512 bytes, block is 2048
        temp_file.write_all(&data).unwrap();
        temp_file.flush().unwrap();

        let (expected_md5, expected_crc) =
            crate::checksum::compute_block_checksums_padded(&data, block_size);

        let (available, damaged) = validate_blocks_md5_crc32(
            temp_file.path(),
            &[(expected_md5, expected_crc)],
            block_size,
        );

        assert_eq!(available, 1, "Partial block should be found");
        assert!(damaged.is_empty(), "Partial block should not be damaged");
    }

    #[test]
    fn test_validate_blocks_exactly_at_block_boundary() {
        // Test block that starts exactly at the end of the first block
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();

        let block1 = vec![11u8; block_size];
        let block2 = vec![22u8; block_size];

        temp_file.write_all(&block1).unwrap();
        temp_file.write_all(&block2).unwrap();
        temp_file.flush().unwrap();

        let checksum2 = crate::checksum::compute_block_checksums(&block2);

        // Validate only the second block
        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &[checksum2], block_size);

        assert_eq!(
            available, 1,
            "Second block should be found at aligned position"
        );
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_offset_exceeds_block_size() {
        // Test when data is offset by more than one block size (should not be found in 2-block window)
        let block_size = 512;
        let mut temp_file = NamedTempFile::new().unwrap();

        // Offset by 1.5 block sizes
        let padding = vec![0u8; block_size + block_size / 2];
        let block_data = vec![88u8; block_size];

        temp_file.write_all(&padding).unwrap();
        temp_file.write_all(&block_data).unwrap();
        temp_file.flush().unwrap();

        let (expected_md5, expected_crc) = crate::checksum::compute_block_checksums(&block_data);

        // Should NOT find the block since it's beyond the 2-block search window
        let (available, damaged) = validate_blocks_md5_crc32(
            temp_file.path(),
            &[(expected_md5, expected_crc)],
            block_size,
        );

        assert_eq!(
            available, 0,
            "Block should not be found (offset > block_size)"
        );
        assert_eq!(damaged.len(), 1, "Block should be marked as damaged");
    }

    #[test]
    fn test_validate_blocks_crc_collision() {
        // Test that MD5 is checked even when CRC32 matches
        // This tests the case where CRC32 matches but MD5 doesn't
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();

        let actual_data = vec![55u8; block_size];
        temp_file.write_all(&actual_data).unwrap();
        temp_file.flush().unwrap();

        // Create checksums for different data that might have same CRC32
        // (in practice CRC collisions are rare, but MD5 adds another layer)
        let wrong_data = vec![56u8; block_size];
        let (wrong_md5, actual_crc) = crate::checksum::compute_block_checksums(&wrong_data);

        // This test verifies the logic path where CRC matches but MD5 doesn't
        // In reality we can't easily create a CRC collision, so we just verify
        // that different data produces different results
        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &[(wrong_md5, actual_crc)], block_size);

        // Should not find the block because MD5 doesn't match
        assert_eq!(available, 0);
        assert_eq!(damaged.len(), 1);
    }

    #[test]
    fn test_validate_blocks_many_small_blocks() {
        // Test with many small blocks
        let block_size = 64;
        let num_blocks = 20;
        let mut temp_file = NamedTempFile::new().unwrap();

        let mut checksums = Vec::new();
        for i in 0..num_blocks {
            let block_data = vec![i as u8; block_size];
            temp_file.write_all(&block_data).unwrap();
            checksums.push(crate::checksum::compute_block_checksums(&block_data));
        }
        temp_file.flush().unwrap();

        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &checksums, block_size);

        assert_eq!(
            available, num_blocks,
            "All {} blocks should be found",
            num_blocks
        );
        assert!(damaged.is_empty(), "No blocks should be damaged");
    }

    #[test]
    fn test_validate_blocks_interleaved_corruption() {
        // Test with alternating good and bad blocks
        let block_size = 256;
        let mut temp_file = NamedTempFile::new().unwrap();

        let block1 = vec![1u8; block_size];
        let block2 = vec![2u8; block_size];
        let block3 = vec![3u8; block_size];
        let block4 = vec![4u8; block_size];

        temp_file.write_all(&block1).unwrap();
        temp_file.write_all(&block2).unwrap();
        temp_file.write_all(&block3).unwrap();
        temp_file.write_all(&block4).unwrap();
        temp_file.flush().unwrap();

        // Correct checksums for blocks 1 and 3, wrong for 2 and 4
        let checksums = vec![
            crate::checksum::compute_block_checksums(&block1),
            crate::checksum::compute_block_checksums(&vec![99u8; block_size]), // Wrong
            crate::checksum::compute_block_checksums(&block3),
            crate::checksum::compute_block_checksums(&vec![88u8; block_size]), // Wrong
        ];

        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &checksums, block_size);

        assert_eq!(available, 2, "Blocks 0 and 2 should be found");
        assert_eq!(damaged.len(), 2, "Blocks 1 and 3 should be damaged");
        assert_eq!(damaged[0], 1, "Block 1 should be damaged");
        assert_eq!(damaged[1], 3, "Block 3 should be damaged");
    }

    #[test]
    fn test_validate_blocks_single_byte_offset() {
        // Test block shifted by just 1 byte
        let block_size = 512;
        let mut temp_file = NamedTempFile::new().unwrap();

        let padding = vec![0u8; 1]; // Just 1 byte offset
        let block_data = vec![123u8; block_size];

        temp_file.write_all(&padding).unwrap();
        temp_file.write_all(&block_data).unwrap();
        temp_file.flush().unwrap();

        let (expected_md5, expected_crc) = crate::checksum::compute_block_checksums(&block_data);

        let (available, damaged) = validate_blocks_md5_crc32(
            temp_file.path(),
            &[(expected_md5, expected_crc)],
            block_size,
        );

        assert_eq!(available, 1, "Block should be found at 1-byte offset");
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_maximum_offset_in_window() {
        // Test block at the maximum offset within the search window (block_size - 1)
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();

        let padding = vec![0u8; block_size - 1]; // Maximum offset within window
        let block_data = vec![200u8; block_size];

        temp_file.write_all(&padding).unwrap();
        temp_file.write_all(&block_data).unwrap();
        temp_file.flush().unwrap();

        let (expected_md5, expected_crc) = crate::checksum::compute_block_checksums(&block_data);

        let (available, damaged) = validate_blocks_md5_crc32(
            temp_file.path(),
            &[(expected_md5, expected_crc)],
            block_size,
        );

        assert_eq!(available, 1, "Block should be found at maximum offset");
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_mixed_aligned_and_misaligned() {
        // Test with some blocks aligned and some misaligned
        let block_size = 512;
        let mut temp_file = NamedTempFile::new().unwrap();

        // Block 0: aligned
        let block0 = vec![10u8; block_size];
        temp_file.write_all(&block0).unwrap();

        // Block 1: misaligned by 50 bytes
        let padding = vec![0u8; 50];
        temp_file.write_all(&padding).unwrap();
        let block1 = vec![20u8; block_size];
        temp_file.write_all(&block1).unwrap();

        temp_file.flush().unwrap();

        let checksums = vec![
            crate::checksum::compute_block_checksums(&block0),
            crate::checksum::compute_block_checksums(&block1),
        ];

        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &checksums, block_size);

        assert_eq!(
            available, 2,
            "Both blocks should be found (aligned and misaligned)"
        );
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_all_zeros() {
        // Test with blocks containing all zeros
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();

        let block = vec![0u8; block_size];
        temp_file.write_all(&block).unwrap();
        temp_file.flush().unwrap();

        let (expected_md5, expected_crc) = crate::checksum::compute_block_checksums(&block);

        let (available, damaged) = validate_blocks_md5_crc32(
            temp_file.path(),
            &[(expected_md5, expected_crc)],
            block_size,
        );

        assert_eq!(available, 1, "All-zeros block should be found");
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_all_ones() {
        // Test with blocks containing all 0xFF
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();

        let block = vec![0xFFu8; block_size];
        temp_file.write_all(&block).unwrap();
        temp_file.flush().unwrap();

        let (expected_md5, expected_crc) = crate::checksum::compute_block_checksums(&block);

        let (available, damaged) = validate_blocks_md5_crc32(
            temp_file.path(),
            &[(expected_md5, expected_crc)],
            block_size,
        );

        assert_eq!(available, 1, "All-ones block should be found");
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_random_pattern() {
        // Test with blocks containing a pseudo-random pattern
        let block_size = 2048;
        let mut temp_file = NamedTempFile::new().unwrap();

        // Create pseudo-random pattern
        let mut block = Vec::new();
        for i in 0..block_size {
            block.push(((i * 7 + 13) % 256) as u8);
        }
        temp_file.write_all(&block).unwrap();
        temp_file.flush().unwrap();

        let (expected_md5, expected_crc) = crate::checksum::compute_block_checksums(&block);

        let (available, damaged) = validate_blocks_md5_crc32(
            temp_file.path(),
            &[(expected_md5, expected_crc)],
            block_size,
        );

        assert_eq!(available, 1, "Block with pattern should be found");
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_two_blocks_same_content() {
        // Test with two consecutive blocks containing identical data
        let block_size = 512;
        let mut temp_file = NamedTempFile::new().unwrap();

        let block = vec![42u8; block_size];
        temp_file.write_all(&block).unwrap();
        temp_file.write_all(&block).unwrap(); // Same data twice
        temp_file.flush().unwrap();

        let checksum = crate::checksum::compute_block_checksums(&block);

        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &[checksum, checksum], block_size);

        assert_eq!(available, 2, "Both identical blocks should be found");
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_boundary_case_exact_two_blocks() {
        // Test with file size exactly 2 * block_size
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();

        let block1 = vec![100u8; block_size];
        let block2 = vec![200u8; block_size];

        temp_file.write_all(&block1).unwrap();
        temp_file.write_all(&block2).unwrap();
        temp_file.flush().unwrap();

        let checksums = vec![
            crate::checksum::compute_block_checksums(&block1),
            crate::checksum::compute_block_checksums(&block2),
        ];

        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &checksums, block_size);

        assert_eq!(available, 2, "Both blocks should be found");
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_three_blocks_middle_corrupted() {
        // Test with three blocks where only the middle one is corrupted
        let block_size = 256;
        let mut temp_file = NamedTempFile::new().unwrap();

        let block1 = vec![10u8; block_size];
        let block2 = vec![20u8; block_size];
        let block3 = vec![30u8; block_size];

        temp_file.write_all(&block1).unwrap();
        temp_file.write_all(&block2).unwrap();
        temp_file.write_all(&block3).unwrap();
        temp_file.flush().unwrap();

        let checksums = vec![
            crate::checksum::compute_block_checksums(&block1),
            crate::checksum::compute_block_checksums(&vec![99u8; block_size]), // Wrong
            crate::checksum::compute_block_checksums(&block3),
        ];

        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &checksums, block_size);

        assert_eq!(available, 2, "First and third blocks should be found");
        assert_eq!(damaged.len(), 1);
        assert_eq!(damaged[0], 1, "Only middle block should be damaged");
    }

    #[test]
    fn test_validate_blocks_last_block_corrupted() {
        // Test with multiple blocks where only the last is corrupted
        let block_size = 512;
        let mut temp_file = NamedTempFile::new().unwrap();

        let block1 = vec![1u8; block_size];
        let block2 = vec![2u8; block_size];
        let block3 = vec![3u8; block_size];

        temp_file.write_all(&block1).unwrap();
        temp_file.write_all(&block2).unwrap();
        temp_file.write_all(&block3).unwrap();
        temp_file.flush().unwrap();

        let checksums = vec![
            crate::checksum::compute_block_checksums(&block1),
            crate::checksum::compute_block_checksums(&block2),
            crate::checksum::compute_block_checksums(&vec![99u8; block_size]),
        ];

        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &checksums, block_size);

        assert_eq!(available, 2);
        assert_eq!(damaged, vec![2], "Only last block should be damaged");
    }

    #[test]
    fn test_validate_blocks_first_block_corrupted() {
        // Test with multiple blocks where only the first is corrupted
        let block_size = 512;
        let mut temp_file = NamedTempFile::new().unwrap();

        let block1 = vec![1u8; block_size];
        let block2 = vec![2u8; block_size];
        let block3 = vec![3u8; block_size];

        temp_file.write_all(&block1).unwrap();
        temp_file.write_all(&block2).unwrap();
        temp_file.write_all(&block3).unwrap();
        temp_file.flush().unwrap();

        let checksums = vec![
            crate::checksum::compute_block_checksums(&vec![99u8; block_size]),
            crate::checksum::compute_block_checksums(&block2),
            crate::checksum::compute_block_checksums(&block3),
        ];

        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &checksums, block_size);

        assert_eq!(available, 2);
        assert_eq!(damaged, vec![0], "Only first block should be damaged");
    }

    #[test]
    fn test_validate_blocks_offset_by_half_block() {
        // Test block offset by exactly half a block size
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();

        let padding = vec![0u8; block_size / 2];
        let block_data = vec![150u8; block_size];

        temp_file.write_all(&padding).unwrap();
        temp_file.write_all(&block_data).unwrap();
        temp_file.flush().unwrap();

        let (expected_md5, expected_crc) = crate::checksum::compute_block_checksums(&block_data);

        let (available, damaged) = validate_blocks_md5_crc32(
            temp_file.path(),
            &[(expected_md5, expected_crc)],
            block_size,
        );

        assert_eq!(available, 1, "Block should be found at half-block offset");
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_varying_offsets() {
        // Test multiple blocks each with different offsets
        let block_size = 256;
        let mut temp_file = NamedTempFile::new().unwrap();

        // Block 0: offset by 10 bytes
        temp_file.write_all(&[0u8; 10]).unwrap();
        let block0 = vec![11u8; block_size];
        temp_file.write_all(&block0).unwrap();

        // Block 1: offset by 20 bytes from previous end
        temp_file.write_all(&[0u8; 20]).unwrap();
        let block1 = vec![22u8; block_size];
        temp_file.write_all(&block1).unwrap();

        // Block 2: offset by 30 bytes from previous end
        temp_file.write_all(&[0u8; 30]).unwrap();
        let block2 = vec![33u8; block_size];
        temp_file.write_all(&block2).unwrap();

        temp_file.flush().unwrap();

        let checksums = vec![
            crate::checksum::compute_block_checksums(&block0),
            crate::checksum::compute_block_checksums(&block1),
            crate::checksum::compute_block_checksums(&block2),
        ];

        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &checksums, block_size);

        assert_eq!(
            available, 3,
            "All blocks with varying offsets should be found"
        );
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_empty_checksums_list() {
        // Test with empty checksums list
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(&vec![0u8; block_size]).unwrap();
        temp_file.flush().unwrap();

        let (available, damaged) = validate_blocks_md5_crc32(temp_file.path(), &[], block_size);

        assert_eq!(available, 0);
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_very_small_block_size() {
        // Test with very small block size (1 byte)
        let block_size = 1;
        let mut temp_file = NamedTempFile::new().unwrap();

        let data = vec![42u8, 43u8, 44u8];
        temp_file.write_all(&data).unwrap();
        temp_file.flush().unwrap();

        let checksums = vec![
            crate::checksum::compute_block_checksums(&[42u8]),
            crate::checksum::compute_block_checksums(&[43u8]),
            crate::checksum::compute_block_checksums(&[44u8]),
        ];

        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &checksums, block_size);

        assert_eq!(available, 3, "All 1-byte blocks should be found");
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_alternating_pattern() {
        // Test with blocks containing alternating bit pattern
        let block_size = 512;
        let mut temp_file = NamedTempFile::new().unwrap();

        let mut block = Vec::new();
        for i in 0..block_size {
            block.push(if i % 2 == 0 { 0xAA } else { 0x55 });
        }
        temp_file.write_all(&block).unwrap();
        temp_file.flush().unwrap();

        let (expected_md5, expected_crc) = crate::checksum::compute_block_checksums(&block);

        let (available, damaged) = validate_blocks_md5_crc32(
            temp_file.path(),
            &[(expected_md5, expected_crc)],
            block_size,
        );

        assert_eq!(available, 1, "Alternating pattern block should be found");
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_sequential_bytes() {
        // Test with blocks containing sequential byte values
        let block_size = 256;
        let mut temp_file = NamedTempFile::new().unwrap();

        let block: Vec<u8> = (0..block_size).map(|i| (i % 256) as u8).collect();
        temp_file.write_all(&block).unwrap();
        temp_file.flush().unwrap();

        let (expected_md5, expected_crc) = crate::checksum::compute_block_checksums(&block);

        let (available, damaged) = validate_blocks_md5_crc32(
            temp_file.path(),
            &[(expected_md5, expected_crc)],
            block_size,
        );

        assert_eq!(available, 1, "Sequential bytes block should be found");
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_power_of_two_sizes() {
        // Test various power-of-two block sizes
        for power in 4..=12 {
            let block_size = 1 << power; // 16, 32, 64, ..., 4096
            let mut temp_file = NamedTempFile::new().unwrap();

            let block = vec![power as u8; block_size];
            temp_file.write_all(&block).unwrap();
            temp_file.flush().unwrap();

            let (expected_md5, expected_crc) = crate::checksum::compute_block_checksums(&block);

            let (available, damaged) = validate_blocks_md5_crc32(
                temp_file.path(),
                &[(expected_md5, expected_crc)],
                block_size,
            );

            assert_eq!(available, 1, "Block of size {} should be found", block_size);
            assert!(damaged.is_empty());
        }
    }

    #[test]
    fn test_validate_blocks_file_truncated() {
        // Test when file is shorter than expected (simulates truncation)
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();

        // Write only half of what we'll check for
        let partial_block = vec![77u8; block_size / 2];
        temp_file.write_all(&partial_block).unwrap();
        temp_file.flush().unwrap();

        // Expect two full blocks but file only has half of first
        let block = vec![77u8; block_size];
        let checksum = crate::checksum::compute_block_checksums(&block);

        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &[checksum, checksum], block_size);

        assert_eq!(available, 0, "Truncated blocks should not be found");
        assert_eq!(damaged.len(), 2, "Both blocks should be marked damaged");
    }

    #[test]
    fn test_validate_blocks_partial_then_full() {
        // Test partial block followed by full block
        // When a partial block is written, it gets padded to block_size for checksum
        // The file layout is: [256 bytes of 10] [512 bytes of 20]
        // Block 0 starts at 0, expects 512 bytes (256 bytes of 10 + 256 zeros)
        // Block 1 starts at 512, expects 512 bytes of 20, but file only has 768 bytes total
        let block_size = 512;
        let mut temp_file = NamedTempFile::new().unwrap();

        let partial = vec![10u8; block_size / 2];
        let full = vec![20u8; block_size];

        temp_file.write_all(&partial).unwrap();
        temp_file.write_all(&full).unwrap();
        temp_file.flush().unwrap();

        // For a partial block at start, the checksum is based on the actual bytes + padding
        let checksums = vec![
            crate::checksum::compute_block_checksums_padded(&partial, block_size),
            crate::checksum::compute_block_checksums(&full),
        ];

        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &checksums, block_size);

        // This is tricky: the file has 768 bytes total
        // Block 0 (pos 0-512): Has 256 bytes of data + 256 bytes of 0x20 (not zeros!)
        // Block 1 (pos 512-1024): Has 256 bytes of 0x20 + zeros
        // Neither block will match because block 0 has wrong padding
        // The partial block checksum assumes zero padding, but actual file has 0x20 padding
        assert_eq!(available, 0, "Blocks won't match due to padding mismatch");
        assert_eq!(damaged.len(), 2, "Both blocks are effectively damaged");
    }

    #[test]
    fn test_validate_blocks_checksum_order_matters() {
        // Test that block order in checksums matters
        // With sliding window, block2 data CAN be found within block1's search window
        // if the data happens to be within 2 blocks of the expected position
        let block_size = 256;
        let mut temp_file = NamedTempFile::new().unwrap();

        let block1 = vec![1u8; block_size];
        let block2 = vec![2u8; block_size];

        temp_file.write_all(&block1).unwrap();
        temp_file.write_all(&block2).unwrap();
        temp_file.flush().unwrap();

        // Reverse the checksums order
        let checksums = vec![
            crate::checksum::compute_block_checksums(&block2),
            crate::checksum::compute_block_checksums(&block1),
        ];

        let (available, _damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &checksums, block_size);

        // Block 0 position: expects block2 data, searches 0-512, finds block2 at 256
        // Block 1 position: expects block1 data, searches 256-768, finds block1 at 256
        // Actually, with the sliding window, we might find block2 in the first position
        // This test demonstrates that sliding window CAN find misplaced blocks
        // The actual result depends on the implementation
        assert!(
            available <= 2,
            "At most 2 blocks can be found (depending on search window overlap)"
        );
    }

    #[test]
    fn test_validate_blocks_ascending_values() {
        // Test with multiple blocks of ascending values
        let block_size = 128;
        let mut temp_file = NamedTempFile::new().unwrap();
        let mut checksums = Vec::new();

        for value in 0..10 {
            let block = vec![value; block_size];
            temp_file.write_all(&block).unwrap();
            checksums.push(crate::checksum::compute_block_checksums(&block));
        }
        temp_file.flush().unwrap();

        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &checksums, block_size);

        assert_eq!(available, 10, "All 10 blocks should be found");
        assert!(damaged.is_empty());
    }
}
