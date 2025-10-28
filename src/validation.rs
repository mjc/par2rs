//! Shared validation logic for verifying file slices/blocks using CRC32 and MD5 checksums.
//!
//! This module provides efficient sequential I/O-based validation that is shared between
//! the verify and repair modules.

use crate::domain::{Crc32Value, Md5Hash};
use rustc_hash::FxHashSet as HashSet;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

/// Buffer size for sequential I/O operations (128MB for optimal throughput)
const BUFFER_CAPACITY: usize = 128 * 1024 * 1024;

/// Calculate the actual size of a slice, handling the last partial slice
#[inline]
fn calculate_slice_size(
    slice_index: usize,
    total_slices: usize,
    slice_size: usize,
    file_size: u64,
) -> usize {
    if slice_index == total_slices - 1 {
        let remaining = (file_size % slice_size as u64) as usize;
        if remaining == 0 {
            slice_size
        } else {
            remaining
        }
    } else {
        slice_size
    }
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
    let file = File::open(file_path)?;
    let mut reader = BufReader::with_capacity(BUFFER_CAPACITY, file);

    // Pre-allocate with expected capacity to avoid rehashing
    let mut valid_slices =
        HashSet::with_capacity_and_hasher(slice_checksums.len(), Default::default());

    // Reuse single buffer for all slices
    let mut slice_data = vec![0u8; slice_size];

    for (slice_index, &expected_crc) in slice_checksums.iter().enumerate() {
        let actual_size =
            calculate_slice_size(slice_index, slice_checksums.len(), slice_size, file_size);

        // Zero padding if needed (PAR2 spec requires zero-padded CRC32)
        if actual_size < slice_size {
            slice_data[actual_size..].fill(0);
        }

        // Sequential read - early continue on read failure
        if reader.read_exact(&mut slice_data[..actual_size]).is_err() {
            continue;
        }

        // Validate CRC32 on full slice (with padding)
        let slice_crc = if actual_size < slice_size {
            crate::checksum::compute_crc32_padded(&slice_data[..actual_size], slice_size)
        } else {
            crate::checksum::compute_crc32(&slice_data[..slice_size])
        };

        if slice_crc == expected_crc {
            valid_slices.insert(slice_index);
        }
    }

    Ok(valid_slices)
}

/// Validates blocks in a file using both MD5 and CRC32 checksums.
///
/// This is used for verification operations where both hash types must match.
/// Uses sequential I/O with a large buffer for optimal throughput.
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
/// - Sequential reads avoid expensive seeking operations
/// - Buffer is reused across all blocks to minimize allocations
pub fn validate_blocks_md5_crc32<P: AsRef<Path>>(
    file_path: P,
    block_checksums: &[(Md5Hash, Crc32Value)],
    block_size: usize,
) -> (usize, Vec<u32>) {
    // Open file or return all blocks as damaged
    let Ok(file) = File::open(file_path) else {
        return (0, (0..block_checksums.len() as u32).collect());
    };

    // Get file size or return all blocks as damaged
    let Ok(metadata) = file.metadata() else {
        return (0, (0..block_checksums.len() as u32).collect());
    };
    let file_size = metadata.len() as usize;

    let mut reader = BufReader::with_capacity(BUFFER_CAPACITY, file);
    let mut buffer = vec![0u8; block_size];

    let mut available_blocks = 0;
    let mut damaged_blocks = Vec::with_capacity(block_checksums.len());

    for (block_index, (expected_md5, expected_crc)) in block_checksums.iter().enumerate() {
        let block_offset = block_index * block_size;

        // Calculate bytes to read for this block
        let bytes_to_read = match () {
            _ if block_offset >= file_size => {
                damaged_blocks.push(block_index as u32);
                continue;
            }
            _ if block_offset + block_size <= file_size => block_size,
            _ => file_size - block_offset,
        };

        // Zero-pad if partial block
        if bytes_to_read < block_size {
            buffer[bytes_to_read..].fill(0);
        }

        // Read block data
        if reader.read_exact(&mut buffer[..bytes_to_read]).is_err() {
            damaged_blocks.push(block_index as u32);
            continue;
        }

        // Compute both MD5 and CRC32 in one pass (more efficient)
        let (block_md5, block_crc) = if bytes_to_read < block_size {
            crate::checksum::compute_block_checksums_padded(&buffer[..bytes_to_read], block_size)
        } else {
            crate::checksum::compute_block_checksums(&buffer[..bytes_to_read])
        };

        // Fast path: Check CRC32 first (cheaper comparison)
        if &block_crc != expected_crc {
            damaged_blocks.push(block_index as u32);
            continue;
        }

        // Slow path: Verify MD5 only if CRC32 matched
        if &block_md5 == expected_md5 {
            available_blocks += 1;
        } else {
            damaged_blocks.push(block_index as u32);
        }
    }

    (available_blocks, damaged_blocks)
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
}
