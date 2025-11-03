//! Enhanced verification engine using global block table
//!
//! This module provides comprehensive PAR2 verification using a global block table
//! approach similar to par2cmdline. It verifies the entire recovery set holistically
//! rather than individual files in isolation.

use super::global_table::{GlobalBlockTable, GlobalBlockTableBuilder};
use super::types::{
    BlockVerificationResult, FileStatus, FileVerificationResult, VerificationResults,
};
use super::utils::extract_file_name;

use crate::domain::{Crc32Value, FileId, Md5Hash};
use crate::packets::FileDescriptionPacket;
use crate::reporters::VerificationReporter;
use rayon::prelude::*;
use rustc_hash::FxHashMap as HashMap;
use std::path::Path;
use std::sync::Mutex;

/// Enhanced verification engine with global block table
///
/// This engine performs PAR2 verification using a global block table approach.
/// It builds a complete index of all blocks in the recovery set and then
/// scans available files to determine which blocks are present or missing.
pub struct GlobalVerificationEngine {
    /// Global block table for fast lookup
    block_table: GlobalBlockTable,
    /// File descriptions for all files in the recovery set
    file_descriptions: HashMap<FileId, FileDescriptionPacket>,
    /// Base directory for file operations
    base_dir: std::path::PathBuf,
}

/// Result of verifying a single file using global block table
#[derive(Debug, Clone)]
pub struct GlobalFileVerificationResult {
    /// Basic file verification result
    pub file_result: FileVerificationResult,
    /// Block verification results
    pub block_results: Vec<BlockVerificationResult>,
    /// Blocks found available (including from other files)
    pub available_blocks: Vec<u32>,
    /// Blocks that are definitely missing/corrupted
    pub damaged_blocks: Vec<u32>,
    /// Alternative sources for damaged blocks
    pub alternative_sources: HashMap<u32, Vec<(FileId, u32)>>,
}

impl GlobalVerificationEngine {
    /// Create a new verification engine from packets
    pub fn from_packets(
        packets: &[crate::Packet],
        base_dir: impl AsRef<Path>,
    ) -> Result<Self, String> {
        // Extract packet information
        let block_size = crate::packets::processing::extract_main_packet(packets)
            .map(|m| m.slice_size)
            .ok_or("No main packet found")?;

        let file_descriptions = crate::packets::processing::extract_file_descriptions(packets);
        let slice_checksums = crate::packets::processing::extract_slice_checksums(packets);

        // Build global block table
        let mut builder = GlobalBlockTableBuilder::new(block_size);

        for file_desc in &file_descriptions {
            if let Some(checksums) = slice_checksums.get(&file_desc.file_id) {
                builder.add_file_blocks(file_desc.file_id, checksums);
            }
        }

        let block_table = builder.build();

        // Create file description lookup
        let file_lookup = file_descriptions
            .into_iter()
            .map(|desc| (desc.file_id, desc.clone()))
            .collect();

        Ok(Self {
            block_table,
            file_descriptions: file_lookup,
            base_dir: base_dir.as_ref().to_path_buf(),
        })
    }

    /// Get the global block table
    pub fn block_table(&self) -> &GlobalBlockTable {
        &self.block_table
    }

    /// Verify the entire recovery set using the global block table
    ///
    /// This performs comprehensive verification by:
    /// 1. Scanning all available files and building a map of available blocks
    /// 2. Comparing against the global block table to determine what's missing
    /// 3. Computing file-level status based on block availability
    pub fn verify_recovery_set<R: VerificationReporter>(
        &self,
        reporter: &R,
    ) -> VerificationResults {
        // Note: report_verification_start and report_files_found should be called by the caller

        // Step 1: Scan all available files to build availability map
        let available_blocks = self.scan_available_blocks(reporter);

        // Step 2: Create aggregate results (individual file reporting already done in scan_available_blocks)
        let file_results = self.create_file_results(&available_blocks);
        let block_results = self.create_block_verification_results(&available_blocks);

        self.aggregate_results(file_results, block_results)
    }

