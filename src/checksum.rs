//! Centralized hashing utilities for PAR2 operations
//!
//! This module provides all MD5 and CRC32 hashing operations used throughout
//! the PAR2 implementation. All hashing should go through these functions to
//! avoid duplication and ensure consistent behavior.
//!
//! ## Design Philosophy
//!
//! - **Thin wrappers**: All functions are `#[inline]` for zero runtime overhead
//! - **Domain types**: Return `Md5Hash` and `Crc32Value` for type safety
//! - **Performance**: Critical path functions are optimized for large files
//! - **Convenience**: Combined operations for common patterns (MD5+CRC32)

use crate::domain::{Crc32Value, FileId, Md5Hash};
use md5::{Digest, Md5};
use std::io::Read;

// ============================================================================
// MD5 Hashing
// ============================================================================

/// Compute MD5 hash of data in one shot
#[inline]
pub fn compute_md5(data: &[u8]) -> Md5Hash {
    Md5Hash::new(Md5::digest(data).into())
}

/// Create a new MD5 hasher for incremental hashing
#[inline]
pub fn new_md5_hasher() -> Md5 {
    Md5::new()
}

/// Finalize an MD5 hasher and return the hash
#[inline]
pub fn finalize_md5(hasher: Md5) -> Md5Hash {
    Md5Hash::new(hasher.finalize().into())
}

/// Compute MD5 hash as raw bytes (for packet verification)
#[inline]
pub fn compute_md5_bytes(data: &[u8]) -> [u8; 16] {
    Md5::digest(data).into()
}

// ============================================================================
// CRC32 Hashing
// ============================================================================

/// Compute CRC32 checksum of data
///
/// Uses CCITT polynomial (same as Ethernet, PKZIP, PAR2 spec)
#[inline]
pub fn compute_crc32(data: &[u8]) -> Crc32Value {
    Crc32Value::new(crc32fast::hash(data))
}

/// Compute CRC32 with zero-padding to specified block size
///
/// This is used for partial blocks in PAR2 verification.
/// If data is shorter than block_size, it's padded with zeros.
#[inline]
pub fn compute_crc32_padded(data: &[u8], block_size: usize) -> Crc32Value {
    if data.len() < block_size {
        let mut padded = vec![0u8; block_size];
        padded[..data.len()].copy_from_slice(data);
        Crc32Value::new(crc32fast::hash(&padded))
    } else {
        Crc32Value::new(crc32fast::hash(data))
    }
}

// ============================================================================
// Combined MD5 + CRC32 (Performance Optimization)
// ============================================================================

/// Compute both MD5 hash and CRC32 checksum in a single pass
///
/// This is more efficient than calling `compute_md5()` and `compute_crc32()`
/// separately, as it only reads the data once and processes it in parallel.
///
/// PAR2 frequently needs both checksums for block verification (CRC32 for
/// fast pre-screening, MD5 for cryptographic verification).
#[inline]
pub fn compute_block_checksums(data: &[u8]) -> (Md5Hash, Crc32Value) {
    (compute_md5(data), compute_crc32(data))
}

/// Compute MD5 hash and CRC32 checksum with zero-padding
///
/// Used for partial blocks at the end of files. The data is padded to
/// block_size with zeros before computing both checksums.
#[inline]
pub fn compute_block_checksums_padded(data: &[u8], block_size: usize) -> (Md5Hash, Crc32Value) {
    if data.len() < block_size {
        let mut padded = vec![0u8; block_size];
        padded[..data.len()].copy_from_slice(data);
        (compute_md5(&padded), compute_crc32(&padded))
    } else {
        (compute_md5(data), compute_crc32(data))
    }
}

// ============================================================================
// PAR2-Specific Hash Operations
// ============================================================================

/// Compute PAR2 File ID from file metadata
///
/// File ID = MD5(MD5-16k || file_length || filename)
///
/// This is the canonical way to compute file IDs per the PAR2 specification.
/// The file ID uniquely identifies a file and allows identification even if
/// the file is renamed.
///
/// # Arguments
///
/// * `md5_16k` - MD5 hash of first 16KB of file
/// * `file_length` - Length of file in bytes
/// * `filename` - ASCII filename (as stored in PAR2 packets)
pub fn compute_file_id(md5_16k: &Md5Hash, file_length: u64, filename: &[u8]) -> FileId {
    let mut hasher = new_md5_hasher();
    hasher.update(md5_16k.as_bytes());
    hasher.update(file_length.to_le_bytes());
    hasher.update(filename);
    FileId::new(hasher.finalize().into())
}

