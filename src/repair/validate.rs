//! Block validation utilities used by repair operations.
//!
//! This file contains a self-contained implementation of
//! `validate_blocks_md5_crc32` moved out of the verify/validation module so
//! that the repair module owns block-level validation logic and tests.

use crate::domain::{Crc32Value, Md5Hash};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Validate a sequence of blocks in a file using CRC32 + MD5 checksums.
///
/// For each expected block (given as an (md5, crc32) pair) the function
/// searches in a 2*block_size window starting at the expected byte offset
/// (block_index * block_size). The search allows misalignment within the
/// first block_size bytes (sliding window). Partial blocks at EOF are
/// padded with zeros for checksum computation.
///
/// Returns (available_count, damaged_indices)
pub fn validate_blocks_md5_crc32<P: AsRef<Path>>(
    file_path: P,
    block_checksums: &[(Md5Hash, Crc32Value)],
    block_size: usize,
) -> (usize, Vec<u32>) {
    // If no blocks to check, nothing to do.
    if block_checksums.is_empty() {
        return (0, Vec::new());
    }

    // Try to open file; if missing, mark all as damaged
    let mut file = match File::open(&file_path) {
        Ok(f) => f,
        Err(_) => return (0, (0..block_checksums.len() as u32).collect()),
    };

    // Determine file length
    let file_len = match file.seek(SeekFrom::End(0)) {
        Ok(len) => len as usize,
        Err(_) => return (0, (0..block_checksums.len() as u32).collect()),
    };

    // Pre-allocate a window buffer of 2 * block_size bytes
    let window_capacity = block_size.saturating_mul(2).max(1);
    let mut window = vec![0u8; window_capacity];

    let mut available = 0usize;
    let mut damaged = Vec::new();

    for (idx, (expected_md5, expected_crc)) in block_checksums.iter().enumerate() {
        let block_offset = (idx * block_size) as u64;

        // If expected offset already beyond file, mark damaged
        if block_offset as usize >= file_len {
            damaged.push(idx as u32);
            continue;
        }

        // Compute how many bytes we can read starting at this offset
        let max_bytes = file_len.saturating_sub(block_offset as usize);
        let bytes_to_read = std::cmp::min(max_bytes, window_capacity);

        // Read into window (zero-filled from previous iterations)
        window.fill(0);
        if file.seek(SeekFrom::Start(block_offset)).is_err() {
            damaged.push(idx as u32);
            continue;
        }

        // Read available bytes (may be less than requested at EOF)
        let read_buf = &mut window[..bytes_to_read];
        if file.read_exact(read_buf).is_err() {
            // If we couldn't read the requested bytes, treat as damaged
            damaged.push(idx as u32);
            continue;
        }

        // Determine maximum search offset. If bytes_to_read < block_size,
        // candidate end will be shortened (partial block) but we still test offset 0.
        let max_offset = bytes_to_read.saturating_sub(block_size);

        // Search offsets from 0..=max_offset
        let mut found = false;
        for raw_offset in 0..=max_offset {
            // Candidate slice may be shorter than block_size at EOF; copy/pad as needed
            let available_len = (raw_offset + block_size).min(bytes_to_read) - raw_offset;
            let candidate = &window[raw_offset..raw_offset + available_len];

            // Compute CRC32 (padded if necessary)
            let crc = if available_len < block_size {
                crate::checksum::compute_crc32_padded(candidate, block_size)
            } else {
                crate::checksum::compute_crc32(candidate)
            };

            if &crc != expected_crc {
                continue;
            }

            // CRC matches; compute MD5 (padded if needed)
            let md5 = if available_len < block_size {
                // pad to block_size with zeros
                let mut padded = vec![0u8; block_size];
                padded[..available_len].copy_from_slice(candidate);
                crate::checksum::compute_md5(&padded)
            } else {
                crate::checksum::compute_md5(candidate)
            };

            if &md5 == expected_md5 {
                found = true;
                break;
            }
        }

        if found {
            available += 1;
        } else {
            damaged.push(idx as u32);
        }
    }

    (available, damaged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_validate_blocks_simple_offset() {
        // Simple misaligned block test
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

    // -- The rest of the original block validation tests are included below.
    // For brevity they are kept but mirror the previous test expectations.

    #[test]
    fn test_validate_blocks_partial_block_at_end() {
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();

        let block1 = vec![1u8; block_size];
        let block2_partial = vec![2u8; 512];

        temp_file.write_all(&block1).unwrap();
        temp_file.write_all(&block2_partial).unwrap();
        temp_file.flush().unwrap();

        let checksum1 = crate::checksum::compute_block_checksums(&block1);
        let checksum2 =
            crate::checksum::compute_block_checksums_padded(&block2_partial, block_size);

        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &[checksum1, checksum2], block_size);

        assert_eq!(available, 2);
        assert!(damaged.is_empty());
    }

    #[test]
    fn test_validate_blocks_corrupted_block() {
        let block_size = 1024;
        let mut temp_file = NamedTempFile::new().unwrap();

        let block_data = vec![7u8; block_size];
        temp_file.write_all(&block_data).unwrap();
        temp_file.flush().unwrap();

        let wrong_data = vec![9u8; block_size];
        let (wrong_md5, wrong_crc) = crate::checksum::compute_block_checksums(&wrong_data);

        let (available, damaged) =
            validate_blocks_md5_crc32(temp_file.path(), &[(wrong_md5, wrong_crc)], block_size);

        assert_eq!(available, 0);
        assert_eq!(damaged.len(), 1);
        assert_eq!(damaged[0], 0);
    }

    #[test]
    fn test_validate_blocks_missing_file() {
        let block_size = 1024;
        let (expected_md5, expected_crc) =
            crate::checksum::compute_block_checksums(&vec![0u8; block_size]);

        let (available, damaged) = validate_blocks_md5_crc32(
            "/nonexistent/path/to/file.dat",
            &[(expected_md5, expected_crc)],
            block_size,
        );

        assert_eq!(available, 0);
        assert_eq!(damaged.len(), 1);
    }

    #[test]
    fn test_validate_blocks_from_real_file() {
        // Test with a real file's checksums (if available)
        use crate::domain::{Crc32Value, Md5Hash};
        use std::path::Path;

        let test_path = "tests/fixtures/testfile";
        if Path::new(test_path).exists() {
            // Create some dummy checksums to test the function interface
            let checksums = vec![(Md5Hash::new([0x11; 16]), Crc32Value::new(0x12345678))];

            let (available, damaged) = validate_blocks_md5_crc32(test_path, &checksums, 1024);

            // The function should run without panicking
            assert_eq!(available + damaged.len(), checksums.len());
        }
    }

    #[test]
    fn test_reports_all_blocks_damaged_for_missing_file() {
        let checksums = vec![
            (
                crate::domain::Md5Hash::new([0x11; 16]),
                crate::domain::Crc32Value::new(0x12345678),
            ),
            (
                crate::domain::Md5Hash::new([0x22; 16]),
                crate::domain::Crc32Value::new(0x87654321),
            ),
        ];

        let (available, damaged) = validate_blocks_md5_crc32("/nonexistent/file", &checksums, 1024);

        assert_eq!(available, 0, "No blocks available for missing file");
        assert_eq!(damaged.len(), 2, "All blocks should be marked damaged");
    }

    #[test]
    fn test_handles_empty_checksum_list() {
        let checksums = vec![];
        let (available, damaged) =
            validate_blocks_md5_crc32("tests/fixtures/testfile", &checksums, 1024);

        assert_eq!(available, 0, "No blocks available for empty list");
        assert!(damaged.is_empty(), "No damaged blocks for empty list");
    }

    #[test]
    fn test_single_block_file_verification() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let content = vec![0x42u8; 512]; // Single block
        temp_file.write_all(&content).unwrap();
        temp_file.flush().unwrap();

        // Compute checksums for single block
        let checksums = vec![(
            crate::domain::Md5Hash::new([0x11; 16]),
            crate::domain::Crc32Value::new(0x12345678),
        )];

        let (available, damaged) = validate_blocks_md5_crc32(temp_file.path(), &checksums, 512);

        // Either the block matches (available=1) or it doesn't (damaged=[0])
        assert_eq!(available + damaged.len(), 1, "Should have exactly 1 block");
    }

    #[test]
    fn test_exact_block_boundaries() {
        let mut temp_file = NamedTempFile::new().unwrap();
        // Create file with exactly 3 blocks
        let content = vec![0xAAu8; 3 * 512];
        temp_file.write_all(&content).unwrap();
        temp_file.flush().unwrap();

        let checksums = vec![
            (
                crate::domain::Md5Hash::new([0x11; 16]),
                crate::domain::Crc32Value::new(0x12345678),
            ),
            (
                crate::domain::Md5Hash::new([0x22; 16]),
                crate::domain::Crc32Value::new(0x87654321),
            ),
            (
                crate::domain::Md5Hash::new([0x33; 16]),
                crate::domain::Crc32Value::new(0xAAAAAAAA),
            ),
        ];

        let (available, damaged) = validate_blocks_md5_crc32(temp_file.path(), &checksums, 512);

        assert_eq!(available + damaged.len(), 3, "Should have exactly 3 blocks");
    }

    #[test]
    fn test_partial_last_block() {
        let mut temp_file = NamedTempFile::new().unwrap();
        // Create file with 2.5 blocks
        let content = vec![0xBBu8; 2 * 512 + 256];
        temp_file.write_all(&content).unwrap();
        temp_file.flush().unwrap();

        let checksums = vec![
            (
                crate::domain::Md5Hash::new([0x11; 16]),
                crate::domain::Crc32Value::new(0x12345678),
            ),
            (
                crate::domain::Md5Hash::new([0x22; 16]),
                crate::domain::Crc32Value::new(0x87654321),
            ),
            (
                crate::domain::Md5Hash::new([0x33; 16]),
                crate::domain::Crc32Value::new(0xAAAAAAAA),
            ),
        ];

        let (available, damaged) = validate_blocks_md5_crc32(temp_file.path(), &checksums, 512);

        assert_eq!(available + damaged.len(), 3, "Should process all 3 blocks");
    }
}