    /// Scan all available files and build a global map of which blocks exist where
    /// This is the core of the global block table approach - we scan every file
    /// and index every block we find by its checksum, regardless of filename
    fn scan_available_blocks<R: VerificationReporter>(
        &self,
        reporter: &R,
    ) -> HashMap<(Md5Hash, Crc32Value), Vec<(FileId, u32)>> {
        // Wrap reporter in Mutex for thread-safe output (like par2cmdline-turbo's output_lock)
        let reporter_lock = Mutex::new(reporter);

        // Calculate total size of all files for progress reporting
        let total_size: u64 = self
            .file_descriptions
            .values()
            .filter(|desc| {
                let file_name = extract_file_name(desc);
                let file_path = self.base_dir.join(&file_name);
                file_path.exists()
            })
            .map(|desc| desc.file_length)
            .sum();

        // Collect files to scan
        let files_to_scan: Vec<_> = self
            .file_descriptions
            .values()
            .filter(|desc| {
                let file_name = extract_file_name(desc);
                let file_path = self.base_dir.join(&file_name);
                file_path.exists()
            })
            .collect();

        // Scan files in parallel and collect individual results
        let file_results: Vec<_> = files_to_scan
            .par_iter()
            .map(|file_desc| {
                let file_name = extract_file_name(file_desc);
                let file_path = self.base_dir.join(&file_name);

                // Lock reporter for output
                {
                    let reporter = reporter_lock.lock().unwrap();
                    reporter.report_verifying_file(&file_name);
                }

                // Scan this file and get its local block map
                let local_block_map = self.scan_single_file(&file_path);

                // Calculate status for this file
                let total_blocks = self.calculate_total_blocks(file_desc.file_length);
                let mut blocks_available = 0;
                let mut damaged_blocks = Vec::new();
                let file_blocks = self.block_table.get_file_blocks(file_desc.file_id);

                for block_num in 0..total_blocks {
                    if let Some(expected_block) = file_blocks.get(block_num) {
                        let checksum_key = (
                            expected_block.checksums.md5_hash,
                            expected_block.checksums.crc32,
                        );
                        if local_block_map.contains_key(&checksum_key) {
                            blocks_available += 1;
                        } else {
                            damaged_blocks.push(block_num as u32);
                        }
                    }
                }

                // Determine status
                let status = if blocks_available == total_blocks {
                    FileStatus::Present
                } else if blocks_available == 0 {
                    FileStatus::Missing
                } else {
                    FileStatus::Corrupted
                };

                // Lock reporter for status output
                {
                    let reporter = reporter_lock.lock().unwrap();
                    match status {
                        FileStatus::Present => {
                            reporter.report_file_status(&file_name, status);
                        }
                        FileStatus::Missing => {
                            reporter.report_file_status(&file_name, status);
                        }
                        FileStatus::Corrupted => {
                            reporter.report_damaged_blocks(
                                &file_name,
                                &damaged_blocks,
                                blocks_available,
                                total_blocks,
                            );
                        }
                        FileStatus::Renamed => {
                            reporter.report_file_status(&file_name, status);
                        }
                    }
                }

                (local_block_map, file_desc.file_length)
            })
            .collect();

        // Merge all local maps into global map and track progress
        let mut global_block_map = HashMap::default();
        let mut bytes_scanned = 0u64;
        let mut last_reported_fraction = 0u32;

        for (local_map, file_size) in file_results {
            // Merge local map into global
            for (key, entries) in local_map {
                global_block_map
                    .entry(key)
                    .or_insert_with(Vec::new)
                    .extend(entries);
            }

            // Update progress
            bytes_scanned += file_size;
            if total_size > 0 {
                let new_fraction = ((bytes_scanned * 1000) / total_size) as u32;
                if new_fraction != last_reported_fraction {
                    reporter.report_scanning_progress(new_fraction as f64 / 1000.0);
                    last_reported_fraction = new_fraction;
                }
            }
        }

        global_block_map
    }

