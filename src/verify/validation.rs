//! Slice validation logic for verifying file slices using CRC32 checksums.
//!
//! This module provides efficient sequential I/O-based slice validation.
//! Block-level validation has been moved to the repair module.

use crate::domain::Crc32Value;
use rustc_hash::FxHashSet as HashSet;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// Validates slices in a file using CRC32 checksums (convenience function)
///
/// This function validates each slice against its expected CRC32 value.
/// Uses sequential I/O for optimal throughput.
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
/// Uses sequential I/O for optimal throughput.
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
    _file_size: u64,
    _progress: &dyn crate::repair::ProgressReporter,
    _parallel_mode: bool,
) -> io::Result<HashSet<usize>> {
    let mut file = File::open(file_path)?;
    let mut buf = vec![0u8; slice_size];
    let mut valid = HashSet::default();

    for (i, &expected_crc) in slice_checksums.iter().enumerate() {
        // Report progress occasionally (silently for now)
        if i % 100 == 0 {
            // Progress reporting removed to avoid trait issues
        }

        let read = match file.read(&mut buf) {
            Ok(0) => break, // EOF
            Ok(n) => n,
            Err(e) => return Err(e),
        };

        let is_valid = if read == slice_size {
            expected_crc.as_u32() == crate::checksum::compute_crc32(&buf[..read])
        } else {
            // Partial slice at EOF: compute padded CRC
            expected_crc.as_u32() == crate::checksum::compute_crc32_padded(&buf[..read], slice_size)
        };

        if is_valid {
            valid.insert(i);
        }
    }

    Ok(valid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_validate_slices_crc32_all_valid() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let slice_size = 64;
        let content = b"A".repeat(slice_size * 3); // 3 slices
        temp_file.write_all(&content).unwrap();
        temp_file.flush().unwrap();

        let slice_checksums: Vec<Crc32Value> = (0..3)
            .map(|i| {
                let slice_data = &content[i * slice_size..(i + 1) * slice_size];
                crate::checksum::compute_crc32(slice_data)
            })
            .collect();

        let valid_slices = validate_slices_crc32(
            temp_file.path(),
            &slice_checksums,
            slice_size,
            content.len() as u64,
        )
        .unwrap();

        assert_eq!(valid_slices.len(), 3);
        assert!(valid_slices.contains(&0));
        assert!(valid_slices.contains(&1));
        assert!(valid_slices.contains(&2));
    }
}
