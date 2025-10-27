//! Single-pass file checksummer for efficient verification
//!
//! This module provides a FileCheckSummer that reads a file once and computes:
//! - MD5 hash of first 16KB
//! - MD5 hash of full file
//! - Rolling CRC32 checksums for block matching
//!
//! Based on the par2cmdline approach for optimal I/O performance.

use crate::domain::{Crc32Value, Md5Hash};
use md5::{Digest, Md5};
use std::fs::File;
use std::io::{BufReader, Read};

const BUFFER_SIZE: usize = 1024 * 1024; // 1MB read buffer

/// Single-pass file checksummer
///
/// Reads a file once and accumulates MD5 hashes while also providing
/// block-level checksums for verification.
pub struct FileCheckSummer {
    file_path: String,
    block_size: usize,
    file_size: u64,
}

/// Results from checksumming a file
pub struct ChecksumResults {
    pub hash_16k: Md5Hash,
    pub hash_full: Md5Hash,
    pub file_size: u64,
}

impl FileCheckSummer {
    /// Create a new checksummer for a file
    pub fn new(file_path: String, block_size: usize) -> std::io::Result<Self> {
        let metadata = std::fs::metadata(&file_path)?;
        let file_size = metadata.len();

        Ok(Self {
            file_path,
            block_size,
            file_size,
        })
    }

    /// Compute file hashes in a single pass
    ///
    /// This reads the file once and computes both the 16KB and full file MD5 hashes.
    pub fn compute_file_hashes(&self) -> std::io::Result<ChecksumResults> {
        let file = File::open(&self.file_path)?;
        let mut reader = BufReader::with_capacity(BUFFER_SIZE, file);

        let mut hasher_16k = Md5::new();
        let mut hasher_full = Md5::new();
        let mut buffer = vec![0u8; BUFFER_SIZE];
        let mut total_read = 0u64;
        let mut hash_16k_finalized = false;

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }

            let data = &buffer[..bytes_read];

            // Update 16k hash if we haven't finished it yet
            if !hash_16k_finalized {
                if total_read < 16384 {
                    let bytes_for_16k = std::cmp::min(bytes_read, (16384 - total_read) as usize);
                    hasher_16k.update(&data[..bytes_for_16k]);

                    // If we just reached or passed 16k, copy the state to full hasher
                    if total_read + bytes_read as u64 >= 16384 {
                        hasher_full = hasher_16k.clone();
                        hash_16k_finalized = true;

                        // Add remaining bytes to full hasher if we went past 16k
                        if total_read + bytes_read as u64 > 16384 {
                            let remaining_start = (16384 - total_read) as usize;
                            hasher_full.update(&data[remaining_start..]);
                        }
                    }
                }
            } else {
                // Just update full hash
                hasher_full.update(data);
            }