    /// Scan a single file and return its local block map
    fn scan_single_file(
        &self,
        file_path: &Path,
    ) -> HashMap<(Md5Hash, Crc32Value), Vec<(FileId, u32)>> {
        use crate::checksum::rolling_crc::RollingCrcTable;
        use crate::checksum::{compute_block_checksums_padded, compute_crc32};
        use std::fs::File;
        use std::io::Read;

        let mut local_block_map = HashMap::default();

        let mut file = match File::open(file_path) {
            Ok(f) => f,
            Err(_) => return local_block_map,
        };

        let block_size = self.block_table.block_size() as usize;
        let buffer_size = block_size * 2; // 2-block buffer like par2cmdline
        let mut buffer = vec![0u8; buffer_size];

        // Create rolling CRC table for efficient scanning
        let rolling_table = RollingCrcTable::new(block_size);

        // Initial fill of the buffer
        let mut bytes_in_buffer = match file.read(&mut buffer) {
            Ok(n) => n,
            Err(_) => return local_block_map,
        };

        loop {
            if bytes_in_buffer == 0 {
                break; // EOF
            }

            // Scan byte-by-byte within the current 2-block buffer using rolling CRC
            let mut scan_pos = 0;

            // Initialize rolling CRC with first block if possible
            let mut rolling_crc: Option<Crc32Value> = if bytes_in_buffer >= block_size {
                Some(compute_crc32(&buffer[0..block_size]))
            } else {
                None
            };

            while scan_pos + block_size <= bytes_in_buffer {
                let block_data = &buffer[scan_pos..scan_pos + block_size];

                // Use rolling CRC if we have it, otherwise compute fresh
                let crc32 = if let Some(crc) = rolling_crc {
                    crc
                } else {
                    compute_crc32(block_data)
                };

                // Fast CRC32 lookup in global table
                if let Some(candidates) = self.block_table.find_by_crc32(crc32) {
                    // Only compute MD5 if we have a CRC32 match
                    let md5_hash = crate::checksum::compute_md5_only(block_data);

                    // Check MD5 hash against all candidates with this CRC32
                    for candidate in candidates {
                        if candidate.checksums.md5_hash == md5_hash {
                            // Found valid block - record it in local map
                            for duplicate in candidate.iter_duplicates() {
                                local_block_map.entry((md5_hash, crc32)).or_default().push((
                                    duplicate.position.file_id,
                                    duplicate.position.block_number,
                                ));
                            }
                            break;
                        }
                    }
                }

                // Advance scan position by 1 byte
                scan_pos += 1;

                // Update rolling CRC for next iteration
                if scan_pos + block_size <= bytes_in_buffer {
                    if let Some(crc) = rolling_crc {
                        let byte_out = buffer[scan_pos - 1];
                        let byte_in = buffer[scan_pos + block_size - 1];
                        rolling_crc = Some(Crc32Value::new(rolling_table.slide(
                            crc.as_u32(),
                            byte_in,
                            byte_out,
                        )));
                    }
                }
            }

            // Handle partial block at end
            let remainder_start = scan_pos;
            let remainder_size = bytes_in_buffer.saturating_sub(remainder_start);

            if remainder_size > 0 && remainder_size < block_size {
                let partial_data = &buffer[remainder_start..bytes_in_buffer];
                let (md5_hash, crc32) = compute_block_checksums_padded(partial_data, block_size);

                if let Some(candidates) = self.block_table.find_by_crc32(crc32) {
                    for candidate in candidates {
                        if candidate.checksums.md5_hash == md5_hash {
                            for duplicate in candidate.iter_duplicates() {
                                local_block_map.entry((md5_hash, crc32)).or_default().push((
                                    duplicate.position.file_id,
                                    duplicate.position.block_number,
                                ));
                            }
                            break;
                        }
                    }
                }

                if scan_pos == 0 {
                    break;
                }
            }

            // Slide window forward
            if bytes_in_buffer >= block_size {
                buffer.copy_within(block_size..bytes_in_buffer, 0);
                let bytes_to_keep = bytes_in_buffer - block_size;

                let bytes_read = match file.read(&mut buffer[bytes_to_keep..]) {
                    Ok(n) => n,
                    Err(_) => break,
                };

                bytes_in_buffer = bytes_to_keep + bytes_read;
            } else {
                break;
            }
        }

        local_block_map
    }

    /// Create file results based on available blocks (reporting already done)
    fn create_file_results(
        &self,
        available_blocks: &HashMap<(Md5Hash, Crc32Value), Vec<(FileId, u32)>>,
    ) -> Vec<FileVerificationResult> {
        let mut file_results = Vec::new();

        for file_desc in self.file_descriptions.values() {
            let file_name = extract_file_name(file_desc);
            let total_blocks = self.calculate_total_blocks(file_desc.file_length);

            // Count available blocks for this file by checking if each block's
            // checksum is available in any location
            let mut blocks_available = 0;
            let mut damaged_blocks = Vec::new();
            let file_blocks = self.block_table.get_file_blocks(file_desc.file_id);

            for block_num in 0..total_blocks {
                let mut found = false;

                // Look for this block's checksum in our available blocks map
                if let Some(expected_block) = file_blocks.get(block_num) {
                    let checksum_key = (
                        expected_block.checksums.md5_hash,
                        expected_block.checksums.crc32,
                    );
                    if let Some(_locations) = available_blocks.get(&checksum_key) {
                        // Block data is available somewhere (could be used for repair)
                        found = true;
                    }
                }

                if found {
                    blocks_available += 1;
                } else {
                    damaged_blocks.push(block_num as u32);
                }
            }

            // Determine file status
            let status = if blocks_available == total_blocks {
                FileStatus::Present
            } else if blocks_available == 0 {
                FileStatus::Missing
            } else {
                FileStatus::Corrupted
            };

            // Just create the result record (reporting already done inline)

            file_results.push(FileVerificationResult {
                file_name,
                file_id: file_desc.file_id,
                status,
                blocks_available,
                total_blocks,
                damaged_blocks,
            });
        }

        file_results
    }