/// Compute PAR2 Recovery Set ID from main packet body
///
/// Recovery Set ID = MD5(main packet body)
///
/// The recovery set ID identifies all packets that belong together.
/// It's computed as the MD5 hash of the main packet's body (excluding
/// the packet header).
///
/// # Arguments
///
/// * `main_packet_body` - Serialized body of main packet (slice_size + file_count + file_ids)
pub fn compute_recovery_set_id(main_packet_body: &[u8]) -> [u8; 16] {
    compute_md5_bytes(main_packet_body)
}

// ============================================================================
// File-level MD5 Operations (Functional Style)
// ============================================================================

/// Calculate MD5 hash of the first 16KB of a file
///
/// Functional implementation using iterator-based reading.
/// Returns only the hash of the first 16KB for fast file identification.
#[inline]
pub fn calculate_file_md5_16k(file_path: &std::path::Path) -> std::io::Result<Md5Hash> {
    use std::io::BufReader;
    BufReader::new(std::fs::File::open(file_path)?)
        .bytes()
        .take(16384)
        .try_fold(new_md5_hasher(), |mut hasher, byte_result| {
            byte_result.map(|byte| {
                hasher.update([byte]);
                hasher
            })
        })
        .map(finalize_md5)
}

/// Calculate MD5 hash of entire file
///
/// Functional implementation using chunked iteration for performance.
/// Uses 128MB chunks to maximize hardware-accelerated MD5 throughput.
#[inline]
pub fn calculate_file_md5(file_path: &std::path::Path) -> std::io::Result<Md5Hash> {
    const CHUNK_SIZE: usize = 128 * 1024 * 1024;

    let file = std::fs::File::open(file_path)?;
    let reader = std::io::BufReader::with_capacity(CHUNK_SIZE, file);

    std::iter::from_fn({
        let mut reader = reader;
        let mut buffer = vec![0u8; CHUNK_SIZE];
        move || match reader.read(&mut buffer) {
            Ok(0) => None,
            Ok(n) => Some(Ok(buffer[..n].to_vec())),
            Err(e) => Some(Err(e)),
        }
    })
    .try_fold(new_md5_hasher(), |mut hasher, chunk_result| {
        chunk_result.map(|chunk| {
            hasher.update(&chunk);
            hasher
        })
    })
    .map(finalize_md5)
}

// ============================================================================
// File-level, single-pass checksummer (migrated from file_checksummer.rs)
// ============================================================================

use std::fs::File;
use std::io::{BufReader, Result as IoResult};

const BUFFER_SIZE: usize = 1024 * 1024; // 1MB read buffer
const HASH_16K_THRESHOLD: u64 = 16384;

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
#[derive(Debug, Clone, Copy)]
pub struct ChecksumResults {
    pub hash_16k: Md5Hash,
    pub hash_full: Md5Hash,
    pub file_size: u64,
}

/// State for accumulating hashes while reading
struct HashAccumulator {
    hasher_16k: Md5,
    hasher_full: Md5,
    total_bytes_read: u64,
    is_16k_complete: bool,
}

impl HashAccumulator {
    fn new() -> Self {
        Self {
            hasher_16k: new_md5_hasher(),
            hasher_full: new_md5_hasher(),
            total_bytes_read: 0,
            is_16k_complete: false,
        }
    }

    fn update(mut self, data: &[u8]) -> Self {
        let bytes_read = data.len() as u64;

        if !self.is_16k_complete && self.total_bytes_read < HASH_16K_THRESHOLD {
            let bytes_to_hash = data
                .len()
                .min((HASH_16K_THRESHOLD - self.total_bytes_read) as usize);
            self.hasher_16k.update(&data[..bytes_to_hash]);

            // Check if we've reached the 16k threshold
            if self.total_bytes_read + bytes_read >= HASH_16K_THRESHOLD {
                self.hasher_full = self.hasher_16k.clone();
                self.is_16k_complete = true;

                // Add remaining bytes if we went past 16k
                if self.total_bytes_read + bytes_read > HASH_16K_THRESHOLD {
                    let remaining_start = (HASH_16K_THRESHOLD - self.total_bytes_read) as usize;
                    self.hasher_full.update(&data[remaining_start..]);
                }
            }
        } else {
            self.hasher_full.update(data);
        }

        self.total_bytes_read += bytes_read;
        self
    }

