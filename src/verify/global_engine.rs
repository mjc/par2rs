//! Enhanced verification engine using global block table
//!
//! This module provides comprehensive PAR2 verification using a global block table
//! approach similar to par2cmdline. It verifies the entire recovery set holistically
//! rather than individual files in isolation.

use super::global_table::{GlobalBlockTable, GlobalBlockTableBuilder};
use super::types::{
    BlockCount, BlockNumber, BlockVerificationResult, FileSize, FileStatus, FileVerificationResult,
    VerificationResults,
};
use super::utils::extract_file_name;

use crate::domain::{Crc32Value, FileId, Md5Hash};
use crate::packets::FileDescriptionPacket;
use crate::reporters::VerificationReporter;
use rayon::prelude::*;
use rustc_hash::FxHashMap as HashMap;
use smallvec::SmallVec;
use std::path::Path;
use std::sync::Mutex;

/// Local block map type: maps (MD5, CRC32) -> list of (FileId, block_number)
/// Uses SmallVec to avoid heap allocation for 1-2 entries (common case)
type LocalBlockMap = HashMap<(Md5Hash, Crc32Value), SmallVec<[(FileId, u32); 2]>>;

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
        parallel: bool,
    ) -> VerificationResults {
        // Note: report_verification_start and report_files_found should be called by the caller

        // Step 1: Scan all available files to build availability map
        let available_blocks = self.scan_available_blocks(reporter, parallel);

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
        parallel: bool,
    ) -> HashMap<(Md5Hash, Crc32Value), Vec<(FileId, u32)>> {
        // Wrap reporter in Mutex for thread-safe output (like par2cmdline-turbo's output_lock)
        let reporter_lock = Mutex::new(reporter);

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

        // Scan files in parallel or sequentially based on config
        let file_results: Vec<_> = if parallel {
            files_to_scan
                .par_iter()
                .map(|file_desc| self.process_single_file(file_desc, &reporter_lock))
                .collect()
        } else {
            files_to_scan
                .iter()
                .map(|file_desc| self.process_single_file(file_desc, &reporter_lock))
                .collect()
        };

        // Merge all local maps into global map
        let mut global_block_map = HashMap::default();

        for (local_map, _file_size) in file_results {
            // Merge local map into global
            for (key, entries) in local_map {
                global_block_map
                    .entry(key)
                    .or_insert_with(Vec::new)
                    .extend(entries);
            }
        }

        global_block_map
    }

    /// Process a single file: scan blocks and report status
    fn process_single_file<R: VerificationReporter>(
        &self,
        file_desc: &FileDescriptionPacket,
        reporter_lock: &Mutex<&R>,
    ) -> (LocalBlockMap, FileSize) {
        use crate::verify::types::{BlockCount, BlockNumber, FileSize};

        let file_name = extract_file_name(file_desc);
        let file_path = self.base_dir.join(&file_name);
        let file_size = FileSize::new(file_desc.file_length);

        // Lock reporter to start file
        {
            let reporter = reporter_lock.lock().unwrap();
            reporter.report_verifying_file(&file_name);
        }

        // Scan this file and get its local block map, reporting progress
        let local_block_map =
            self.scan_single_file_with_progress(&file_path, file_size, reporter_lock);

        // Calculate status for this file
        let total_blocks = self.calculate_total_blocks(file_size);
        let mut blocks_available = BlockCount::zero();
        let mut damaged_blocks = Vec::new();
        let file_blocks = self.block_table.get_file_blocks(file_desc.file_id);

        for block_num in 0..total_blocks.as_usize() {
            let block_number = BlockNumber::new(block_num);
            if let Some(expected_block) = file_blocks.get(block_num) {
                let checksum_key = (
                    expected_block.checksums.md5_hash,
                    expected_block.checksums.crc32,
                );
                if local_block_map.contains_key(&checksum_key) {
                    blocks_available.increment();
                } else {
                    damaged_blocks.push(block_number.as_u32());
                }
            }
        }

        // Determine status
        let status = if blocks_available == total_blocks {
            FileStatus::Present
        } else if blocks_available == BlockCount::zero() {
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
                        blocks_available.as_usize(),
                        total_blocks.as_usize(),
                    );
                }
                FileStatus::Renamed => {
                    reporter.report_file_status(&file_name, status);
                }
            }
        }

        (local_block_map, file_size)
    }

    /// Scan a single file and return its local block map with progress reporting
    fn scan_single_file_with_progress<R: VerificationReporter>(
        &self,
        file_path: &Path,
        file_size: FileSize,
        reporter_lock: &Mutex<&R>,
    ) -> LocalBlockMap {
        use crate::checksum::compute_crc32;
        use crate::checksum::rolling_crc::RollingCrcTable;
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::BlockSize;
        use std::fs::File;
        use std::io::Read;

        let mut local_block_map = HashMap::default();

        let mut file = match File::open(file_path) {
            Ok(f) => f,
            Err(_) => return local_block_map,
        };

        let block_size = BlockSize::new(self.block_table.block_size() as usize);
        let buffer_capacity = block_size.doubled();
        let mut buffer = vec![0u8; buffer_capacity];

        // Create rolling CRC table for efficient scanning
        let rolling_table = RollingCrcTable::new(block_size.as_usize());

        // Initial fill of the buffer
        let bytes_read = match file.read(&mut buffer) {
            Ok(n) => n,
            Err(_) => return local_block_map,
        };

        // Initialize scanner state
        let mut state = ScannerState::new(bytes_read);

        loop {
            if state.at_eof() {
                break; // EOF
            }

            // OPTIMIZATION: At file start, try aligned blocks first (par2cmdline-turbo approach)
            // Most PAR2 protected files have blocks perfectly aligned
            if state.should_try_aligned_blocks(block_size) {
                self.try_aligned_blocks(
                    &buffer,
                    &mut local_block_map,
                    block_size,
                    state.bytes_in_buffer,
                );
            }

            // Initialize rolling CRC with first block if possible
            let initial_crc = if state.bytes_in_buffer.has_at_least(block_size) {
                Some(compute_crc32(&buffer[0..block_size.as_usize()]))
            } else {
                None
            };
            state.set_rolling_crc(initial_crc);

            // Scan byte-by-byte within the current 2-block buffer using rolling CRC
            while state.can_fit_block(block_size) {
                let start = state.scan_pos.as_usize();
                let end = start + block_size.as_usize();
                let block_data = &buffer[start..end];

                // Use rolling CRC if we have it, otherwise compute fresh
                let crc32 = state
                    .rolling_crc
                    .unwrap_or_else(|| compute_crc32(block_data));

                // Fast CRC32 lookup in global table - only compute MD5 if CRC matches
                let found_match = if self.block_table.find_by_crc32(crc32).is_some() {
                    let md5_hash = crate::checksum::compute_md5_only(block_data);
                    self.insert_matching_blocks(md5_hash, crc32, &mut local_block_map)
                } else {
                    false
                };

                if found_match {
                    // Skip ahead by full block (blocks don't overlap in PAR2)
                    state.skip_block(block_size);

                    // Recompute rolling CRC for new position if still in buffer
                    let new_crc = if state.can_fit_block(block_size) {
                        Some(compute_crc32(
                            &buffer[state.scan_pos.as_usize()
                                ..state.scan_pos.as_usize() + block_size.as_usize()],
                        ))
                    } else {
                        None
                    };
                    state.set_rolling_crc(new_crc);
                    continue;
                }

                // No match - advance by 1 byte and roll
                state.advance_one_byte();

                // Update rolling CRC for next iteration
                if state.can_fit_block(block_size) {
                    if let Some(crc) = state.rolling_crc {
                        let byte_out = buffer[state.scan_pos.as_usize() - 1];
                        let byte_in = buffer[state.scan_pos.as_usize() + block_size.as_usize() - 1];
                        let new_crc =
                            Crc32Value::new(rolling_table.slide(crc.as_u32(), byte_in, byte_out));
                        state.update_rolling_crc(new_crc);
                    }
                }
            }

            // Handle partial block at end
            let remainder_size = state.remainder_size(block_size);
            if remainder_size > 0 {
                let start = state.scan_pos.as_usize();
                let partial_data = &buffer[start..state.bytes_in_buffer.as_usize()];

                self.try_match_and_insert_partial_block(
                    partial_data,
                    block_size.as_usize(),
                    &mut local_block_map,
                );

                if state.is_remainder_at_start() {
                    break;
                }
            }

            // Slide window forward
            if state.can_slide_window(block_size) {
                let block_sz = block_size.as_usize();
                let buf_sz = state.bytes_in_buffer.as_usize();
                buffer.copy_within(block_sz..buf_sz, 0);
                let bytes_to_keep = buf_sz - block_sz;

                let bytes_read = match file.read(&mut buffer[bytes_to_keep..]) {
                    Ok(n) => n,
                    Err(_) => break,
                };

                let new_buffer_size =
                    crate::verify::types::BufferSize::new(bytes_to_keep + bytes_read);
                state.slide_window(block_size, new_buffer_size);

                // Update progress after sliding
                let fraction = state.bytes_processed.progress_fraction(file_size.as_u64());
                if let Ok(reporter) = reporter_lock.lock() {
                    reporter.report_scanning_progress(fraction);
                }
            } else {
                break;
            }
        }

        // Mark file as 100% scanned
        if let Ok(reporter) = reporter_lock.lock() {
            reporter.report_scanning_progress(1.0);
        }

        local_block_map
    }

    /// Insert all matching blocks from the global table into the local block map
    /// Returns true if at least one match was found
    fn insert_matching_blocks(
        &self,
        md5_hash: Md5Hash,
        crc32: Crc32Value,
        local_block_map: &mut LocalBlockMap,
    ) -> bool {
        if let Some(candidates) = self.block_table.find_by_crc32(crc32) {
            let mut found_any = false;

            for candidate in candidates {
                if candidate.checksums.md5_hash == md5_hash {
                    for duplicate in candidate.iter_duplicates() {
                        local_block_map
                            .entry((md5_hash, crc32))
                            .or_default()
                            .push((duplicate.position.file_id, duplicate.position.block_number));
                    }
                    found_any = true;
                    break; // Only need to match once per unique checksum
                }
            }

            found_any
        } else {
            false
        }
    }

    /// Try to match a block of data against the global block table
    /// If found, insert all matching blocks into the local map
    /// Returns true if the block matched
    fn try_match_and_insert_block(
        &self,
        block_data: &[u8],
        local_block_map: &mut LocalBlockMap,
    ) -> bool {
        use crate::checksum::{compute_crc32, compute_md5_only};

        let crc32 = compute_crc32(block_data);

        // Fast CRC32 lookup - only compute expensive MD5 if CRC matches
        if self.block_table.find_by_crc32(crc32).is_some() {
            let md5_hash = compute_md5_only(block_data);
            self.insert_matching_blocks(md5_hash, crc32, local_block_map)
        } else {
            false
        }
    }

    /// Try to match a partial block (with padding) against the global block table
    /// Returns true if the block matched
    fn try_match_and_insert_partial_block(
        &self,
        partial_data: &[u8],
        block_size: usize,
        local_block_map: &mut LocalBlockMap,
    ) -> bool {
        use crate::checksum::compute_block_checksums_padded;

        let (md5_hash, crc32) = compute_block_checksums_padded(partial_data, block_size);
        self.insert_matching_blocks(md5_hash, crc32, local_block_map)
    }

    /// Try to find blocks at aligned positions (optimization for well-formed PAR2 files)
    fn try_aligned_blocks(
        &self,
        buffer: &[u8],
        local_block_map: &mut LocalBlockMap,
        block_size: crate::verify::types::BlockSize,
        bytes_in_buffer: crate::verify::types::BufferSize,
    ) {
        for block_idx in 0..2 {
            let start = block_idx * block_size.as_usize();
            let end = start + block_size.as_usize();

            if end <= bytes_in_buffer.as_usize() {
                let block_data = &buffer[start..end];
                self.try_match_and_insert_block(block_data, local_block_map);
            }
        }
    }

    /// Create file results based on available blocks (reporting already done)
    fn create_file_results(
        &self,
        available_blocks: &HashMap<(Md5Hash, Crc32Value), Vec<(FileId, u32)>>,
    ) -> Vec<FileVerificationResult> {
        let mut file_results = Vec::new();

        for file_desc in self.file_descriptions.values() {
            let file_name = extract_file_name(file_desc);
            let file_size = FileSize::new(file_desc.file_length);
            let total_blocks = self.calculate_total_blocks(file_size);

            // Count available blocks for this file by checking if each block's
            // checksum is available in any location
            let mut blocks_available = BlockCount::zero();
            let mut damaged_blocks = Vec::new();
            let file_blocks = self.block_table.get_file_blocks(file_desc.file_id);

            for block_num in 0..total_blocks.as_usize() {
                let block_number = BlockNumber::new(block_num);

                // Look for this block's checksum in our available blocks map
                let is_available = file_blocks
                    .get(block_num)
                    .and_then(|expected_block| {
                        let checksum_key = (
                            expected_block.checksums.md5_hash,
                            expected_block.checksums.crc32,
                        );
                        available_blocks.get(&checksum_key)
                    })
                    .is_some();

                if is_available {
                    blocks_available.increment();
                } else {
                    damaged_blocks.push(block_number.as_u32());
                }
            }

            // Determine file status
            let status = if blocks_available == total_blocks {
                FileStatus::Present
            } else if blocks_available == BlockCount::zero() {
                FileStatus::Missing
            } else {
                FileStatus::Corrupted
            };

            // Just create the result record (reporting already done inline)

            file_results.push(FileVerificationResult {
                file_name,
                file_id: file_desc.file_id,
                status,
                blocks_available: blocks_available.as_usize(),
                total_blocks: total_blocks.as_usize(),
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
    fn calculate_total_blocks(&self, file_length: FileSize) -> BlockCount {
        use crate::verify::types::BlockSize;

        let block_size = BlockSize::new(self.block_table.block_size() as usize);
        file_length.total_blocks(block_size)
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
        let results = engine.verify_recovery_set(&reporter, true); // parallel=true for tests

        // Since the file doesn't exist, it should be reported as missing
        assert_eq!(results.missing_file_count, 1);
        assert_eq!(results.present_file_count, 0);
        assert_eq!(results.total_block_count, 1); // 1024 bytes = 1 block of 1024
    }

    #[test]
    fn test_insert_matching_blocks() {
        use crate::verify::global_table::GlobalBlockTableBuilder;

        // Create a simple block table with one known block
        let mut builder = GlobalBlockTableBuilder::new(1024);
        let file_id = FileId::new([1; 16]);
        let checksums = vec![(Md5Hash::new([0xAA; 16]), Crc32Value::new(0x12345678))];
        builder.add_file_blocks(file_id, &checksums);
        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        let mut local_map = HashMap::default();

        // Test matching block
        let found = engine.insert_matching_blocks(
            Md5Hash::new([0xAA; 16]),
            Crc32Value::new(0x12345678),
            &mut local_map,
        );
        assert!(found, "Should find matching block");
        assert_eq!(local_map.len(), 1, "Should have one entry in local map");

        // Test non-matching MD5
        let mut local_map2 = HashMap::default();
        let found = engine.insert_matching_blocks(
            Md5Hash::new([0xBB; 16]),
            Crc32Value::new(0x12345678),
            &mut local_map2,
        );
        assert!(!found, "Should not find block with wrong MD5");
        assert_eq!(local_map2.len(), 0, "Should have no entries");

        // Test non-matching CRC32
        let mut local_map3 = HashMap::default();
        let found = engine.insert_matching_blocks(
            Md5Hash::new([0xAA; 16]),
            Crc32Value::new(0x99999999),
            &mut local_map3,
        );
        assert!(!found, "Should not find block with wrong CRC32");
        assert_eq!(local_map3.len(), 0, "Should have no entries");
    }

    #[test]
    fn test_try_match_and_insert_block() {
        use crate::verify::global_table::GlobalBlockTableBuilder;

        // Create a block of test data
        let block_data = vec![0x42; 1024];
        let expected_crc32 = crate::checksum::compute_crc32(&block_data);
        let expected_md5 = crate::checksum::compute_md5_only(&block_data);

        // Build a block table with this block
        let mut builder = GlobalBlockTableBuilder::new(1024);
        let file_id = FileId::new([1; 16]);
        let checksums = vec![(expected_md5, expected_crc32)];
        builder.add_file_blocks(file_id, &checksums);
        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        let mut local_map = HashMap::default();

        // Test matching
        let found = engine.try_match_and_insert_block(&block_data, &mut local_map);
        assert!(found, "Should find matching block");
        assert_eq!(local_map.len(), 1, "Should have one entry");

        // Test non-matching data
        let wrong_data = vec![0x99; 1024];
        let mut local_map2 = HashMap::default();
        let found = engine.try_match_and_insert_block(&wrong_data, &mut local_map2);
        assert!(!found, "Should not find non-matching block");
        assert_eq!(local_map2.len(), 0, "Should have no entries");
    }

    #[test]
    fn test_try_match_and_insert_partial_block() {
        use crate::verify::global_table::GlobalBlockTableBuilder;

        // Create a partial block (500 bytes of a 1024 byte block)
        let partial_data = vec![0x42; 500];

        // Compute what the checksums should be with padding
        let (expected_md5, expected_crc32) =
            crate::checksum::compute_block_checksums_padded(&partial_data, 1024);

        // Build a block table with this block
        let mut builder = GlobalBlockTableBuilder::new(1024);
        let file_id = FileId::new([1; 16]);
        let checksums = vec![(expected_md5, expected_crc32)];
        builder.add_file_blocks(file_id, &checksums);
        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        let mut local_map = HashMap::default();

        // Test matching partial block
        let found = engine.try_match_and_insert_partial_block(&partial_data, 1024, &mut local_map);
        assert!(found, "Should find matching partial block");
        assert_eq!(local_map.len(), 1, "Should have one entry");

        // Test non-matching partial data
        let wrong_data = vec![0x99; 500];
        let mut local_map2 = HashMap::default();
        let found = engine.try_match_and_insert_partial_block(&wrong_data, 1024, &mut local_map2);
        assert!(!found, "Should not find non-matching partial block");
        assert_eq!(local_map2.len(), 0, "Should have no entries");
    }

    #[test]
    fn test_block_matching_is_consistent() {
        // This test verifies that the three different code paths
        // (aligned, byte-by-byte, partial) all use the same logic

        use crate::verify::global_table::GlobalBlockTableBuilder;

        let block_data = vec![0x42; 1024];
        let expected_crc32 = crate::checksum::compute_crc32(&block_data);
        let expected_md5 = crate::checksum::compute_md5_only(&block_data);

        let mut builder = GlobalBlockTableBuilder::new(1024);
        let file_id = FileId::new([1; 16]);
        let checksums = vec![(expected_md5, expected_crc32)];
        builder.add_file_blocks(file_id, &checksums);
        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        // Test 1: Direct insertion
        let mut map1 = HashMap::default();
        let found1 = engine.insert_matching_blocks(expected_md5, expected_crc32, &mut map1);

        // Test 2: Via try_match_and_insert_block
        let mut map2 = HashMap::default();
        let found2 = engine.try_match_and_insert_block(&block_data, &mut map2);

        // Both should find the block
        assert_eq!(found1, found2, "Both methods should return same result");
        assert_eq!(map1.len(), map2.len(), "Both maps should have same size");

        // Verify the maps contain the same entries
        for (key, value1) in &map1 {
            let value2 = map2.get(key).expect("Key should exist in map2");
            assert_eq!(value1, value2, "Values should match for key {:?}", key);
        }
    }
}