    /// Create block verification results
    fn create_block_verification_results(
        &self,
        available_blocks: &HashMap<(Md5Hash, Crc32Value), Vec<(FileId, u32)>>,
    ) -> Vec<BlockVerificationResult> {
        let mut block_results = Vec::new();

        // Iterate through all blocks in the global table
        for entry in self.block_table.iter_blocks() {
            let checksum_key = (entry.checksums.md5_hash, entry.checksums.crc32);
            let is_valid = available_blocks.contains_key(&checksum_key);

            block_results.push(BlockVerificationResult {
                block_number: entry.position.block_number,
                file_id: entry.position.file_id,
                is_valid,
                expected_hash: Some(entry.checksums.md5_hash),
                expected_crc: Some(entry.checksums.crc32),
            });
        }

        block_results
    }

    /// Calculate total blocks for a file
    fn calculate_total_blocks(&self, file_length: u64) -> usize {
        let block_size = self.block_table.block_size();
        if block_size > 0 {
            file_length.div_ceil(block_size) as usize
        } else {
            0
        }
    }

    /// Aggregate results into final verification results
    fn aggregate_results(
        &self,
        file_results: Vec<FileVerificationResult>,
        block_results: Vec<BlockVerificationResult>,
    ) -> VerificationResults {
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

        // Count recovery blocks (would need to be passed in or calculated)
        let recovery_blocks_available = 0; // TODO: Extract from recovery packets

        VerificationResults {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_file_desc(file_id: FileId, length: u64) -> FileDescriptionPacket {
        use crate::domain::RecoverySetId;
        FileDescriptionPacket {
            length: 120 + 8, // minimal packet size + filename
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([1; 16]),
            packet_type: *b"PAR 2.0\0FileDesc",
            file_id,
            md5_hash: Md5Hash::new([0; 16]),
            md5_16k: Md5Hash::new([0; 16]),
            file_length: length,
            file_name: b"test.txt".to_vec(),
        }
    }

    #[test]
    fn test_global_verification_engine_creation() {
        // Create minimal packet set
        let main_packet = crate::packets::MainPacket {
            length: 92,
            md5: Md5Hash::new([0; 16]),
            set_id: crate::domain::RecoverySetId::new([1; 16]),
            slice_size: 1024,
            file_count: 1,
            file_ids: vec![FileId::new([2; 16])],
            non_recovery_file_ids: vec![],
        };

        let file_desc = create_test_file_desc(FileId::new([2; 16]), 1024);

        let packets = vec![
            crate::Packet::Main(main_packet),
            crate::Packet::FileDescription(file_desc),
        ];

        let engine = GlobalVerificationEngine::from_packets(&packets, ".");
        assert!(engine.is_ok());

        let engine = engine.unwrap();
        assert_eq!(engine.block_table().block_size(), 1024);
    }

    #[test]
    fn test_missing_file_verification() {
        let main_packet = crate::packets::MainPacket {
            length: 92,
            md5: Md5Hash::new([0; 16]),
            set_id: crate::domain::RecoverySetId::new([1; 16]),
            slice_size: 1024,
            file_count: 1,
            file_ids: vec![FileId::new([2; 16])],
            non_recovery_file_ids: vec![],
        };

        let file_desc = create_test_file_desc(FileId::new([2; 16]), 1024);

        let packets = vec![
            crate::Packet::Main(main_packet),
            crate::Packet::FileDescription(file_desc.clone()),
        ];

        let temp_dir = tempfile::tempdir().unwrap();
        let engine = GlobalVerificationEngine::from_packets(&packets, temp_dir.path()).unwrap();
        let reporter = crate::reporters::ConsoleVerificationReporter::new();
        let results = engine.verify_recovery_set(&reporter);

        // Since the file doesn't exist, it should be reported as missing
        assert_eq!(results.missing_file_count, 1);
        assert_eq!(results.present_file_count, 0);
        assert_eq!(results.total_block_count, 1); // 1024 bytes = 1 block of 1024
    }
}