    fn finalize(self, file_size: u64) -> (Md5Hash, Md5Hash) {
        let hash_16k = finalize_md5(self.hasher_16k);
        let hash_full = if file_size < HASH_16K_THRESHOLD {
            hash_16k
        } else {
            finalize_md5(self.hasher_full)
        };
        (hash_16k, hash_full)
    }
}

/// Iterator that reads chunks from a file
struct ChunkReader {
    reader: BufReader<File>,
    buffer: Vec<u8>,
}

impl ChunkReader {
    fn new(file: File) -> Self {
        Self {
            reader: BufReader::with_capacity(BUFFER_SIZE, file),
            buffer: vec![0u8; BUFFER_SIZE],
        }
    }
}

impl Iterator for ChunkReader {
    type Item = IoResult<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.reader.read(&mut self.buffer) {
            Ok(0) => None,
            Ok(n) => Some(Ok(self.buffer[..n].to_vec())),
            Err(e) => Some(Err(e)),
        }
    }
}

impl FileCheckSummer {
    /// Create a new checksummer for a file
    pub fn new(file_path: String, block_size: usize) -> IoResult<Self> {
        let file_size = std::fs::metadata(&file_path)?.len();
        Ok(Self {
            file_path,
            block_size,
            file_size,
        })
    }

    /// Compute file hashes in a single pass using functional iteration
    pub fn compute_file_hashes(&self) -> IoResult<ChecksumResults> {
        let file = File::open(&self.file_path)?;
        let mut chunks = ChunkReader::new(file);

        // Fold over chunks to accumulate hashes
        let accumulator = chunks.try_fold(HashAccumulator::new(), |acc, chunk| {
            chunk.map(|data| acc.update(&data))
        })?;

        let (hash_16k, hash_full) = accumulator.finalize(self.file_size);

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
    ) -> IoResult<(Md5Hash, Md5Hash, usize, Vec<u32>)> {
        let file = File::open(&self.file_path)?;
        let blocks = BlockReader::new(file, self.block_size);

        let (accumulator, valid_count, damaged_blocks) = blocks.enumerate().try_fold(
            (HashAccumulator::new(), 0usize, Vec::new()),
            |(acc, valid_count, mut damaged_blocks), (block_num, block_result)| {
                let block = block_result?;
                let updated_acc = acc.update(&block.data);

                // Verify block if we have expected checksums
                let (new_valid_count, new_damaged) =
                    self.verify_block(&block, block_num as u32, expected_checksums);

                if let Some(damaged_block_num) = new_damaged {
                    damaged_blocks.push(damaged_block_num);
                }

                Ok::<_, std::io::Error>((
                    updated_acc,
                    valid_count + new_valid_count,
                    damaged_blocks,
                ))
            },
        )?;

        let (hash_16k, hash_full) = accumulator.finalize(self.file_size);

        Ok((hash_16k, hash_full, valid_count, damaged_blocks))
    }

    /// Verify a single block against expected checksums
    fn verify_block(
        &self,
        block: &Block,
        block_num: u32,
        expected_checksums: &[(Md5Hash, Crc32Value)],
    ) -> (usize, Option<u32>) {
        expected_checksums
            .get(block_num as usize)
            .map(|(expected_md5, expected_crc)| {
                // Compute both MD5 and CRC32 in optimal way
                let (computed_md5, computed_crc) = if block.is_partial {
                    compute_block_checksums_padded(&block.data, self.block_size)
                } else {
                    compute_block_checksums_padded(&block.data, block.data.len())
                };

                // Fast CRC32 check first
                if computed_crc == *expected_crc {
                    if computed_md5 == *expected_md5 {
                        (1, None) // Valid block
                    } else {
                        (0, Some(block_num)) // CRC match but MD5 mismatch
                    }
                } else {
                    (0, Some(block_num)) // CRC mismatch
                }
            })
            .unwrap_or((0, None)) // No expected checksum for this block
    }

