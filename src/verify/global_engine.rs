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

/// Result of attempting to match a block against the global table
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockMatchResult {
    /// Block matched and was inserted into the local map
    Matched,
    /// Block did not match any entry in the global table
    NotMatched,
}

impl BlockMatchResult {
    pub fn is_match(self) -> bool {
        matches!(self, BlockMatchResult::Matched)
    }

    pub fn from_bool(matched: bool) -> Self {
        if matched {
            BlockMatchResult::Matched
        } else {
            BlockMatchResult::NotMatched
        }
    }
}

/// Result of attempting to slide the buffer window
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferSlideResult {
    /// Successfully slid the window and read more data
    Success,
    /// Cannot slide - buffer doesn't have enough data
    CannotSlide,
}

/// Action to take after scanning a block position
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanAction {
    /// Found a match - skip ahead by a full block
    SkipBlock,
    /// No match - advance by one byte and continue rolling
    AdvanceOneByte,
}

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
        use crate::verify::types::FileSize;

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
        let (blocks_available, damaged_blocks) =
            self.count_file_blocks(file_desc.file_id, file_size, &local_block_map);

        // Determine status
        let status = Self::determine_file_status(blocks_available, total_blocks);

        // Report status
        Self::report_file_status(
            reporter_lock,
            &file_name,
            status,
            &damaged_blocks,
            blocks_available,
            total_blocks,
        );

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
        use crate::verify::types::{BlockSize, ScanBuffer};
        use std::fs::File;

        let mut local_block_map = HashMap::default();

        let mut file = match File::open(file_path) {
            Ok(f) => f,
            Err(_) => return local_block_map,
        };

        let block_size = BlockSize::new(self.block_table.block_size() as usize);
        let buffer_capacity = block_size.doubled();
        let mut buffer = ScanBuffer::with_capacity(buffer_capacity);

        // Create rolling CRC table for efficient scanning
        let rolling_table = RollingCrcTable::new(block_size.as_usize());

        // Initial fill of the buffer
        let bytes_read = match buffer.read_from(&mut file) {
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
                Some(compute_crc32(buffer.first_block(block_size)))
            } else {
                None
            };
            state.set_rolling_crc(initial_crc);

            // Scan byte-by-byte within the current 2-block buffer using rolling CRC
            while state.can_fit_block(block_size) {
                let action =
                    self.scan_block_position(&buffer, &state, block_size, &mut local_block_map);

                match action {
                    ScanAction::SkipBlock => {
                        // Skip ahead by full block (blocks don't overlap in PAR2)
                        state.skip_block(block_size);
                        // Recompute rolling CRC for new position if still in buffer
                        Self::update_rolling_crc_after_skip(
                            &rolling_table,
                            &buffer,
                            &mut state,
                            block_size,
                        );
                    }
                    ScanAction::AdvanceOneByte => {
                        // No match - advance by 1 byte and roll
                        state.advance_one_byte();
                        // Update rolling CRC for next iteration
                        Self::slide_rolling_crc_one_byte(
                            &rolling_table,
                            &buffer,
                            &mut state,
                            block_size,
                        );
                    }
                }
            }

            // Handle partial block at end
            let remainder_size = state.remainder_size(block_size);
            if remainder_size > 0 {
                let partial_data = buffer.slice_from(state.scan_pos, state.bytes_in_buffer);

                self.try_match_and_insert_partial_block(
                    partial_data,
                    block_size.as_usize(),
                    &mut local_block_map,
                );

                if state.is_remainder_at_start() {
                    break;
                }
            }

            // Slide window forward and read more data
            match Self::slide_buffer_window(&mut file, &mut buffer, &mut state, block_size) {
                Ok(BufferSlideResult::Success) => {
                    // Successfully slid window, report progress
                    Self::report_progress(reporter_lock, &state, file_size);
                }
                Ok(BufferSlideResult::CannotSlide) => break,
                Err(_) => break, // Read error
            }
        }

        // Mark file as 100% scanned
        Self::report_progress(reporter_lock, &state, file_size);

        local_block_map
    }

    /// Insert all matching blocks from the global table into the local block map
    /// Returns whether at least one match was found
    fn insert_matching_blocks(
        &self,
        md5_hash: Md5Hash,
        crc32: Crc32Value,
        local_block_map: &mut LocalBlockMap,
    ) -> BlockMatchResult {
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

            BlockMatchResult::from_bool(found_any)
        } else {
            BlockMatchResult::NotMatched
        }
    }

    /// Try to match a block of data against the global block table
    /// If found, insert all matching blocks into the local map
    fn try_match_and_insert_block(
        &self,
        block_data: &[u8],
        local_block_map: &mut LocalBlockMap,
    ) -> BlockMatchResult {
        use crate::checksum::{compute_crc32, compute_md5_only};

        let crc32 = compute_crc32(block_data);

        // Fast CRC32 lookup - only compute expensive MD5 if CRC matches
        if self.block_table.find_by_crc32(crc32).is_some() {
            let md5_hash = compute_md5_only(block_data);
            self.insert_matching_blocks(md5_hash, crc32, local_block_map)
        } else {
            BlockMatchResult::NotMatched
        }
    }

    /// Try to match a partial block (with padding) against the global block table
    fn try_match_and_insert_partial_block(
        &self,
        partial_data: &[u8],
        block_size: usize,
        local_block_map: &mut LocalBlockMap,
    ) -> BlockMatchResult {
        use crate::checksum::compute_block_checksums_padded;

        let (md5_hash, crc32) = compute_block_checksums_padded(partial_data, block_size);
        self.insert_matching_blocks(md5_hash, crc32, local_block_map)
    }

    /// Update rolling CRC after skipping forward by a full block
    fn update_rolling_crc_after_skip(
        rolling_table: &crate::checksum::rolling_crc::RollingCrcTable,
        buffer: &crate::verify::types::ScanBuffer,
        state: &mut crate::verify::scanner_state::ScannerState,
        block_size: crate::verify::types::BlockSize,
    ) {
        let new_crc = rolling_table
            .compute_crc_at_position(
                buffer.as_slice(),
                state.scan_pos.as_usize(),
                block_size.as_usize(),
                state.bytes_in_buffer.as_usize(),
            )
            .map(Crc32Value::new);
        state.set_rolling_crc(new_crc);
    }

    /// Slide rolling CRC forward by one byte using rolling window algorithm
    fn slide_rolling_crc_one_byte(
        rolling_table: &crate::checksum::rolling_crc::RollingCrcTable,
        buffer: &crate::verify::types::ScanBuffer,
        state: &mut crate::verify::scanner_state::ScannerState,
        block_size: crate::verify::types::BlockSize,
    ) {
        if let Some(crc) = state.rolling_crc {
            let new_crc = rolling_table
                .slide_crc_forward(
                    crc.as_u32(),
                    buffer.as_slice(),
                    state.scan_pos.as_usize(),
                    block_size.as_usize(),
                    state.bytes_in_buffer.as_usize(),
                )
                .map(Crc32Value::new);
            state.set_rolling_crc(new_crc);
        }
    }

    /// Slide the buffer window forward and read more data from the file
    fn slide_buffer_window<F: std::io::Read>(
        file: &mut F,
        buffer: &mut crate::verify::types::ScanBuffer,
        state: &mut crate::verify::scanner_state::ScannerState,
        block_size: crate::verify::types::BlockSize,
    ) -> Result<BufferSlideResult, ()> {
        use crate::verify::types::BufferSize;

        if !state.can_slide_window(block_size) {
            return Ok(BufferSlideResult::CannotSlide);
        }

        let bytes_to_keep = state.bytes_in_buffer.bytes_after_slide(block_size);

        // Slide buffer contents
        buffer.slide_window(state.bytes_in_buffer, block_size);

        // Read more data
        let bytes_read = buffer
            .read_into_slice(file, bytes_to_keep)
            .map_err(|_| ())?;

        // Update state
        let new_buffer_size = BufferSize::from_slide(bytes_to_keep, bytes_read);
        state.slide_window(block_size, new_buffer_size);

        Ok(BufferSlideResult::Success)
    }

    /// Report scanning progress to the reporter
    fn report_progress<R: VerificationReporter>(
        reporter_lock: &Mutex<&R>,
        state: &crate::verify::scanner_state::ScannerState,
        file_size: crate::verify::types::FileSize,
    ) {
        let fraction = state.bytes_processed.progress_fraction(file_size.as_u64());
        if let Ok(reporter) = reporter_lock.lock() {
            reporter.report_scanning_progress(fraction);
        }
    }

    /// Scan a single block position and determine action
    /// Returns SkipBlock if match found, AdvanceOneByte if no match
    fn scan_block_position(
        &self,
        buffer: &crate::verify::types::ScanBuffer,
        state: &crate::verify::scanner_state::ScannerState,
        block_size: crate::verify::types::BlockSize,
        local_block_map: &mut LocalBlockMap,
    ) -> ScanAction {
        use crate::checksum::compute_crc32;

        let block_data = buffer.block_at(state.scan_pos, block_size);

        // Use rolling CRC if we have it, otherwise compute fresh
        let crc32 = state
            .rolling_crc
            .unwrap_or_else(|| compute_crc32(block_data));

        // Fast CRC32 lookup in global table - only compute MD5 if CRC matches
        let found_match = if self.block_table.find_by_crc32(crc32).is_some() {
            let md5_hash = crate::checksum::compute_md5_only(block_data);
            self.insert_matching_blocks(md5_hash, crc32, local_block_map)
        } else {
            BlockMatchResult::NotMatched
        };

        if found_match.is_match() {
            ScanAction::SkipBlock
        } else {
            ScanAction::AdvanceOneByte
        }
    }

    /// Count available blocks and damaged blocks for a file
    fn count_file_blocks(
        &self,
        file_id: FileId,
        file_size: FileSize,
        local_block_map: &LocalBlockMap,
    ) -> (BlockCount, Vec<u32>) {
        use crate::verify::types::BlockCount;

        let total_blocks = self.calculate_total_blocks(file_size);
        let mut blocks_available = BlockCount::zero();
        let mut damaged_blocks = Vec::new();
        let file_blocks = self.block_table.get_file_blocks(file_id);

        for block_number in total_blocks.iter_block_numbers() {
            if let Some(expected_block) = file_blocks.get(block_number.as_usize()) {
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

        (blocks_available, damaged_blocks)
    }

    /// Determine file status based on available blocks
    fn determine_file_status(blocks_available: BlockCount, total_blocks: BlockCount) -> FileStatus {
        if blocks_available.is_complete(total_blocks) {
            FileStatus::Present
        } else if blocks_available.is_empty() {
            FileStatus::Missing
        } else {
            FileStatus::Corrupted
        }
    }

    /// Report file verification status to the reporter
    fn report_file_status<R: VerificationReporter>(
        reporter_lock: &Mutex<&R>,
        file_name: &str,
        status: FileStatus,
        damaged_blocks: &[u32],
        blocks_available: BlockCount,
        total_blocks: BlockCount,
    ) {
        let reporter = match reporter_lock.lock() {
            Ok(r) => r,
            Err(_) => return,
        };

        match status {
            FileStatus::Present | FileStatus::Missing | FileStatus::Renamed => {
                reporter.report_file_status(file_name, status);
            }
            FileStatus::Corrupted => {
                reporter.report_damaged_blocks(
                    file_name,
                    damaged_blocks,
                    blocks_available.as_usize(),
                    total_blocks.as_usize(),
                );
            }
        }
    }

    /// Try to find blocks at aligned positions (optimization for well-formed PAR2 files)
    fn try_aligned_blocks(
        &self,
        buffer: &crate::verify::types::ScanBuffer,
        local_block_map: &mut LocalBlockMap,
        block_size: crate::verify::types::BlockSize,
        bytes_in_buffer: crate::verify::types::BufferSize,
    ) {
        for block_idx in 0..2 {
            if let Some(block_data) =
                buffer.try_aligned_block(block_idx, block_size, bytes_in_buffer)
            {
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
            let status = if blocks_available.is_complete(total_blocks) {
                FileStatus::Present
            } else if blocks_available.is_empty() {
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
        assert_eq!(
            found,
            BlockMatchResult::Matched,
            "Should find matching block"
        );
        assert_eq!(local_map.len(), 1, "Should have one entry in local map");

        // Test non-matching MD5
        let mut local_map2 = HashMap::default();
        let found = engine.insert_matching_blocks(
            Md5Hash::new([0xBB; 16]),
            Crc32Value::new(0x12345678),
            &mut local_map2,
        );
        assert_eq!(
            found,
            BlockMatchResult::NotMatched,
            "Should not find block with wrong MD5"
        );
        assert_eq!(local_map2.len(), 0, "Should have no entries");

        // Test non-matching CRC32
        let mut local_map3 = HashMap::default();
        let found = engine.insert_matching_blocks(
            Md5Hash::new([0xAA; 16]),
            Crc32Value::new(0x99999999),
            &mut local_map3,
        );
        assert_eq!(
            found,
            BlockMatchResult::NotMatched,
            "Should not find block with wrong CRC32"
        );
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
        assert_eq!(
            found,
            BlockMatchResult::Matched,
            "Should find matching block"
        );
        assert_eq!(local_map.len(), 1, "Should have one entry");

        // Test non-matching data
        let wrong_data = vec![0x99; 1024];
        let mut local_map2 = HashMap::default();
        let found = engine.try_match_and_insert_block(&wrong_data, &mut local_map2);
        assert_eq!(
            found,
            BlockMatchResult::NotMatched,
            "Should not find non-matching block"
        );
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
        assert_eq!(
            found,
            BlockMatchResult::Matched,
            "Should find matching partial block"
        );
        assert_eq!(local_map.len(), 1, "Should have one entry");

        // Test non-matching partial data
        let wrong_data = vec![0x99; 500];
        let mut local_map2 = HashMap::default();
        let found = engine.try_match_and_insert_partial_block(&wrong_data, 1024, &mut local_map2);
        assert_eq!(
            found,
            BlockMatchResult::NotMatched,
            "Should not find non-matching partial block"
        );
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

    #[test]
    fn test_update_crc_after_skip() {
        use crate::checksum::compute_crc32;
        use crate::checksum::rolling_crc::RollingCrcTable;
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, ScanBuffer};

        let block_size = BlockSize::new(1024);
        let rolling_table = RollingCrcTable::new(1024);
        let mut buffer = ScanBuffer::with_capacity(2048);
        buffer.fill(0x42u8);

        // Create state at position 0
        let mut state = ScannerState::new(2048);
        state.set_rolling_crc(Some(Crc32Value::new(0x12345678))); // Set some initial CRC

        // Skip forward by one block
        state.skip_block(block_size);

        // Update CRC after skip - should recompute from scratch at new position
        let new_crc = rolling_table
            .compute_crc_at_position(
                buffer.as_slice(),
                state.scan_pos.as_usize(),
                block_size.as_usize(),
                state.bytes_in_buffer.as_usize(),
            )
            .map(Crc32Value::new);
        state.set_rolling_crc(new_crc);

        // Should have computed CRC for block starting at position 1024
        assert!(state.rolling_crc.is_some(), "Should have rolling CRC");
        let expected_crc = compute_crc32(buffer.slice(1024..2048));
        assert_eq!(
            state.rolling_crc.unwrap(),
            expected_crc,
            "CRC should match expected value for second block"
        );

        // Test case where we can't fit another block (at end)
        let mut state2 = ScannerState::new(1024); // Only one block
        state2.set_rolling_crc(Some(Crc32Value::new(0x99999999)));
        state2.skip_block(block_size); // Now at position 1024 with buffer size 1024

        let new_crc2 = rolling_table
            .compute_crc_at_position(
                buffer.as_slice(),
                state2.scan_pos.as_usize(),
                block_size.as_usize(),
                state2.bytes_in_buffer.as_usize(),
            )
            .map(Crc32Value::new);
        state2.set_rolling_crc(new_crc2);

        // Should have cleared CRC because we can't fit another block
        assert!(
            state2.rolling_crc.is_none(),
            "Should clear CRC when can't fit block"
        );
    }

    #[test]
    fn test_slide_crc_one_byte() {
        use crate::checksum::compute_crc32;
        use crate::checksum::rolling_crc::RollingCrcTable;
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, ScanBuffer};

        let block_size = BlockSize::new(1024);
        let rolling_table = RollingCrcTable::new(1024);

        // Create a buffer with known pattern
        let mut buffer = ScanBuffer::with_capacity(2048);
        for (i, item) in buffer.iter_mut().enumerate().take(2048) {
            *item = (i % 256) as u8;
        }

        // Create state at position 1 (after advancing from 0)
        let mut state = ScannerState::new(2048);

        // Compute initial CRC for block at position 0
        let initial_crc = compute_crc32(buffer.slice(0..1024));
        state.set_rolling_crc(Some(initial_crc));

        // Advance one byte to position 1
        state.advance_one_byte();

        // Now slide the CRC - should use rolling algorithm
        if let Some(crc) = state.rolling_crc {
            let new_crc = rolling_table
                .slide_crc_forward(
                    crc.as_u32(),
                    buffer.as_slice(),
                    state.scan_pos.as_usize(),
                    block_size.as_usize(),
                    state.bytes_in_buffer.as_usize(),
                )
                .map(Crc32Value::new);
            state.set_rolling_crc(new_crc);
        }

        // Verify the CRC was updated correctly
        assert!(state.rolling_crc.is_some(), "Should have rolling CRC");

        // The rolled CRC should match a fresh computation at position 1
        let expected_crc = compute_crc32(buffer.slice(1..1025));
        assert_eq!(
            state.rolling_crc.unwrap(),
            expected_crc,
            "Rolled CRC should match fresh CRC at position 1"
        );

        // Test case where state has no rolling CRC
        let mut state2 = ScannerState::new(2048);
        state2.advance_one_byte(); // At position 1 but no CRC

        if let Some(crc) = state2.rolling_crc {
            let new_crc = rolling_table
                .slide_crc_forward(
                    crc.as_u32(),
                    buffer.as_slice(),
                    state2.scan_pos.as_usize(),
                    block_size.as_usize(),
                    state2.bytes_in_buffer.as_usize(),
                )
                .map(Crc32Value::new);
            state2.set_rolling_crc(new_crc);
        }

        // Should remain None
        assert!(
            state2.rolling_crc.is_none(),
            "Should stay None when no initial CRC"
        );

        // Test case where we can't fit a full block after current position
        let mut state3 = ScannerState::new(1000); // Less than a full block
        state3.set_rolling_crc(Some(Crc32Value::new(0x12345678)));
        state3.advance_one_byte();

        if let Some(crc) = state3.rolling_crc {
            let new_crc = rolling_table
                .slide_crc_forward(
                    crc.as_u32(),
                    buffer.as_slice(),
                    state3.scan_pos.as_usize(),
                    block_size.as_usize(),
                    state3.bytes_in_buffer.as_usize(),
                )
                .map(Crc32Value::new);
            state3.set_rolling_crc(new_crc);
        }

        // CRC should be cleared since we can't fit a block
        assert!(
            state3.rolling_crc.is_none(),
            "CRC should remain unchanged when can't fit block"
        );
    }

    #[test]
    fn test_crc_helpers_are_consistent() {
        // Verify that compute_crc_at_position and slide_crc_forward produce
        // consistent results with manual CRC computation

        use crate::checksum::compute_crc32;
        use crate::checksum::rolling_crc::RollingCrcTable;
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, ScanBuffer};

        let block_size = BlockSize::new(1024);
        let rolling_table = RollingCrcTable::new(1024);
        let mut buffer = ScanBuffer::with_capacity(3072);
        buffer.fill(0x42u8);

        // Method 1: Skip forward and recompute
        let mut state1 = ScannerState::new(3072);
        state1.set_rolling_crc(Some(compute_crc32(buffer.slice(0..1024))));
        state1.skip_block(block_size); // Now at position 1024
        let new_crc1 = rolling_table
            .compute_crc_at_position(
                buffer.as_slice(),
                state1.scan_pos.as_usize(),
                block_size.as_usize(),
                state1.bytes_in_buffer.as_usize(),
            )
            .map(Crc32Value::new);
        state1.set_rolling_crc(new_crc1);

        // Method 2: Advance byte by byte using rolling CRC
        let mut state2 = ScannerState::new(3072);
        state2.set_rolling_crc(Some(compute_crc32(buffer.slice(0..1024))));

        for _ in 0..1024 {
            state2.advance_one_byte();
            if let Some(crc) = state2.rolling_crc {
                let new_crc = rolling_table
                    .slide_crc_forward(
                        crc.as_u32(),
                        buffer.as_slice(),
                        state2.scan_pos.as_usize(),
                        block_size.as_usize(),
                        state2.bytes_in_buffer.as_usize(),
                    )
                    .map(Crc32Value::new);
                state2.set_rolling_crc(new_crc);
            }
        }

        // Both should produce the same CRC
        assert_eq!(
            state1.rolling_crc, state2.rolling_crc,
            "Skip-and-recompute should match rolling CRC after 1024 slides"
        );

        // And both should match manual computation
        let expected_crc = compute_crc32(buffer.slice(1024..2048));
        assert_eq!(
            state1.rolling_crc.unwrap(),
            expected_crc,
            "Both methods should match manual CRC computation"
        );
    }

    #[test]
    fn test_slide_buffer_window() {
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, ScanBuffer};
        use std::io::Cursor;

        let block_size = BlockSize::new(1024);

        // Create a mock file with 3 blocks of data
        let file_data = vec![0x42u8; 3072];
        let mut file = Cursor::new(file_data);

        // Create buffer that can hold 2 blocks
        let mut buffer = ScanBuffer::with_capacity(2048);

        // Initial read
        let bytes_read = buffer.read_from(&mut file).unwrap();
        let mut state = ScannerState::new(bytes_read);

        assert_eq!(
            state.bytes_in_buffer.as_usize(),
            2048,
            "Should have 2 blocks initially"
        );

        // Slide the window
        let result = GlobalVerificationEngine::slide_buffer_window(
            &mut file,
            &mut buffer,
            &mut state,
            block_size,
        );

        assert!(result.is_ok(), "Slide should succeed");
        assert_eq!(
            result.unwrap(),
            BufferSlideResult::Success,
            "Should return Success on successful slide"
        );

        // After sliding, should have 1 old block + 1 new block
        assert_eq!(
            state.bytes_in_buffer.as_usize(),
            2048,
            "Should still have 2 blocks"
        );

        // Check bytes processed - should be 1 block worth
        assert_eq!(
            state.bytes_processed.as_u64(),
            1024,
            "Should have processed 1 block"
        );

        // Slide again
        let result2 = GlobalVerificationEngine::slide_buffer_window(
            &mut file,
            &mut buffer,
            &mut state,
            block_size,
        );

        assert!(result2.is_ok(), "Second slide should succeed");
        assert_eq!(
            result2.unwrap(),
            BufferSlideResult::Success,
            "Should return Success on second slide"
        );

        assert_eq!(
            state.bytes_processed.as_u64(),
            2048,
            "Should have processed 2 blocks"
        );

        // Try to slide when at EOF (no more data)
        let result3 = GlobalVerificationEngine::slide_buffer_window(
            &mut file,
            &mut buffer,
            &mut state,
            block_size,
        );

        assert!(result3.is_ok(), "Should succeed but return false");
        // At this point we have less than a full block remaining, so can't slide
    }

    #[test]
    fn test_slide_buffer_window_cant_slide() {
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, ScanBuffer};
        use std::io::Cursor;

        let block_size = BlockSize::new(1024);

        // Create a file with less than 1 block
        let file_data = vec![0x42u8; 512];
        let mut file = Cursor::new(file_data);

        let mut buffer = ScanBuffer::with_capacity(2048);
        let bytes_read = buffer.read_from(&mut file).unwrap();
        let mut state = ScannerState::new(bytes_read);

        // Can't slide because buffer has less than 1 block
        let result = GlobalVerificationEngine::slide_buffer_window(
            &mut file,
            &mut buffer,
            &mut state,
            block_size,
        );

        assert!(result.is_ok(), "Should not error");
        assert_eq!(
            result.unwrap(),
            BufferSlideResult::CannotSlide,
            "Should return CannotSlide when can't slide"
        );
    }

    #[test]
    fn test_report_progress() {
        use crate::reporters::ConsoleVerificationReporter;
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, FileSize};
        use std::sync::Mutex;

        let reporter = ConsoleVerificationReporter::new();
        let reporter_lock = Mutex::new(&reporter);

        let block_size = BlockSize::new(1024);

        // Create state that's halfway through a 2048 byte file
        let mut state = ScannerState::new(2048);
        // Advance by one block to simulate processing
        state.bytes_processed.advance_by(block_size);

        let file_size = FileSize::new(2048);

        // Report progress - should be 50%
        GlobalVerificationEngine::report_progress(&reporter_lock, &state, file_size);

        // Verify the reporter was called (we can't easily inspect the output,
        // but at least we verify it doesn't panic)
        // The function should compute fraction = 1024 / 2048 = 0.5

        // Test at 100%
        state.bytes_processed.advance_by(block_size);
        GlobalVerificationEngine::report_progress(&reporter_lock, &state, file_size);
    }

    #[test]
    fn test_count_file_blocks() {
        use crate::verify::global_table::GlobalBlockTableBuilder;
        use crate::verify::types::FileSize;

        // Create a block table with 3 blocks for a file
        let mut builder = GlobalBlockTableBuilder::new(1024);
        let file_id = FileId::new([1; 16]);

        let checksums = vec![
            (Md5Hash::new([0xAA; 16]), Crc32Value::new(0x11111111)),
            (Md5Hash::new([0xBB; 16]), Crc32Value::new(0x22222222)),
            (Md5Hash::new([0xCC; 16]), Crc32Value::new(0x33333333)),
        ];
        builder.add_file_blocks(file_id, &checksums);
        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        // Case 1: All blocks available
        let mut local_map = HashMap::default();
        local_map.insert(
            (Md5Hash::new([0xAA; 16]), Crc32Value::new(0x11111111)),
            SmallVec::new(),
        );
        local_map.insert(
            (Md5Hash::new([0xBB; 16]), Crc32Value::new(0x22222222)),
            SmallVec::new(),
        );
        local_map.insert(
            (Md5Hash::new([0xCC; 16]), Crc32Value::new(0x33333333)),
            SmallVec::new(),
        );

        let (available, damaged) =
            engine.count_file_blocks(file_id, FileSize::new(3072), &local_map);
        assert_eq!(available.as_usize(), 3, "Should have all 3 blocks");
        assert_eq!(damaged.len(), 0, "Should have no damaged blocks");

        // Case 2: Some blocks missing
        let mut local_map2 = HashMap::default();
        local_map2.insert(
            (Md5Hash::new([0xAA; 16]), Crc32Value::new(0x11111111)),
            SmallVec::new(),
        );
        // Block 1 missing
        local_map2.insert(
            (Md5Hash::new([0xCC; 16]), Crc32Value::new(0x33333333)),
            SmallVec::new(),
        );

        let (available2, damaged2) =
            engine.count_file_blocks(file_id, FileSize::new(3072), &local_map2);
        assert_eq!(available2.as_usize(), 2, "Should have 2 blocks");
        assert_eq!(damaged2.len(), 1, "Should have 1 damaged block");
        assert_eq!(damaged2[0], 1, "Block 1 should be damaged");

        // Case 3: No blocks available
        let local_map3 = HashMap::default();
        let (available3, damaged3) =
            engine.count_file_blocks(file_id, FileSize::new(3072), &local_map3);
        assert_eq!(available3.as_usize(), 0, "Should have 0 blocks");
        assert_eq!(damaged3.len(), 3, "Should have 3 damaged blocks");
    }

    #[test]
    fn test_determine_file_status() {
        use crate::verify::types::BlockCount;

        // All blocks present
        let status = GlobalVerificationEngine::determine_file_status(
            BlockCount::new(10),
            BlockCount::new(10),
        );
        assert_eq!(status, FileStatus::Present);

        // No blocks present
        let status2 = GlobalVerificationEngine::determine_file_status(
            BlockCount::zero(),
            BlockCount::new(10),
        );
        assert_eq!(status2, FileStatus::Missing);

        // Some blocks present
        let status3 = GlobalVerificationEngine::determine_file_status(
            BlockCount::new(5),
            BlockCount::new(10),
        );
        assert_eq!(status3, FileStatus::Corrupted);
    }

    #[test]
    fn test_report_file_status() {
        use crate::reporters::ConsoleVerificationReporter;
        use crate::verify::types::BlockCount;
        use std::sync::Mutex;

        let reporter = ConsoleVerificationReporter::new();
        let reporter_lock = Mutex::new(&reporter);

        // Test reporting present file
        GlobalVerificationEngine::report_file_status(
            &reporter_lock,
            "test.txt",
            FileStatus::Present,
            &[],
            BlockCount::new(10),
            BlockCount::new(10),
        );

        // Test reporting missing file
        GlobalVerificationEngine::report_file_status(
            &reporter_lock,
            "test.txt",
            FileStatus::Missing,
            &[],
            BlockCount::zero(),
            BlockCount::new(10),
        );

        // Test reporting corrupted file
        GlobalVerificationEngine::report_file_status(
            &reporter_lock,
            "test.txt",
            FileStatus::Corrupted,
            &[1, 5, 7],
            BlockCount::new(7),
            BlockCount::new(10),
        );

        // All calls should succeed without panicking
    }

    #[test]
    fn test_scan_block_position() {
        use crate::verify::global_table::GlobalBlockTableBuilder;
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, ScanBuffer};

        // Create a block table with one known block
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

        // Create a buffer with the matching block
        let mut buffer = ScanBuffer::with_capacity(2048);
        buffer.fill(0x42);
        let block_size = BlockSize::new(1024);
        let state = ScannerState::new(2048);

        let mut local_map = HashMap::default();

        // Test: matching block should return SkipBlock
        let action = engine.scan_block_position(&buffer, &state, block_size, &mut local_map);
        assert_eq!(action, ScanAction::SkipBlock, "Should skip matching block");
        assert_eq!(local_map.len(), 1, "Should have inserted the match");

        // Test: non-matching block should return AdvanceOneByte
        let mut wrong_buffer = ScanBuffer::with_capacity(2048);
        wrong_buffer.fill(0x99);
        let mut local_map2 = HashMap::default();
        let action2 =
            engine.scan_block_position(&wrong_buffer, &state, block_size, &mut local_map2);
        assert_eq!(
            action2,
            ScanAction::AdvanceOneByte,
            "Should advance on non-match"
        );
        assert_eq!(local_map2.len(), 0, "Should not have inserted anything");
    }
}