            total_read += bytes_read as u64;
        }

        // Get the 16k hash
        let hash_16k = if self.file_size < 16384 {
            // For files smaller than 16k, 16k hash = full hash
            Md5Hash::new(hasher_16k.finalize().into())
        } else {
            Md5Hash::new(hasher_16k.finalize().into())
        };

        // Get the full hash
        let hash_full = if self.file_size < 16384 {
            hash_16k
        } else {
            Md5Hash::new(hasher_full.finalize().into())
        };

        Ok(ChecksumResults {
            hash_16k,
            hash_full,
            file_size: self.file_size,
        })
    }

    /// Scan file with block-level CRC32 checksums and accumulate MD5
    ///
    /// This performs a single pass that:
    /// 1. Accumulates MD5 for 16k and full file
    /// 2. Computes CRC32 for each block
    /// 3. Returns which blocks match expected checksums
    ///
    /// Returns: (hash_16k, hash_full, valid_blocks_count, damaged_block_numbers)
    pub fn scan_with_block_checksums(
        &self,
        expected_checksums: &[(Md5Hash, Crc32Value)],
    ) -> std::io::Result<(Md5Hash, Md5Hash, usize, Vec<u32>)> {
        let file = File::open(&self.file_path)?;
        let mut reader = BufReader::with_capacity(BUFFER_SIZE, file);

        let mut hasher_16k = Md5::new();
        let mut hasher_full = Md5::new();
        let mut block_buffer = vec![0u8; self.block_size];
        let mut total_read = 0u64;
        let mut hash_16k_finalized = false;

        let mut valid_blocks = 0;
        let mut damaged_blocks = Vec::new();
        let mut block_num = 0u32;

        loop {
            // Read one block
            let mut bytes_in_block = 0;
            while bytes_in_block < self.block_size {
                let bytes_to_read = self.block_size - bytes_in_block;
                let bytes_read = reader
                    .read(&mut block_buffer[bytes_in_block..bytes_in_block + bytes_to_read])?;

                if bytes_read == 0 {
                    break; // EOF
                }

                bytes_in_block += bytes_read;
            }

            if bytes_in_block == 0 {
                break; // No more data
            }

            let actual_data = &block_buffer[..bytes_in_block];

            // Update MD5 hashes
            if !hash_16k_finalized {
                if total_read < 16384 {
                    let bytes_for_16k =
                        std::cmp::min(bytes_in_block, (16384 - total_read) as usize);
                    hasher_16k.update(&actual_data[..bytes_for_16k]);

                    if total_read + bytes_in_block as u64 >= 16384 {
                        hasher_full = hasher_16k.clone();
                        hash_16k_finalized = true;

                        if total_read + bytes_in_block as u64 > 16384 {
                            let remaining_start = (16384 - total_read) as usize;
                            hasher_full.update(&actual_data[remaining_start..]);
                        }
                    }
                }
            } else {
                hasher_full.update(actual_data);
            }

            // Verify block if we have expected checksums
            if let Some((expected_md5, expected_crc)) = expected_checksums.get(block_num as usize) {
                // Compute CRC32 with padding (PAR2 spec)
                let computed_crc = if bytes_in_block < self.block_size {
                    // Pad the block for CRC32 calculation
                    let mut padded = block_buffer.clone();
                    padded[bytes_in_block..].fill(0);
                    Crc32Value::new(crc32fast::hash(&padded))
                } else {
                    Crc32Value::new(crc32fast::hash(actual_data))
                };

                // First check CRC32 (fast)
                if computed_crc == *expected_crc {
                    // CRC matches, now verify MD5 (slower but only on matches)
                    let computed_md5 = Md5Hash::new(Md5::digest(actual_data).into());

                    if computed_md5 == *expected_md5 {
                        valid_blocks += 1;
                    } else {
                        damaged_blocks.push(block_num);
                    }
                } else {
                    damaged_blocks.push(block_num);
                }
            }

            total_read += bytes_in_block as u64;
            block_num += 1;

            if bytes_in_block < self.block_size {
                break; // Last block
            }
        }

        // Get the hashes
        let hash_16k = Md5Hash::new(hasher_16k.finalize().into());

        let hash_full = if self.file_size < 16384 {
            hash_16k
        } else {
            Md5Hash::new(hasher_full.finalize().into())
        };

        Ok((hash_16k, hash_full, valid_blocks, damaged_blocks))
    }

    /// Get the file size
    pub fn file_size(&self) -> u64 {
        self.file_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_compute_hashes_small_file() {
        // Create a small test file (< 16KB)
        let mut temp_file = NamedTempFile::new().unwrap();
        let data = b"Hello, World!";
        temp_file.write_all(data).unwrap();
        temp_file.flush().unwrap();

        let checksummer =
            FileCheckSummer::new(temp_file.path().to_string_lossy().to_string(), 1024).unwrap();

        let results = checksummer.compute_file_hashes().unwrap();

        // For files < 16k, both hashes should be the same
        assert_eq!(results.hash_16k, results.hash_full);
        assert_eq!(results.file_size, data.len() as u64);
    }

    #[test]
    fn test_compute_hashes_large_file() {
        // Create a file > 16KB
        let mut temp_file = NamedTempFile::new().unwrap();
        let data = vec![0xAB_u8; 20000];
        temp_file.write_all(&data).unwrap();
        temp_file.flush().unwrap();

        let checksummer =
            FileCheckSummer::new(temp_file.path().to_string_lossy().to_string(), 1024).unwrap();

        let results = checksummer.compute_file_hashes().unwrap();

        // For files >= 16k, hashes should be different
        assert_ne!(results.hash_16k, results.hash_full);
        assert_eq!(results.file_size, data.len() as u64);
    }
}