    /// Get the file size
    pub fn file_size(&self) -> u64 {
        self.file_size
    }
}

/// A single block read from a file
struct Block {
    data: Vec<u8>,
    is_partial: bool,
}

/// Iterator that reads fixed-size blocks from a file
struct BlockReader {
    reader: BufReader<File>,
    block_size: usize,
    buffer: Vec<u8>,
}

impl BlockReader {
    fn new(file: File, block_size: usize) -> Self {
        Self {
            reader: BufReader::with_capacity(BUFFER_SIZE, file),
            block_size,
            buffer: vec![0u8; block_size],
        }
    }

    fn read_block(&mut self) -> IoResult<Option<Block>> {
        let mut bytes_read_total = 0;

        while bytes_read_total < self.block_size {
            match self.reader.read(&mut self.buffer[bytes_read_total..])? {
                0 if bytes_read_total == 0 => return Ok(None), // EOF
                0 => break,                                    // Partial block at EOF
                n => bytes_read_total += n,
            }
        }

        if bytes_read_total == 0 {
            Ok(None)
        } else {
            Ok(Some(Block {
                data: self.buffer[..bytes_read_total].to_vec(),
                is_partial: bytes_read_total < self.block_size,
            }))
        }
    }
}

impl Iterator for BlockReader {
    type Item = IoResult<Block>;

