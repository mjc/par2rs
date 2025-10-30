//! File verification implementation

use super::error::*;
use super::types::*;
use super::utils;
use crate::domain::{Crc32Value, FileId, Md5Hash};
use crate::packets::FileDescriptionPacket;
use std::path::Path;

const DEFAULT_BUFFER_SIZE: usize = 1024;

/// Unified file verifier that can be used by both par2verify and par2repair
///
/// This consolidates the file verification logic that was previously duplicated
/// between the verify and repair modules. It provides efficient file status
/// determination with the 16KB MD5 optimization.
pub struct FileVerifier {
    base_path: std::path::PathBuf,
}

impl FileVerifier {
    /// Create a new file verifier with the specified base path
    pub fn new<P: AsRef<Path>>(base_path: P) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
        }
    }

    /// Determine the status of a single file using efficient verification
    ///
    /// This unified implementation combines the logic from both par2verify and par2repair:
    /// 1. Check file existence
    /// 2. Check file size
    /// 3. Use 16KB MD5 optimization for fast integrity check
    /// 4. Fall back to full MD5 verification if needed
    ///
    /// # Arguments
    /// * `file_name` - Name of the file to verify
    /// * `expected_md5_16k` - Expected MD5 hash of first 16KB
    /// * `expected_md5_full` - Expected MD5 hash of entire file  
    /// * `expected_length` - Expected file length in bytes
    ///
    /// # Returns
    /// FileStatus indicating the current state of the file
    pub fn determine_file_status(
        &self,
        file_name: &str,
        expected_md5_16k: &Md5Hash,
        expected_md5_full: &Md5Hash,
        expected_length: u64,
    ) -> FileStatus {
        let file_path = self.base_path.join(file_name);

        // Functional verification pipeline with proper error handling
        match Self::check_file_existence(&file_path) {
            Err(FileStatus::Missing) => FileStatus::Missing,
            Err(status) => status,
            Ok(_) => Self::check_file_size(&file_path, expected_length)
                .and_then(|_| {
                    Self::verify_file_hashes(&file_path, expected_md5_16k, expected_md5_full)
                })
                .unwrap_or(FileStatus::Corrupted),
        }
    }

    /// Check if file exists
    fn check_file_existence(file_path: &Path) -> Result<(), FileStatus> {
        if file_path.exists() {
            Ok(())
        } else {
            Err(FileStatus::Missing)
        }
    }

    /// Check if file size matches expected
    fn check_file_size(file_path: &Path, expected_length: u64) -> Result<(), FileStatus> {
        std::fs::metadata(file_path)
            .map_err(|_| FileStatus::Corrupted)
            .and_then(|metadata| {
                if metadata.len() == expected_length {
                    Ok(())
                } else {
                    Err(FileStatus::Corrupted)
                }
            })
    }

    /// Verify file hashes using 16KB optimization
    fn verify_file_hashes(
        file_path: &Path,
        expected_md5_16k: &Md5Hash,
        expected_md5_full: &Md5Hash,
    ) -> Result<FileStatus, FileStatus> {
        use crate::checksum::{calculate_file_md5, calculate_file_md5_16k};

        // ULTRA-FAST filter: Check 16KB MD5 first (optimization from repair module)
        calculate_file_md5_16k(file_path)
            .map_err(|_| FileStatus::Corrupted)
            .and_then(|md5_16k| {
                if md5_16k == *expected_md5_16k {
                    // 16KB matches - verify full hash to be certain
                    calculate_file_md5(file_path)
                        .map_err(|_| FileStatus::Corrupted)
                        .map(|file_md5| {
                            if file_md5 == *expected_md5_full {
                                FileStatus::Present
                            } else {
                                FileStatus::Corrupted
                            }
                        })
                } else {
                    // 16KB doesn't match - file is definitely corrupted
                    Err(FileStatus::Corrupted)
                }
            })
    }

    /// Verify file integrity using FileDescription packet data
    ///
    /// This is a convenience method that extracts the necessary hashes and length
    /// from a FileDescription packet and calls determine_file_status.
    pub fn verify_file_from_description(&self, file_desc: &FileDescriptionPacket) -> FileStatus {
        let file_name = utils::extract_file_name(file_desc);

        self.determine_file_status(
            &file_name,
            &file_desc.md5_16k,
            &file_desc.md5_hash,
            file_desc.file_length,
        )
    }

    /// Verify file integrity with progress reporting
    ///
    /// This method provides the same verification as determine_file_status but
    /// can report progress for large files. It uses FileCheckSummer for
    /// comprehensive hash computation when needed.
    pub fn verify_file_with_progress<P: crate::checksum::ProgressReporter>(
        &self,
        file_desc: &FileDescriptionPacket,
        progress: &P,
    ) -> VerificationResult<FileStatus> {
        let file_name = utils::extract_file_name(file_desc);
        let file_path = self.base_path.join(&file_name);

        // Check if file exists, return Missing immediately if not
        match Self::check_file_existence(&file_path) {
            Err(FileStatus::Missing) => Ok(FileStatus::Missing),
            Err(_) => Err(VerificationError::Io("File access error".to_string())),
            Ok(_) => Self::verify_with_checksummer(&file_path, file_desc, progress),
        }
    }

    /// Verify file using FileCheckSummer with progress reporting
    fn verify_with_checksummer<P: crate::checksum::ProgressReporter>(
        file_path: &Path,
        file_desc: &FileDescriptionPacket,
        progress: &P,
    ) -> VerificationResult<FileStatus> {
        crate::checksum::FileCheckSummer::new(
            file_path.to_string_lossy().to_string(),
            DEFAULT_BUFFER_SIZE,
        )
        .map_err(|e| VerificationError::ChecksumCalculation(e.to_string()))
        .and_then(|checksummer| {
            checksummer
                .compute_file_hashes_with_progress(progress)
                .map_err(|e| VerificationError::ChecksumCalculation(e.to_string()))
        })
        .map(|results| {
            // Functional validation of all criteria
            if [
                results.file_size == file_desc.file_length,
                results.hash_16k == file_desc.md5_16k,
                results.hash_full == file_desc.md5_hash,
            ]
            .iter()
            .all(|&valid| valid)
            {
                FileStatus::Present
            } else {
                FileStatus::Corrupted
            }
        })
    }
}

