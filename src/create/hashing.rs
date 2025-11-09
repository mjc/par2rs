//! File hashing functionality for PAR2 creation
//!
//! This module handles parallel computation of MD5 hashes and CRC32 checksums
//! for source files during PAR2 creation.
//!
//! Reference: par2cmdline-turbo/src/par2creator.cpp OpenSourceFiles() and
//! FinishFileHashComputation()

use crate::checksum::{calculate_file_md5_16k, compute_block_checksums_padded, Md5Reader};
use std::fs::File;
use std::io::Read;

use super::error::{CreateError, CreateResult};
use super::progress::CreateReporter;
use super::source_file::{BlockChecksum, SourceFileInfo};

/// Hash a single source file and compute block checksums
///
/// This computes:
/// - Full file MD5 hash (streamed during block checksum computation)
/// - First 16KB MD5 hash (for FileId generation)
/// - Per-block MD5 + CRC32 checksums
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp OpenSourceFiles()
pub fn hash_source_file(
    source_file: &mut SourceFileInfo,
    block_size: u64,
    global_block_offset: u32,
    reporter: &dyn CreateReporter,
) -> CreateResult<()> {
    let path = &source_file.path;

    // Compute 16KB hash for FileId
    let hash_16k = calculate_file_md5_16k(path).map_err(|e| CreateError::FileReadError {
        file: path.to_string_lossy().to_string(),
        source: e,
    })?;

    // Compute FileId from [Hash16k, Length, Name]
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .as_bytes();
    let file_id = crate::checksum::compute_file_id(&hash_16k, source_file.size, filename);

    // Compute block checksums AND full file MD5 in single pass
    let block_count = source_file.calculate_block_count(block_size);
    let mut block_checksums = Vec::with_capacity(block_count as usize);

    let hash_full = if source_file.size > 0 {
        let file = File::open(path).map_err(|e| CreateError::FileReadError {
            file: path.to_string_lossy().to_string(),
            source: e,
        })?;

        // Wrap file in Md5Reader to compute hash while reading blocks
        let mut md5_reader = Md5Reader::new(file);
        let mut buffer = vec![0u8; block_size as usize];

        for block_idx in 0..block_count {
            // Read block data
            let bytes_read =
                md5_reader
                    .read(&mut buffer)
                    .map_err(|e| CreateError::FileReadError {
                        file: path.to_string_lossy().to_string(),
                        source: e,
                    })?;

            if bytes_read == 0 {
                break;
            }

            // Compute checksums (with padding for last block if needed)
            let (md5, crc32) = if bytes_read < block_size as usize {
                compute_block_checksums_padded(&buffer[..bytes_read], block_size as usize)
            } else {
                crate::checksum::compute_block_checksums(&buffer[..bytes_read])
            };

            block_checksums.push(BlockChecksum {
                crc32: crc32.as_u32(),
                hash: md5,
                global_index: global_block_offset + block_idx,
            });

            // Report progress
            let bytes_processed =
                std::cmp::min((block_idx + 1) as u64 * block_size, source_file.size);
            reporter.report_file_hashing(
                &source_file.filename(),
                bytes_processed,
                source_file.size,
            );
        }

        // Finalize to get full file MD5 hash
        let (_, hash) = md5_reader.finalize();
        crate::domain::Md5Hash::new(hash)
    } else {
        // Empty file - compute MD5 of empty data
        crate::checksum::compute_md5(&[])
    };

    // Update source file info
    source_file.file_id = file_id;
    source_file.hash = hash_full;
    source_file.hash_16k = hash_16k;
    source_file.block_checksums = block_checksums;
    source_file.block_count = block_count;
    source_file.global_block_offset = global_block_offset;

    Ok(())
}