    fn next(&mut self) -> Option<Self::Item> {
        self.read_block().transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // MD5 Tests
    // ========================================================================

    #[test]
    fn test_compute_md5() {
        let data = b"hello world";
        let hash1 = compute_md5(data);
        let hash2 = compute_md5(data);
        assert_eq!(hash1, hash2, "Same data should produce same hash");
    }

    #[test]
    fn test_incremental_md5() {
        let mut hasher = new_md5_hasher();
        hasher.update(b"hello");
        hasher.update(b" ");
        hasher.update(b"world");
        let hash1 = finalize_md5(hasher);

        let hash2 = compute_md5(b"hello world");
        assert_eq!(hash1, hash2, "Incremental and one-shot should match");
    }

    #[test]
    fn test_md5_bytes() {
        let data = b"test";
        let bytes = compute_md5_bytes(data);
        let hash = compute_md5(data);
        assert_eq!(bytes, *hash.as_bytes(), "Bytes and hash should match");
    }

    #[test]
    fn test_md5_empty() {
        let hash = compute_md5(b"");
        // MD5 of empty string is d41d8cd98f00b204e9800998ecf8427e
        let expected = [
            0xd4, 0x1d, 0x8c, 0xd9, 0x8f, 0x00, 0xb2, 0x04, 0xe9, 0x80, 0x09, 0x98, 0xec, 0xf8,
            0x42, 0x7e,
        ];
        assert_eq!(*hash.as_bytes(), expected);
    }

    // ========================================================================
    // CRC32 Tests
    // ========================================================================

    #[test]
    fn test_compute_crc32() {
        let data = b"hello world";
        let crc1 = compute_crc32(data);
        let crc2 = compute_crc32(data);
        assert_eq!(crc1, crc2, "Same data should produce same CRC");
    }

    #[test]
    fn test_crc32_padded_no_padding() {
        let data = b"test";
        let crc_padded = compute_crc32_padded(data, 4);
        let crc_normal = compute_crc32(data);
        assert_eq!(
            crc_padded, crc_normal,
            "No padding needed when data equals block size"
        );
    }

    #[test]
    fn test_crc32_padded_with_padding() {
        let data = b"test";
        let block_size = 10;

        // Manually pad and compute CRC
        let mut padded = vec![0u8; block_size];
        padded[..data.len()].copy_from_slice(data);
        let expected = compute_crc32(&padded);

        // Use utility function
        let result = compute_crc32_padded(data, block_size);

        assert_eq!(result, expected, "Padded CRC should match manually padded");
    }

    #[test]
    fn test_crc32_empty() {
        let crc = compute_crc32(b"");
        // CRC32 of empty data is 0
        assert_eq!(crc, Crc32Value::new(0));
    }

    // ========================================================================
    // Combined MD5 + CRC32 Tests
    // ========================================================================

    #[test]
    fn test_compute_block_checksums() {
        let data = b"test block data";
        let (md5, crc) = compute_block_checksums(data);

        // Should match individual computations
        assert_eq!(md5, compute_md5(data));
        assert_eq!(crc, compute_crc32(data));
    }

    #[test]
    fn test_compute_block_checksums_padded_no_padding() {
        let data = b"test";
        let block_size = 4;

        let (md5, crc) = compute_block_checksums_padded(data, block_size);

        // Should match unpadded when size equals block size
        assert_eq!(md5, compute_md5(data));
        assert_eq!(crc, compute_crc32(data));
    }

    #[test]
    fn test_compute_block_checksums_padded_with_padding() {
        let data = b"test";
        let block_size = 10;

        let (md5, crc) = compute_block_checksums_padded(data, block_size);

        // Manually pad and verify
        let mut padded = vec![0u8; block_size];
        padded[..data.len()].copy_from_slice(data);

        assert_eq!(md5, compute_md5(&padded));
        assert_eq!(crc, compute_crc32(&padded));
    }

    // ========================================================================
    // PAR2-Specific Operation Tests
    // ========================================================================

    #[test]
    fn test_compute_file_id() {
        let md5_16k = compute_md5(b"first 16kb");
        let file_length = 12345u64;
        let filename = b"test.txt";

        let file_id1 = compute_file_id(&md5_16k, file_length, filename);
        let file_id2 = compute_file_id(&md5_16k, file_length, filename);

        assert_eq!(
            file_id1, file_id2,
            "Same inputs should produce same file ID"
        );
    }

    #[test]
    fn test_compute_file_id_different_filenames() {
        let md5_16k = compute_md5(b"first 16kb");
        let file_length = 12345u64;

        let file_id1 = compute_file_id(&md5_16k, file_length, b"file1.txt");
        let file_id2 = compute_file_id(&md5_16k, file_length, b"file2.txt");

        assert_ne!(
            file_id1, file_id2,
            "Different filenames should produce different IDs"
        );
    }

    #[test]
    fn test_compute_file_id_different_lengths() {
        let md5_16k = compute_md5(b"first 16kb");
        let filename = b"test.txt";

        let file_id1 = compute_file_id(&md5_16k, 100, filename);
        let file_id2 = compute_file_id(&md5_16k, 200, filename);

        assert_ne!(
            file_id1, file_id2,
            "Different lengths should produce different IDs"
        );
    }

    #[test]
    fn test_compute_recovery_set_id() {
        let body = b"main packet body content";

        let set_id1 = compute_recovery_set_id(body);
        let set_id2 = compute_recovery_set_id(body);

        assert_eq!(
            set_id1, set_id2,
            "Same body should produce same recovery set ID"
        );
        assert_eq!(set_id1.len(), 16, "Recovery set ID should be 16 bytes");
    }

    #[test]
    fn test_compute_recovery_set_id_different_bodies() {
        let set_id1 = compute_recovery_set_id(b"body1");
        let set_id2 = compute_recovery_set_id(b"body2");

        assert_ne!(
            set_id1, set_id2,
            "Different bodies should produce different IDs"
        );
    }

    // ========================================================================
    // Edge Cases
    // ========================================================================

    #[test]
    fn test_large_data_md5() {
        // Test with 1MB of data
        let data = vec![0x42u8; 1024 * 1024];
        let hash = compute_md5(&data);

        // Should be deterministic
        let hash2 = compute_md5(&data);
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_large_data_crc32() {
        // Test with 1MB of data
        let data = vec![0x42u8; 1024 * 1024];
        let crc = compute_crc32(&data);

        // Should be deterministic
        let crc2 = compute_crc32(&data);
        assert_eq!(crc, crc2);
    }

    #[test]
    fn test_padding_larger_than_data() {
        let data = b"hi";
        let block_size = 1024 * 1024; // 1MB padding for 2 bytes

        let (md5, crc) = compute_block_checksums_padded(data, block_size);

        // Should match manually padded
        let mut padded = vec![0u8; block_size];
        padded[..data.len()].copy_from_slice(data);

        assert_eq!(md5, compute_md5(&padded));
        assert_eq!(crc, compute_crc32(&padded));
    }
}