/// Functional helpers for single file verification
pub mod single_file_verification {
    use super::*;

    /// Generate block results for a given range and validity
    pub fn generate_block_results(
        file_id: FileId,
        range: std::ops::Range<usize>,
        is_valid: bool,
        checksums: Option<&[(Md5Hash, Crc32Value)]>,
    ) -> Vec<BlockVerificationResult> {
        range
            .map(|block_num| {
                let (expected_hash, expected_crc) = if let Some(checksums) = checksums {
                    checksums
                        .get(block_num)
                        .map(|(h, c)| (Some(*h), Some(*c)))
                        .unwrap_or((None, None))
                } else {
                    (None, None)
                };

                BlockVerificationResult {
                    block_number: block_num as u32,
                    file_id,
                    is_valid,
                    expected_hash,
                    expected_crc,
                }
            })
            .collect()
    }

    /// Create block results based on validation results and checksums
    pub fn create_corrupted_block_results(
        file_id: FileId,
        checksums: &[(Md5Hash, Crc32Value)],
        damaged_blocks: &[u32],
    ) -> Vec<BlockVerificationResult> {
        checksums
            .iter()
            .enumerate()
            .map(|(block_num, (expected_hash, expected_crc))| {
                let is_valid = !damaged_blocks.contains(&(block_num as u32));
                BlockVerificationResult {
                    block_number: block_num as u32,
                    file_id,
                    is_valid,
                    expected_hash: Some(*expected_hash),
                    expected_crc: Some(*expected_crc),
                }
            })
            .collect()
    }

    /// Determine file status using FileVerifier
    pub fn determine_file_status<P: crate::checksum::ProgressReporter>(
        file_desc: &FileDescriptionPacket,
        progress: Option<&P>,
    ) -> FileStatus {
        let verifier = FileVerifier::new(".");
        match progress {
            Some(progress_reporter) => verifier
                .verify_file_with_progress(file_desc, progress_reporter)
                .unwrap_or(FileStatus::Corrupted),
            None => verifier.verify_file_from_description(file_desc),
        }
    }
}