/// Hash all source files in parallel
///
/// Computes MD5 hashes, CRC32 checksums, and FileIDs for all source files.
/// Uses Rayon for parallel processing of independent files.
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp ComputeFileIds()
pub fn hash_all_source_files(
    source_files: &mut [SourceFileInfo],
    block_size: u64,
    reporter: &dyn CreateReporter,
    use_parallel: bool,
) -> CreateResult<()> {
    if source_files.is_empty() {
        return Ok(());
    }

    // Calculate global block offsets for each file
    let mut file_offsets = Vec::with_capacity(source_files.len());
    let mut global_offset = 0u32;
    for source_file in source_files.iter() {
        file_offsets.push(global_offset);
        global_offset += source_file.calculate_block_count(block_size);
    }

    // Hash files - use parallel or sequential based on thread count
    let results: Vec<CreateResult<()>> = if use_parallel {
        use rayon::prelude::*;
        source_files
            .par_iter_mut()
            .enumerate()
            .map(|(idx, source_file)| {
                hash_source_file(source_file, block_size, file_offsets[idx], reporter)
            })
            .collect()
    } else {
        source_files
            .iter_mut()
            .enumerate()
            .map(|(idx, source_file)| {
                hash_source_file(source_file, block_size, file_offsets[idx], reporter)
            })
            .collect()
    };

    // Check for errors
    for (idx, result) in results.into_iter().enumerate() {
        result.map_err(|e| {
            CreateError::Other(format!(
                "Failed to hash file {}: {}",
                source_files[idx].filename(),
                e
            ))
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::progress::SilentCreateReporter;
    use crate::domain::{FileId, Md5Hash};
    use tempfile::tempdir;

    #[test]
    fn test_hash_source_file_small() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("test.dat");

        // Create test file (1KB)
        let data = vec![0xAA; 1024];
        std::fs::write(&file_path, &data).unwrap();

        let mut source_info = SourceFileInfo::new(file_path.clone(), 1024, 0);
        let reporter = SilentCreateReporter;

        hash_source_file(&mut source_info, 512, 0, &reporter).unwrap();

        // Should have 2 blocks (1024 / 512)
        assert_eq!(source_info.block_count, 2);
        assert_eq!(source_info.block_checksums.len(), 2);
        assert_eq!(source_info.global_block_offset, 0);

        // File hash should be computed
        assert_ne!(source_info.hash, Md5Hash::new([0u8; 16]));

        // FileId should be computed
        assert_ne!(source_info.file_id, FileId::new([0u8; 16]));
    }

    #[test]
    fn test_hash_source_file_with_partial_block() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("test.dat");

        // Create test file with partial last block (1536 bytes = 1.5 blocks of 1024)
        let data = vec![0xBB; 1536];
        std::fs::write(&file_path, &data).unwrap();

        let mut source_info = SourceFileInfo::new(file_path.clone(), 1536, 0);
        let reporter = SilentCreateReporter;

        hash_source_file(&mut source_info, 1024, 10, &reporter).unwrap();

        // Should have 2 blocks (ceiling of 1536/1024)
        assert_eq!(source_info.block_count, 2);
        assert_eq!(source_info.block_checksums.len(), 2);
        assert_eq!(source_info.global_block_offset, 10);

        // Global indices should be sequential starting from offset
        assert_eq!(source_info.block_checksums[0].global_index, 10);
        assert_eq!(source_info.block_checksums[1].global_index, 11);
    }

    #[test]
    fn test_hash_empty_file() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("empty.dat");

        // Create empty file
        File::create(&file_path).unwrap();

        let mut source_info = SourceFileInfo::new(file_path.clone(), 0, 0);
        let reporter = SilentCreateReporter;

        hash_source_file(&mut source_info, 1024, 0, &reporter).unwrap();

        // Empty file should have 0 blocks
        assert_eq!(source_info.block_count, 0);
        assert_eq!(source_info.block_checksums.len(), 0);

        // But should still have valid file hash and ID
        assert_ne!(source_info.hash, Md5Hash::new([0u8; 16]));
        assert_ne!(source_info.file_id, FileId::new([0u8; 16]));
    }

    #[test]
    fn test_hash_multiple_files_parallel() {
        let temp = tempdir().unwrap();

        // Create multiple test files
        let file1 = temp.path().join("file1.dat");
        let file2 = temp.path().join("file2.dat");
        let file3 = temp.path().join("file3.dat");

        std::fs::write(&file1, vec![0x11; 2048]).unwrap();
        std::fs::write(&file2, vec![0x22; 3072]).unwrap();
        std::fs::write(&file3, vec![0x33; 1024]).unwrap();

        let mut source_files = vec![
            SourceFileInfo::new(file1, 2048, 0),
            SourceFileInfo::new(file2, 3072, 1),
            SourceFileInfo::new(file3, 1024, 2),
        ];

        let reporter = SilentCreateReporter;
        hash_all_source_files(&mut source_files, 1024, &reporter, true).unwrap();

        // File 1: 2048 / 1024 = 2 blocks, offset 0
        assert_eq!(source_files[0].block_count, 2);
        assert_eq!(source_files[0].global_block_offset, 0);

        // File 2: 3072 / 1024 = 3 blocks, offset 2
        assert_eq!(source_files[1].block_count, 3);
        assert_eq!(source_files[1].global_block_offset, 2);

        // File 3: 1024 / 1024 = 1 block, offset 5
        assert_eq!(source_files[2].block_count, 1);
        assert_eq!(source_files[2].global_block_offset, 5);

        // All files should have hashes computed
        for source_file in &source_files {
            assert_ne!(source_file.hash, Md5Hash::new([0u8; 16]));
            assert_ne!(source_file.file_id, FileId::new([0u8; 16]));
        }
    }
}
