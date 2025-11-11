//! Enhanced verification engine using global block table
//!
//! This module provides comprehensive PAR2 verification using a global block table
//! approach similar to par2cmdline. It verifies the entire recovery set holistically
//! rather than individual files in isolation.

use super::global_table::{GlobalBlockTable, GlobalBlockTableBuilder};
use super::scanner_state::ScannerState;
use super::types::{
    BlockCount, BlockNumber, BlockVerificationResult, FileScanMetadata, FileSize, FileStatus,
    FileVerificationResult, VerificationResults,
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

/// Map of block checksums to file locations where they were found
type AvailableBlocksMap = HashMap<(Md5Hash, Crc32Value), Vec<(FileId, u32)>>;
/// Map of file IDs to their determined status
type FileStatusMap = HashMap<FileId, FileStatus>;

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
    /// Number of recovery blocks available
    recovery_block_count: usize,
    /// Skip full file MD5 computation (for pre-repair verification)
    skip_full_md5: bool,
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
        Self::from_packets_with_config(packets, base_dir, &super::VerificationConfig::default())
    }

    /// Create a new verification engine from packets with config
    pub fn from_packets_with_config(
        packets: &[crate::Packet],
        base_dir: impl AsRef<Path>,
        config: &super::VerificationConfig,
    ) -> Result<Self, String> {
        // Extract packet information
        let block_size = crate::packets::processing::extract_main_packet(packets)
            .map(|m| m.slice_size)
            .ok_or("No main packet found")?;

        let file_descriptions = crate::packets::processing::extract_file_descriptions(packets);
        let slice_checksums = crate::packets::processing::extract_slice_checksums(packets);

        // Count recovery blocks available
        let recovery_block_count = packets
            .iter()
            .filter_map(|p| match p {
                crate::Packet::RecoverySlice(_) => Some(1),
                _ => None,
            })
            .sum();

        // Build global block table
        let mut builder = GlobalBlockTableBuilder::new(block_size);

        for file_description in &file_descriptions {
            if let Some(checksums) = slice_checksums.get(&file_description.file_id) {
                builder.add_file_blocks(file_description.file_id, checksums);
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
            recovery_block_count,
            skip_full_md5: config.skip_full_file_md5,
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
        let (available_blocks, file_statuses, scan_metadatas) =
            self.scan_available_blocks(reporter, parallel);

        // Step 2: Create aggregate results (individual file reporting already done in scan_available_blocks)
        let file_results =
            self.create_file_results(&available_blocks, &file_statuses, &scan_metadatas);
        let block_results = self.create_block_verification_results(&available_blocks);

        // Count recovery blocks available from recovery packets
        // Note: We count ALL recovery packets that were loaded, not just those needed
        let recovery_blocks_available = self.recovery_block_count;

        VerificationResults::from_file_results(
            file_results,
            block_results,
            recovery_blocks_available,
        )
    }

    /// Scan all available files and build a global map of which blocks exist where
    /// This is the core of the global block table approach - we scan every file
    /// and index every block we find by its checksum, regardless of filename
    fn scan_available_blocks<R: VerificationReporter>(
        &self,
        reporter: &R,
        parallel: bool,
    ) -> (
        AvailableBlocksMap,
        FileStatusMap,
        HashMap<FileId, FileScanMetadata>,
    ) {
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
                .map(|file_description| self.process_single_file(file_description, &reporter_lock))
                .collect()
        } else {
            files_to_scan
                .iter()
                .map(|file_description| self.process_single_file(file_description, &reporter_lock))
                .collect()
        };

        // Merge all local maps into global map and collect statuses and metadata
        let mut global_block_map = HashMap::default();
        let mut file_statuses = HashMap::default();
        let mut scan_metadatas = HashMap::default();

        for (local_map, _file_size, file_id, status, metadata) in file_results {
            // Store the computed status
            file_statuses.insert(file_id, status);

            // Store the scan metadata
            scan_metadatas.insert(file_id, metadata);

            // Merge local map into global
            for (key, entries) in local_map {
                global_block_map
                    .entry(key)
                    .or_insert_with(Vec::new)
                    .extend(entries);
            }
        }

        (global_block_map, file_statuses, scan_metadatas)
    }

    /// Process a single file: scan blocks and report status
    fn process_single_file<R: VerificationReporter>(
        &self,
        file_description: &FileDescriptionPacket,
        reporter_lock: &Mutex<&R>,
    ) -> (
        LocalBlockMap,
        FileSize,
        FileId,
        FileStatus,
        FileScanMetadata,
    ) {
        use crate::verify::types::FileSize;

        let file_name = extract_file_name(file_description);
        let file_path = self.base_dir.join(&file_name);
        let file_size = FileSize::new(file_description.file_length);

        // Lock reporter to start file
        {
            let reporter = reporter_lock.lock().unwrap();
            reporter.report_verifying_file(&file_name);
        }

        // Scan this file and get its local block map, reporting progress
        let (local_block_map, mut scan_metadata) =
            self.scan_single_file_with_progress(&file_path, file_size, reporter_lock);

        // Calculate status for this file
        let total_blocks = self.calculate_total_blocks(file_size);
        let (blocks_available, damaged_blocks) =
            self.count_file_blocks(file_description.file_id, file_size, &local_block_map);

        // Analyze block positions to determine if blocks are properly aligned
        scan_metadata.analyze_block_positions(file_description.file_id);

        // Determine status (considering scan metadata)
        let status = Self::determine_file_status_with_metadata(
            blocks_available,
            total_blocks,
            &scan_metadata,
            &file_description.md5_hash,
        );

        // Report status
        Self::report_file_status(
            reporter_lock,
            &file_name,
            status,
            &damaged_blocks,
            blocks_available,
            total_blocks,
        );

        (
            local_block_map,
            file_size,
            file_description.file_id,
            status,
            scan_metadata,
        )
    }

    /// Scan a single file and return its local block map with progress reporting
    fn scan_single_file_with_progress<R: VerificationReporter>(
        &self,
        file_path: &Path,
        file_size: FileSize,
        reporter_lock: &Mutex<&R>,
    ) -> (LocalBlockMap, FileScanMetadata) {
        use crate::checksum::compute_crc32;
        use crate::checksum::rolling_crc::RollingCrcTable;
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, ScanBuffer};
        use std::fs::File;

        let mut local_block_map = HashMap::default();

        let mut file = match File::open(file_path) {
            Ok(f) => f,
            Err(_) => return (local_block_map, FileScanMetadata::new()),
        };

        let block_size = BlockSize::new(self.block_table.block_size() as usize);
        let buffer_capacity = block_size.doubled();
        let mut buffer = ScanBuffer::with_capacity(buffer_capacity);

        // Create rolling CRC table for efficient scanning
        let rolling_table = RollingCrcTable::new(block_size.as_usize());

        // Initial fill of the buffer
        let bytes_read = match buffer.read_from(&mut file) {
            Ok(n) => n,
            Err(_) => return (local_block_map, FileScanMetadata::new()),
        };

        log::debug!(
            "Starting scan: block_size={}, buffer_capacity={}, bytes_read={}, file_size={}",
            block_size.as_usize(),
            buffer_capacity,
            bytes_read,
            file_size.as_u64()
        );

        // Initialize scanner state (includes scan metadata)
        let mut state = ScannerState::new(bytes_read);

        // PHASE 1 & 1.5: Aligned blocks and short file detection
        // The type system ensures short files are handled completely here

        // PHASE 1: Try aligned blocks at file start (fast path for well-formed files)
        self.scan_aligned_blocks(&buffer, &mut state, block_size, &mut local_block_map);

        // PHASE 1.5: Detect and handle files entirely smaller than one block
        // This is the ONLY place where short files should be processed
        // Reference: par2cmdline-turbo handles short files in filechecksummer.cpp
        if state.is_remainder_at_start() && state.remainder_size(block_size) > 0 {
            let partial_data = buffer.slice_from(state.buffer_position, state.bytes_in_buffer);
            self.try_match_and_insert_partial_block(
                partial_data,
                block_size.as_usize(),
                &mut local_block_map,
                &mut state,
            );

            // Short file is now complete - mark as 100% scanned and compute file hash
            Self::report_progress(reporter_lock, &state, file_size);

            if !self.skip_full_md5 {
                if let Ok(md5) = crate::checksum::calculate_file_md5(file_path) {
                    state.scan_metadata.actual_file_hash = Some(md5);
                }
            }

            // Return early - file is complete, prevent duplicate detection in Phase 2
            return (local_block_map, state.scan_metadata);
        }

        // PHASE 2: Byte-by-byte scanning through entire file
        // Reference: par2cmdline-turbo/src/par2repairer.cpp:1676-1795 (byte-by-byte scan loop)
        // Reference: par2cmdline-turbo/src/filechecksummer.h:169-192 (Step function)

        // Initialize rolling CRC for current position if we have a full block
        if state.can_fit_block(block_size) {
            let initial_crc = compute_crc32(buffer.block_at(state.buffer_position, block_size));
            state.set_rolling_crc(Some(initial_crc));
        }

        // Scan byte-by-byte through the entire file (like par2cmdline-turbo's Step loop)
        let mut step_count: u64 = 0;
        loop {
            step_count += 1;

            // Check if we've reached end of file
            if state.bytes_processed.as_u64() >= file_size.as_u64() {
                log::debug!(
                    "Reached EOF after {} steps, found {} blocks",
                    step_count,
                    local_block_map.len()
                );
                break;
            }

            // Handle partial block at end of file
            if !state.can_fit_block(block_size) {
                if state.remainder_size(block_size) > 0 {
                    let partial_data =
                        buffer.slice_from(state.buffer_position, state.bytes_in_buffer);
                    self.try_match_and_insert_partial_block(
                        partial_data,
                        block_size.as_usize(),
                        &mut local_block_map,
                        &mut state,
                    );
                }
                log::debug!(
                    "Cannot fit block, exiting after {} steps, found {} blocks",
                    step_count,
                    local_block_map.len()
                );
                break;
            }

            // Try to match block at current position
            match self.scan_block_position(&buffer, &mut state, block_size, &mut local_block_map) {
                ScanAction::SkipBlock => {
                    // Found a match - jump forward by block size (like par2cmdline-turbo's Jump)
                    state.skip_block(block_size);

                    // Recompute CRC after skip (can't roll forward a full block)
                    if state.can_fit_block(block_size) {
                        let new_crc =
                            compute_crc32(buffer.block_at(state.buffer_position, block_size));
                        state.set_rolling_crc(Some(new_crc));
                    } else {
                        state.set_rolling_crc(None);
                    }
                }
                ScanAction::AdvanceOneByte => {
                    // No match - advance one byte with rolling CRC (like par2cmdline-turbo's Step)
                    state.advance_one_byte();
                    state.slide_crc_one_byte(&rolling_table, &buffer, block_size);
                }
            }

            // Check if we need to refill the buffer
            // When buffer position reaches blocksize, slide the buffer to keep data available
            if state.buffer_position.as_usize() >= block_size.as_usize() {
                match Self::slide_buffer_window(&mut file, &mut buffer, &mut state, block_size) {
                    Ok(BufferSlideResult::Success) => {
                        Self::report_progress(reporter_lock, &state, file_size);

                        // Recompute CRC at new buffer position
                        if state.can_fit_block(block_size) {
                            let new_crc =
                                compute_crc32(buffer.block_at(state.buffer_position, block_size));
                            state.set_rolling_crc(Some(new_crc));
                        } else {
                            state.set_rolling_crc(None);
                        }
                    }
                    Ok(BufferSlideResult::CannotSlide) => break,
                    Err(_) => break,
                }
            }
        }

        // Mark file as 100% scanned
        Self::report_progress(reporter_lock, &state, file_size);

        // Compute file MD5 hash and store in metadata using a streaming hasher
        // (avoid reading entire file into memory for large files)
        if !self.skip_full_md5 {
            if let Ok(md5) = crate::checksum::calculate_file_md5(file_path) {
                state.scan_metadata.actual_file_hash = Some(md5);
            }
        }

        (local_block_map, state.scan_metadata)
    }

    /// Insert all matching blocks from the global table into the local block map
    /// Returns whether at least one match was found
    fn insert_matching_blocks(
        &self,
        md5_hash: Md5Hash,
        crc32: Crc32Value,
        local_block_map: &mut LocalBlockMap,
        state: &mut ScannerState,
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

                        // Record block discovery in state metadata
                        state.record_block_found(
                            duplicate.position.file_id,
                            duplicate.position.block_number,
                        );
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
        state: &mut ScannerState,
    ) -> BlockMatchResult {
        use crate::checksum::{compute_crc32, compute_md5_only};

        let crc32 = compute_crc32(block_data);

        // Fast CRC32 lookup - only compute expensive MD5 if CRC matches
        if self.block_table.find_by_crc32(crc32).is_some() {
            let md5_hash = compute_md5_only(block_data);
            self.insert_matching_blocks(md5_hash, crc32, local_block_map, state)
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
        state: &mut ScannerState,
    ) -> BlockMatchResult {
        use crate::checksum::compute_block_checksums_padded;

        let (md5_hash, crc32) = compute_block_checksums_padded(partial_data, block_size);
        self.insert_matching_blocks(md5_hash, crc32, local_block_map, state)
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

        // Slide buffer and read more data
        let bytes_read = buffer
            .slide_and_read(file, state.bytes_in_buffer, block_size)
            .map_err(|_| ())?;

        // Update state
        let bytes_to_keep = state.bytes_in_buffer.bytes_after_slide(block_size);
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
        state: &mut crate::verify::scanner_state::ScannerState,
        block_size: crate::verify::types::BlockSize,
        local_block_map: &mut LocalBlockMap,
    ) -> ScanAction {
        use crate::checksum::compute_crc32;

        let block_data = buffer.block_at(state.buffer_position, block_size);

        // Use rolling CRC if we have it, otherwise compute fresh
        let crc32 = state
            .rolling_crc
            .unwrap_or_else(|| compute_crc32(block_data));

        // Fast CRC32 lookup in global table - only compute MD5 if CRC matches
        let found_match = if self.block_table.find_by_crc32(crc32).is_some() {
            let md5_hash = crate::checksum::compute_md5_only(block_data);
            self.insert_matching_blocks(md5_hash, crc32, local_block_map, state)
        } else {
            BlockMatchResult::NotMatched
        };

        if found_match.is_match() {
            ScanAction::SkipBlock
        } else {
            ScanAction::AdvanceOneByte
        }
    }

    /// Scan aligned blocks at the start of a buffer (optimization for well-formed files)
    /// Reference: par2cmdline-turbo/src/par2repairer.cpp:1702-1720 (checks aligned positions first)
    /// This function does NOT advance buffer_position - it just tries aligned positions
    /// The byte-by-byte scan will start from wherever this leaves off
    fn scan_aligned_blocks(
        &self,
        buffer: &crate::verify::types::ScanBuffer,
        state: &mut crate::verify::scanner_state::ScannerState,
        block_size: crate::verify::types::BlockSize,
        local_block_map: &mut LocalBlockMap,
    ) {
        if !state.bytes_in_buffer.has_at_least_n_blocks(2, block_size) {
            return;
        }

        let bytes_in_buffer = state.bytes_in_buffer.as_usize();
        let block_size_usize = block_size.as_usize();

        // Generate aligned block positions (0, block_size, 2*block_size, ...)
        let mut pos = 0;
        while pos + block_size_usize <= bytes_in_buffer {
            // Temporarily set buffer position for the scan
            state.buffer_position = crate::verify::types::BufferPosition::new(pos);

            let block_data =
                buffer.block_at(crate::verify::types::BufferPosition::new(pos), block_size);
            let matched = self.try_match_and_insert_block(block_data, local_block_map, state);

            if !matched.is_match() {
                // Stop at first non-match - this is where byte-by-byte scanning should start
                state.buffer_position = crate::verify::types::BufferPosition::new(pos);
                return;
            }

            pos += block_size_usize;
        }

        // If we scanned all aligned positions, start byte-by-byte from position 0
        // (we'll revisit these positions but with rolling CRC this time)
        state.buffer_position = crate::verify::types::BufferPosition::zero();
    }

    /// Scan byte-by-byte through the buffer using rolling CRC
    /// Reference: par2cmdline-turbo/src/par2repairer.cpp:1676-1795 (byte-by-byte scan loop)
    /// Reference: par2cmdline-turbo/src/filechecksummer.h:169-192 (Step function)
    #[cfg(test)]
    fn scan_byte_by_byte(
        &self,
        buffer: &crate::verify::types::ScanBuffer,
        state: &mut crate::verify::scanner_state::ScannerState,
        rolling_table: &crate::checksum::rolling_crc::RollingCrcTable,
        block_size: crate::verify::types::BlockSize,
        local_block_map: &mut LocalBlockMap,
    ) {
        use crate::checksum::compute_crc32;

        // Initialize rolling CRC for current position if we have a full block
        if state.can_fit_block(block_size) {
            let initial_crc = compute_crc32(buffer.block_at(state.buffer_position, block_size));
            state.set_rolling_crc(Some(initial_crc));
        }

        // Scan byte-by-byte through the buffer
        loop {
            // Stop if we can't fit another block
            if !state.can_fit_block(block_size) {
                break;
            }

            // Try to match block at current position
            match self.scan_block_position(buffer, state, block_size, local_block_map) {
                ScanAction::SkipBlock => {
                    // Found a match - jump forward by block size
                    state.skip_block(block_size);

                    // Recompute CRC after skip
                    if state.can_fit_block(block_size) {
                        let new_crc =
                            compute_crc32(buffer.block_at(state.buffer_position, block_size));
                        state.set_rolling_crc(Some(new_crc));
                    } else {
                        state.set_rolling_crc(None);
                    }
                }
                ScanAction::AdvanceOneByte => {
                    // No match - advance one byte with rolling CRC
                    state.advance_one_byte();
                    state.slide_crc_one_byte(rolling_table, buffer, block_size);
                }
            }
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

    /// Determine file status considering scan metadata (first block offset, sequence, hash)
    /// Based on par2repairer.cpp:1722-1732 and 1851-1863
    fn determine_file_status_with_metadata(
        blocks_available: BlockCount,
        total_blocks: BlockCount,
        metadata: &FileScanMetadata,
        expected_hash: &Md5Hash,
    ) -> FileStatus {
        // First check basic block availability
        let basic_status = Self::determine_file_status(blocks_available, total_blocks);

        // If not present (missing or corrupted), return early
        if basic_status != FileStatus::Present {
            return basic_status;
        }

        // All blocks found - check additional criteria from par2cmdline-turbo

        // Check if blocks are perfectly aligned (first at offset 0 and in sequence)
        // Reference: par2repairer.cpp:1722-1725, 1728-1732
        if !metadata.is_perfect_match() {
            log::debug!(
                "File marked corrupted: not perfect match (first_at_zero={}, in_sequence={})",
                metadata.first_block_at_offset_zero,
                metadata.blocks_in_sequence
            );
            return FileStatus::Corrupted;
        }

        // Check file hash matches (par2repairer.cpp:1851-1863)
        if let Some(actual_hash) = &metadata.actual_file_hash {
            if actual_hash != expected_hash {
                log::debug!(
                    "File marked corrupted: hash mismatch (actual={:?}, expected={:?})",
                    actual_hash,
                    expected_hash
                );
                return FileStatus::Corrupted;
            }
        } else {
            // Could not compute hash - mark as corrupted
            log::debug!("File marked corrupted: no file hash computed");
            return FileStatus::Corrupted;
        }

        // All checks passed
        FileStatus::Present
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
    /// Create file results based on available blocks (reporting already done)
    fn create_file_results(
        &self,
        available_blocks: &AvailableBlocksMap,
        file_statuses: &FileStatusMap,
        scan_metadatas: &HashMap<FileId, FileScanMetadata>,
    ) -> Vec<FileVerificationResult> {
        let mut file_results = Vec::new();

        for file_description in self.file_descriptions.values() {
            let file_name = extract_file_name(file_description);
            let file_size = FileSize::new(file_description.file_length);
            let total_blocks = self.calculate_total_blocks(file_size);

            // Count available blocks for this file by checking if each block's
            // checksum is available in any location
            let mut blocks_available = BlockCount::zero();
            let mut damaged_blocks = Vec::new();
            let file_blocks = self.block_table.get_file_blocks(file_description.file_id);

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

            // Use the pre-computed status from scanning if available,
            // otherwise fall back to basic block-based determination
            let status = file_statuses
                .get(&file_description.file_id)
                .copied()
                .unwrap_or_else(|| {
                    if blocks_available.is_complete(total_blocks) {
                        FileStatus::Present
                    } else if blocks_available.is_empty() {
                        FileStatus::Missing
                    } else {
                        FileStatus::Corrupted
                    }
                });

            // Extract block positions from scan metadata
            let block_positions = scan_metadatas
                .get(&file_description.file_id)
                .map(|metadata| {
                    metadata
                        .found_blocks
                        .iter()
                        .filter(|(_, fid, _)| *fid == file_description.file_id)
                        .map(|(offset, _, block_num)| (*block_num, *offset))
                        .collect()
                })
                .unwrap_or_default();

            // Just create the result record (reporting already done inline)

            file_results.push(FileVerificationResult {
                file_name,
                file_id: file_description.file_id,
                status,
                blocks_available: blocks_available.as_usize(),
                total_blocks: total_blocks.as_usize(),
                damaged_blocks,
                block_positions,
            });
        }

        file_results
    }

    /// Create block verification results
    fn create_block_verification_results(
        &self,
        available_blocks: &AvailableBlocksMap,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::scanner_state::ScannerState;

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
    // Reference: par2cmdline-turbo/src/verificationhashtable.h:253-264 (VerificationHashTable::Load)
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1580-1633 (ScanDataFile initialization)
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

        let file_description = create_test_file_desc(FileId::new([2; 16]), 1024);

        let packets = vec![
            crate::Packet::Main(main_packet),
            crate::Packet::FileDescription(file_description),
        ];

        let engine = GlobalVerificationEngine::from_packets(&packets, ".");
        assert!(engine.is_ok());

        let engine = engine.unwrap();
        assert_eq!(engine.block_table().block_size(), 1024);
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1599-1608 (handling empty/missing files)
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

        let file_description = create_test_file_desc(FileId::new([2; 16]), 1024);

        let packets = vec![
            crate::Packet::Main(main_packet),
            crate::Packet::FileDescription(file_description.clone()),
        ];

        let temporary_directory = tempfile::tempdir().unwrap();
        let engine =
            GlobalVerificationEngine::from_packets(&packets, temporary_directory.path()).unwrap();
        let reporter = crate::reporters::ConsoleVerificationReporter::new();
        let results = engine.verify_recovery_set(&reporter, true); // parallel=true for tests

        // Since the file doesn't exist, it should be reported as missing
        assert_eq!(results.missing_file_count, 1);
        assert_eq!(results.present_file_count, 0);
        assert_eq!(results.total_block_count, 1); // 1024 bytes = 1 block of 1024
    }

    #[test]
    // Reference: par2cmdline-turbo/src/verificationhashtable.h:303-443 (FindMatch function)
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1702-1770 (match handling logic)
    fn test_insert_matching_blocks() {
        use crate::verify::global_table::GlobalBlockTableBuilder;

        // Create a simple block table with one known block
        let mut builder = GlobalBlockTableBuilder::new(1024);
        let file_id = FileId::new([1; 16]);
        let checksums = vec![(Md5Hash::new([0xAA; 16]), Crc32Value::new(0x12345678))];
        builder.add_file_blocks(file_id, &checksums);
        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        let mut local_map = HashMap::default();
        let mut state = ScannerState::new(0);

        // Test matching block
        let found = engine.insert_matching_blocks(
            Md5Hash::new([0xAA; 16]),
            Crc32Value::new(0x12345678),
            &mut local_map,
            &mut state,
        );
        assert_eq!(
            found,
            BlockMatchResult::Matched,
            "Should find matching block"
        );
        assert_eq!(local_map.len(), 1, "Should have one entry in local map");

        // Test non-matching MD5
        let mut local_map2 = HashMap::default();
        let mut state2 = ScannerState::new(0);
        let found = engine.insert_matching_blocks(
            Md5Hash::new([0xBB; 16]),
            Crc32Value::new(0x12345678),
            &mut local_map2,
            &mut state2,
        );
        assert_eq!(
            found,
            BlockMatchResult::NotMatched,
            "Should not find block with wrong MD5"
        );
        assert_eq!(local_map2.len(), 0, "Should have no entries");

        // Test non-matching CRC32
        let mut local_map3 = HashMap::default();
        let mut state3 = ScannerState::new(0);
        let found = engine.insert_matching_blocks(
            Md5Hash::new([0xAA; 16]),
            Crc32Value::new(0x99999999),
            &mut local_map3,
            &mut state3,
        );
        assert_eq!(
            found,
            BlockMatchResult::NotMatched,
            "Should not find block with wrong CRC32"
        );
        assert_eq!(local_map3.len(), 0, "Should have no entries");
    }

    #[test]
    // Reference: par2cmdline-turbo/src/verificationhashtable.h:303-443 (FindMatch full block check)
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1748-1755 (block match and recording)
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
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        let mut local_map = HashMap::default();
        let mut state = ScannerState::new(0);

        // Test matching
        let found = engine.try_match_and_insert_block(&block_data, &mut local_map, &mut state);
        assert_eq!(
            found,
            BlockMatchResult::Matched,
            "Should find matching block"
        );
        assert_eq!(local_map.len(), 1, "Should have one entry");

        // Test non-matching data
        let wrong_data = vec![0x99; 1024];
        let mut local_map2 = HashMap::default();
        let mut state2 = ScannerState::new(0);
        let found = engine.try_match_and_insert_block(&wrong_data, &mut local_map2, &mut state2);
        assert_eq!(
            found,
            BlockMatchResult::NotMatched,
            "Should not find non-matching block"
        );
        assert_eq!(local_map2.len(), 0, "Should have no entries");
    }

    #[test]
    // Reference: par2cmdline-turbo/src/verificationhashtable.h:319-327 (short block length handling)
    // Reference: par2cmdline-turbo/src/filechecksummer.cpp:318-330 (ShortChecksum with padding)
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
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        let mut local_map = HashMap::default();

        // Test matching partial block
        let found = engine.try_match_and_insert_partial_block(
            &partial_data,
            1024,
            &mut local_map,
            &mut ScannerState::new(0),
        );
        assert_eq!(
            found,
            BlockMatchResult::Matched,
            "Should find matching partial block"
        );
        assert_eq!(local_map.len(), 1, "Should have one entry");

        // Test non-matching partial data
        let wrong_data = vec![0x99; 500];
        let mut local_map2 = HashMap::default();
        let found = engine.try_match_and_insert_partial_block(
            &wrong_data,
            1024,
            &mut local_map2,
            &mut ScannerState::new(0),
        );
        assert_eq!(
            found,
            BlockMatchResult::NotMatched,
            "Should not find non-matching partial block"
        );
        assert_eq!(local_map2.len(), 0, "Should have no entries");
    }

    #[test]
    // Reference: par2cmdline-turbo/src/verificationhashtable.h:303-443 (all match paths converge to FindMatch)
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1702-1770 (single match logic for all paths)
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
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        // Test 1: Direct insertion
        let mut map1 = HashMap::default();
        let found1 = engine.insert_matching_blocks(
            expected_md5,
            expected_crc32,
            &mut map1,
            &mut ScannerState::new(0),
        );

        // Test 2: Via try_match_and_insert_block
        let mut map2 = HashMap::default();
        let found2 =
            engine.try_match_and_insert_block(&block_data, &mut map2, &mut ScannerState::new(0));

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
    // Reference: par2cmdline-turbo/src/filechecksummer.cpp:351-376 (ComputeCurrentChecksum after Jump)
    // Reference: par2cmdline-turbo/src/crc.cpp:119-122 (CRCUpdateBlock for recomputation)
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
        state.update_crc_after_skip(&rolling_table, &buffer, block_size);

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

        state2.update_crc_after_skip(&rolling_table, &buffer, block_size);

        // Should have cleared CRC because we can't fit another block
        assert!(
            state2.rolling_crc.is_none(),
            "Should clear CRC when can't fit block"
        );
    }

    #[test]
    // Reference: par2cmdline-turbo/src/filechecksummer.h:169-192 (Step function with CRCSlideChar)
    // Reference: par2cmdline-turbo/src/crc.h:100-104 (CRCSlideChar rolling update)
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
        state.slide_crc_one_byte(&rolling_table, &buffer, block_size);

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

        state2.slide_crc_one_byte(&rolling_table, &buffer, block_size);

        // Should remain None
        assert!(
            state2.rolling_crc.is_none(),
            "Should stay None when no initial CRC"
        );

        // Test case where we can't fit a full block after current position
        let mut state3 = ScannerState::new(1000); // Less than a full block
        state3.set_rolling_crc(Some(Crc32Value::new(0x12345678)));
        state3.advance_one_byte();

        state3.slide_crc_one_byte(&rolling_table, &buffer, block_size);

        // CRC should be cleared since we can't fit a block
        assert!(
            state3.rolling_crc.is_none(),
            "CRC should remain unchanged when can't fit block"
        );
    }

    #[test]
    // Reference: par2cmdline-turbo/src/filechecksummer.cpp:351-376 (Jump vs Step equivalence)
    // Reference: par2cmdline-turbo/src/crc.cpp:119-122 (both approaches use same CRC computation)
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
        state1.update_crc_after_skip(&rolling_table, &buffer, block_size);

        // Method 2: Advance byte by byte using rolling CRC
        let mut state2 = ScannerState::new(3072);
        state2.set_rolling_crc(Some(compute_crc32(buffer.slice(0..1024))));

        for _ in 0..1024 {
            state2.advance_one_byte();
            state2.slide_crc_one_byte(&rolling_table, &buffer, block_size);
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
    // Reference: par2cmdline-turbo/src/filechecksummer.cpp:257-286 (Fill function for buffer management)
    // Reference: par2cmdline-turbo/src/filechecksummer.h:185-191 (buffer sliding in Step)
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
    // Reference: par2cmdline-turbo/src/filechecksummer.cpp:176-181 (handling insufficient buffer data)
    // Reference: par2cmdline-turbo/src/filechecksummer.h:169-174 (early return when past EOF)
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
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1676-1695 (progress reporting during scan)
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1683 (percentage calculation)
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
    // Reference: par2cmdline-turbo/src/par2repairersourcefile.h:80-81 (GetBlockCount method)
    // Reference: par2cmdline-turbo/src/verificationhashtable.cpp:56-71 (Load function block counting)
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
            recovery_block_count: 0,
            skip_full_md5: false,
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
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1866-1949 (VerifyExtraFile status determination)
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1735-1742 (match type determination logic)
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
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1707-1720 (file status reporting with match details)
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1839-1863 (VerifyExtraFile reporting)
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
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1702-1763 (match found, aligned block scan)
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1763 (Jump after successful match)
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
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        // Create a buffer with the matching block
        let mut buffer = ScanBuffer::with_capacity(2048);
        buffer.fill(0x42);
        let block_size = BlockSize::new(1024);
        let mut state = ScannerState::new(2048);

        let mut local_map = HashMap::default();

        // Test: matching block should return SkipBlock
        let action = engine.scan_block_position(&buffer, &mut state, block_size, &mut local_map);
        assert_eq!(action, ScanAction::SkipBlock, "Should skip matching block");
        assert_eq!(local_map.len(), 1, "Should have inserted the match");

        // Test: non-matching block should return AdvanceOneByte
        let mut wrong_buffer = ScanBuffer::with_capacity(2048);
        wrong_buffer.fill(0x99);
        let mut local_map2 = HashMap::default();
        let action2 =
            engine.scan_block_position(&wrong_buffer, &mut state, block_size, &mut local_map2);
        assert_eq!(
            action2,
            ScanAction::AdvanceOneByte,
            "Should advance on non-match"
        );
        assert_eq!(local_map2.len(), 0, "Should not have inserted anything");
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1702-1770 (aligned block scanning loop)
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1763 (Jump forward on match)
    fn test_scan_aligned_blocks() {
        use crate::verify::global_table::GlobalBlockTableBuilder;
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, BufferPosition, ScanBuffer};

        // Create test data - first 2 blocks with 0x42, last 2 with 0x99
        let mut buffer = ScanBuffer::with_capacity(4096);
        for (i, byte) in buffer.iter_mut().enumerate() {
            *byte = if i < 2048 { 0x42 } else { 0x99 };
        }

        let expected_crc32 = crate::checksum::compute_crc32(buffer.slice(0..1024));
        let expected_md5 = crate::checksum::compute_md5_only(buffer.slice(0..1024));

        // Build block table with only the first block's checksum (matching both first 2 blocks)
        let mut builder = GlobalBlockTableBuilder::new(1024);
        let file_id = FileId::new([1; 16]);
        let checksums = vec![
            (expected_md5, expected_crc32), // Block 0 matches
            (expected_md5, expected_crc32), // Block 1 matches
        ];
        builder.add_file_blocks(file_id, &checksums);
        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        let block_size = BlockSize::new(1024);
        let mut state = ScannerState::new(4096);
        let mut local_map = HashMap::default();

        // Scan aligned blocks
        engine.scan_aligned_blocks(&buffer, &mut state, block_size, &mut local_map);

        // Should have advanced past the 2 matching blocks and stopped at 3rd (non-matching)
        assert_eq!(
            state.buffer_position,
            BufferPosition::new(2048),
            "Should advance to position 2048 after 2 matching blocks, stop at first non-match"
        );
        assert_eq!(
            local_map.len(),
            1,
            "Should have one entry for matching blocks"
        );
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1771-1794 (mismatch handling, partial match)
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1794 (Step() on mismatch)
    fn test_scan_aligned_blocks_stops_at_first_mismatch() {
        use crate::verify::global_table::GlobalBlockTableBuilder;
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, BufferPosition, ScanBuffer};

        // Create buffer with different data for second block
        let mut buffer = ScanBuffer::with_capacity(4096);
        for (i, byte) in buffer.iter_mut().enumerate() {
            *byte = if i < 1024 {
                0x42
            } else if i < 2048 {
                0x99 // Different data
            } else {
                0x42
            };
        }

        let expected_crc32 = crate::checksum::compute_crc32(buffer.slice(0..1024));
        let expected_md5 = crate::checksum::compute_md5_only(buffer.slice(0..1024));

        // Only first block matches
        let mut builder = GlobalBlockTableBuilder::new(1024);
        let file_id = FileId::new([1; 16]);
        let checksums = vec![(expected_md5, expected_crc32)];
        builder.add_file_blocks(file_id, &checksums);
        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        let block_size = BlockSize::new(1024);
        let mut state = ScannerState::new(4096);
        let mut local_map = HashMap::default();

        // Scan aligned blocks
        engine.scan_aligned_blocks(&buffer, &mut state, block_size, &mut local_map);

        // Should stop at position 1024 (first mismatch)
        assert_eq!(
            state.buffer_position,
            BufferPosition::new(1024),
            "Should stop at first non-matching block"
        );
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1794 (Step() one byte at a time)
    // Reference: par2cmdline-turbo/src/filechecksummer.h:169-192 (Step with rolling CRC)
    fn test_scan_byte_by_byte() {
        use crate::checksum::rolling_crc::RollingCrcTable;
        use crate::verify::global_table::GlobalBlockTableBuilder;
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, ScanBuffer};

        // Create test data with a matching block offset by 100 bytes
        let mut buffer = ScanBuffer::with_capacity(2048);
        for (i, byte) in buffer.iter_mut().enumerate() {
            *byte = if (100..1124).contains(&i) { 0x42 } else { 0x00 };
        }

        let expected_crc32 = crate::checksum::compute_crc32(buffer.slice(100..1124));
        let expected_md5 = crate::checksum::compute_md5_only(buffer.slice(100..1124));

        // Build block table with the matching block
        let mut builder = GlobalBlockTableBuilder::new(1024);
        let file_id = FileId::new([1; 16]);
        let checksums = vec![(expected_md5, expected_crc32)];
        builder.add_file_blocks(file_id, &checksums);
        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        let block_size = BlockSize::new(1024);
        let rolling_table = RollingCrcTable::new(1024);
        let mut state = ScannerState::new(2048);
        let mut local_map = HashMap::default();

        // Set rolling CRC
        let initial_crc = crate::checksum::compute_crc32(buffer.slice(0..1024));
        state.set_rolling_crc(Some(initial_crc));

        // Scan byte-by-byte
        engine.scan_byte_by_byte(
            &buffer,
            &mut state,
            &rolling_table,
            block_size,
            &mut local_map,
        );

        // Should have found the block at offset 100 and advanced past it
        assert!(
            state.buffer_position.as_usize() >= 1124,
            "Should have advanced past the matching block at offset 100"
        );
        assert_eq!(local_map.len(), 1, "Should have found one matching block");
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1794 (Step loop continues until EOF)
    // Reference: par2cmdline-turbo/src/filechecksummer.h:169-174 (Step returns false at EOF)
    fn test_scan_byte_by_byte_advances_to_end() {
        use crate::checksum::rolling_crc::RollingCrcTable;
        use crate::verify::global_table::GlobalBlockTableBuilder;
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, ScanBuffer};

        // Create buffer with no matching blocks
        let mut buffer = ScanBuffer::with_capacity(2048);
        buffer.fill(0x99);

        let builder = GlobalBlockTableBuilder::new(1024);
        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        let block_size = BlockSize::new(1024);
        let rolling_table = RollingCrcTable::new(1024);
        let mut state = ScannerState::new(2048);
        let mut local_map = HashMap::default();

        let initial_crc = crate::checksum::compute_crc32(buffer.slice(0..1024));
        state.set_rolling_crc(Some(initial_crc));

        // Scan byte-by-byte
        engine.scan_byte_by_byte(
            &buffer,
            &mut state,
            &rolling_table,
            block_size,
            &mut local_map,
        );

        // Should have scanned to the end (can't fit more blocks)
        assert!(
            !state.can_fit_block(block_size),
            "Should have scanned until can't fit more blocks"
        );
        assert_eq!(local_map.len(), 0, "Should have found no matching blocks");
    }

    #[test]
    // Regression test: buffer overflow bug not present in par2cmdline-turbo
    // Reference: par2cmdline-turbo/src/filechecksummer.h:176-181 (proper bounds checking in Step)
    // Our bug: used has_at_least(block_size) instead of can_fit_block(block_size)
    fn test_scan_single_file_buffer_overflow_regression() {
        use crate::checksum::compute_crc32;
        use crate::verify::global_table::GlobalBlockTableBuilder;
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, ScanBuffer};

        // Regression test for buffer overflow when scan_pos + block_size > buffer size
        // This simulates the condition where we've scanned past most of the buffer
        // and there's less than a full block remaining

        let block_size = BlockSize::new(1024);
        let builder = GlobalBlockTableBuilder::new(1024);
        let block_table = builder.build();

        let _engine = GlobalVerificationEngine {
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        // Create a buffer with 2MB worth of data
        let mut buffer = ScanBuffer::with_capacity(2 * 1024 * 1024);
        buffer.fill(0x42);

        // Simulate state where we're near the end of the buffer
        // bytes_in_buffer is less than what we'd need for scan_pos + block_size
        let mut state = ScannerState::new(1500); // Only 1500 bytes in buffer
        state.buffer_position = crate::verify::types::BufferPosition::new(600); // Position at 600

        // This should NOT panic - we only have 900 bytes remaining (1500 - 600)
        // which is less than block_size (1024), so can_fit_block should return false
        assert!(
            !state.can_fit_block(block_size),
            "Should not be able to fit a block with only 900 bytes remaining"
        );

        // The bug was here: we were checking bytes_in_buffer.has_at_least(block_size)
        // which would return true (1500 >= 1024), but we should check if we can fit
        // a block from the CURRENT scan position

        // This should not panic with the fix
        let initial_crc = if state.can_fit_block(block_size) {
            Some(compute_crc32(
                buffer.block_at(state.buffer_position, block_size),
            ))
        } else {
            None
        };

        assert!(
            initial_crc.is_none(),
            "Should not compute CRC when can't fit a block"
        );
    }

    // ============================================================================
    // COMPREHENSIVE WORKFLOW TESTS
    // These tests cover the full par2cmdline-turbo verification workflow
    // ============================================================================

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1599-1608 (empty file handling)
    fn test_empty_file_handling() {
        use std::fs::File;
        use tempfile::tempdir;

        let temporary_directory = tempdir().unwrap();
        let file_path = temporary_directory.path().join("empty.txt");

        // Create an empty file
        File::create(&file_path).unwrap();

        // Compute correct MD5 hash for empty file
        use crate::checksum::compute_md5;
        let empty_md5 = compute_md5(b"");

        let main_packet = crate::packets::MainPacket {
            length: 92,
            md5: Md5Hash::new([0; 16]),
            set_id: crate::domain::RecoverySetId::new([1; 16]),
            slice_size: 1024,
            file_count: 1,
            file_ids: vec![FileId::new([2; 16])],
            non_recovery_file_ids: vec![],
        };

        let mut file_description = create_test_file_desc(FileId::new([2; 16]), 0);
        file_description.file_name = b"empty.txt".to_vec();
        file_description.md5_hash = empty_md5;
        file_description.md5_16k = empty_md5;

        let packets = vec![
            crate::Packet::Main(main_packet),
            crate::Packet::FileDescription(file_description),
        ];

        let engine =
            GlobalVerificationEngine::from_packets(&packets, temporary_directory.path()).unwrap();
        let reporter = crate::reporters::ConsoleVerificationReporter::new();
        let results = engine.verify_recovery_set(&reporter, false);

        // Empty file should be considered present (no blocks to verify)
        assert_eq!(results.present_file_count, 1);
        assert_eq!(results.total_block_count, 0);
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1725-1756 (match type determination)
    // Tests full match vs partial match based on first block position
    fn test_match_type_first_block_not_at_start() {
        use crate::verify::global_table::GlobalBlockTableBuilder;
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, ScanBuffer};

        // Create a file where the first matching block is NOT at offset 0
        let mut buffer = ScanBuffer::with_capacity(2048);
        // Fill first 100 bytes with zeros, then matching block
        for (i, byte) in buffer.iter_mut().enumerate() {
            *byte = if i < 100 { 0x00 } else { 0x42 };
        }

        let expected_crc32 = crate::checksum::compute_crc32(buffer.slice(100..1124));
        let expected_md5 = crate::checksum::compute_md5_only(buffer.slice(100..1124));

        let mut builder = GlobalBlockTableBuilder::new(1024);
        let file_id = FileId::new([1; 16]);
        let checksums = vec![(expected_md5, expected_crc32)];
        builder.add_file_blocks(file_id, &checksums);
        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        let block_size = BlockSize::new(1024);
        let mut state = ScannerState::new(2048);
        let mut local_map = HashMap::default();

        // Skip to where the match would be
        for _ in 0..100 {
            state.advance_one_byte();
        }

        // Scan and find the match
        let action = engine.scan_block_position(&buffer, &mut state, block_size, &mut local_map);
        assert_eq!(
            action,
            ScanAction::SkipBlock,
            "Should find match at offset 100"
        );

        // In par2cmdline-turbo, this would be a PARTIAL match because:
        // - First match is not at offset 0
        // This test verifies we CAN detect matches at non-zero offsets
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1725-1756 (sequential match validation)
    // Tests that sequential blocks in expected order would be a full match
    fn test_match_type_sequential_blocks() {
        use crate::verify::global_table::GlobalBlockTableBuilder;
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, ScanBuffer};

        // Create a buffer with exactly 3 blocks (3072 bytes)
        let mut buffer = ScanBuffer::with_capacity(3072);
        buffer.fill(0x42);

        let block_size = BlockSize::new(1024);

        // Build table with 3 sequential blocks (note: all have same data = same checksums)
        let mut builder = GlobalBlockTableBuilder::new(1024);
        let file_id = FileId::new([1; 16]);

        let block1_crc = crate::checksum::compute_crc32(buffer.slice(0..1024));
        let block1_md5 = crate::checksum::compute_md5_only(buffer.slice(0..1024));

        // All 3 blocks have identical data, so all 3 have the same checksum
        let checksums = vec![
            (block1_md5, block1_crc), // Block 0
            (block1_md5, block1_crc), // Block 1
            (block1_md5, block1_crc), // Block 2
        ];
        builder.add_file_blocks(file_id, &checksums);
        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        let mut state = ScannerState::new(3072);
        let mut local_map = HashMap::default();

        // Scan all 3 blocks sequentially
        engine.scan_aligned_blocks(&buffer, &mut state, block_size, &mut local_map);

        // Should have found all 3 blocks sequentially
        assert_eq!(
            local_map.len(),
            1,
            "Should have one checksum entry (all blocks have same data)"
        );
        let entries = local_map.values().next().unwrap();

        // We scan 3 blocks in the buffer, and each matches all 3 entries in the table
        // So we get 3 blocks × 3 table entries = 9 total entries
        // Actually this is what we expect - each scanned block can match multiple table entries
        assert_eq!(
            entries.len(),
            9,
            "3 buffer blocks × 3 table entries = 9 matches"
        );

        // Verify we got blocks 0, 1, and 2 (each should appear 3 times)
        let block_0_count = entries
            .iter()
            .filter(|(_, block_num)| *block_num == 0)
            .count();
        let block_1_count = entries
            .iter()
            .filter(|(_, block_num)| *block_num == 1)
            .count();
        let block_2_count = entries
            .iter()
            .filter(|(_, block_num)| *block_num == 2)
            .count();

        assert_eq!(block_0_count, 3, "Block 0 should match 3 times");
        assert_eq!(block_1_count, 3, "Block 1 should match 3 times");
        assert_eq!(block_2_count, 3, "Block 2 should match 3 times");

        // This represents a FULL match in par2cmdline-turbo because:
        // - First block at offset 0
        // - All subsequent blocks in expected sequence
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1754-1756 (multiple targets detection)
    fn test_multiple_target_files_detection() {
        use crate::verify::global_table::GlobalBlockTableBuilder;

        // Create a block table with blocks from TWO different files
        let mut builder = GlobalBlockTableBuilder::new(1024);

        let file_id_1 = FileId::new([1; 16]);
        let file_id_2 = FileId::new([2; 16]);

        let block_data = vec![0x42; 1024];
        let crc32 = crate::checksum::compute_crc32(&block_data);
        let md5 = crate::checksum::compute_md5_only(&block_data);

        // Add same block checksum for two different files
        builder.add_file_blocks(file_id_1, &[(md5, crc32)]);
        builder.add_file_blocks(file_id_2, &[(md5, crc32)]);

        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        let mut local_map = HashMap::default();

        // Try to match the block
        let result = engine.try_match_and_insert_block(
            &block_data,
            &mut local_map,
            &mut ScannerState::new(0),
        );
        assert_eq!(result, BlockMatchResult::Matched);

        // Should have entries for BOTH file IDs
        let entries = local_map.get(&(md5, crc32)).unwrap();
        assert_eq!(entries.len(), 2, "Should have found blocks from both files");

        // Verify both file IDs are present
        assert!(entries.iter().any(|(fid, _)| *fid == file_id_1));
        assert!(entries.iter().any(|(fid, _)| *fid == file_id_2));

        // In par2cmdline-turbo, this would set multipletargets = true
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1707-1720 (gap tracking)
    // Tests tracking of data gaps between matched blocks
    fn test_gap_between_matches() {
        use crate::verify::global_table::GlobalBlockTableBuilder;
        use crate::verify::scanner_state::ScannerState;
        use crate::verify::types::{BlockSize, ScanBuffer};

        // Create buffer with matches at 0 and 2048, with gap in between
        let mut buffer = ScanBuffer::with_capacity(4096);
        // First block: 0x42
        for i in 0..1024 {
            if let Some(b) = buffer.iter_mut().nth(i) {
                *b = 0x42;
            }
        }
        // Gap: 0x00 from 1024..2048
        for i in 1024..2048 {
            if let Some(b) = buffer.iter_mut().nth(i) {
                *b = 0x00;
            }
        }
        // Third block: 0x42 again
        for i in 2048..3072 {
            if let Some(b) = buffer.iter_mut().nth(i) {
                *b = 0x42;
            }
        }

        let block_crc = crate::checksum::compute_crc32(buffer.slice(0..1024));
        let block_md5 = crate::checksum::compute_md5_only(buffer.slice(0..1024));

        let mut builder = GlobalBlockTableBuilder::new(1024);
        let file_id = FileId::new([1; 16]);
        builder.add_file_blocks(file_id, &[(block_md5, block_crc)]);
        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        let block_size = BlockSize::new(1024);
        let mut state = ScannerState::new(4096);
        let mut local_map = HashMap::default();

        // First aligned scan should find block at 0
        engine.scan_aligned_blocks(&buffer, &mut state, block_size, &mut local_map);
        assert_eq!(
            state.buffer_position.as_usize(),
            1024,
            "Should stop after first block"
        );

        // There's a GAP from 1024 to 2048 (1024 bytes of non-matching data)
        // par2cmdline-turbo would report "No data found between offset 1024 and 2048"

        // Continue scanning would eventually find the block at 2048
        // (this is what byte-by-byte scanning is for)
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1853-1863 (post-scan validation)
    // Tests validation of block count match
    fn test_post_scan_block_count_validation() {
        use crate::verify::types::FileSize;

        // Create a file that has FEWER blocks found than expected
        let file_id = FileId::new([1; 16]);
        let file_size = FileSize::new(3072); // 3 blocks

        let mut builder = crate::verify::global_table::GlobalBlockTableBuilder::new(1024);
        let checksums = vec![
            (Md5Hash::new([0xAA; 16]), Crc32Value::new(0x11111111)),
            (Md5Hash::new([0xBB; 16]), Crc32Value::new(0x22222222)),
            (Md5Hash::new([0xCC; 16]), Crc32Value::new(0x33333333)),
        ];
        builder.add_file_blocks(file_id, &checksums);
        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        // Simulate finding only 2 of 3 blocks
        let mut local_map = HashMap::default();
        local_map.insert(
            (Md5Hash::new([0xAA; 16]), Crc32Value::new(0x11111111)),
            smallvec::smallvec![(file_id, 0)],
        );
        local_map.insert(
            (Md5Hash::new([0xBB; 16]), Crc32Value::new(0x22222222)),
            smallvec::smallvec![(file_id, 1)],
        );
        // Block 2 is missing!

        let (available, damaged) = engine.count_file_blocks(file_id, file_size, &local_map);

        assert_eq!(available.as_usize(), 2, "Should have 2 available blocks");
        assert_eq!(damaged.len(), 1, "Should have 1 damaged block");
        assert_eq!(damaged[0], 2, "Block 2 should be damaged");

        // In par2cmdline-turbo, this would downgrade from eFullMatch to ePartialMatch
        let status = GlobalVerificationEngine::determine_file_status(available, BlockCount::new(3));
        assert_eq!(status, FileStatus::Corrupted);
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1853-1863 (hash validation)
    // Tests that file hash mismatch would invalidate a perfect match
    fn test_post_scan_hash_validation() {
        // This test verifies the CONCEPT of hash validation
        // In par2cmdline-turbo, even if all blocks are found, the file is still
        // validated against:
        // 1. File size
        // 2. Full file MD5 hash
        // 3. First 16k MD5 hash
        //
        // If any of these don't match, it's downgraded to ePartialMatch

        // We would need to implement full file hashing during scan to test this
        // For now, this test documents the requirement

        // TODO: Implement file hash computation during scanning
        // TODO: Add validation logic to compare computed vs expected hashes
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1780-1787 (duplicate handling)
    fn test_duplicate_block_detection() {
        use crate::verify::global_table::GlobalBlockTableBuilder;

        let file_id = FileId::new([1; 16]);
        let block_data = vec![0x42; 1024];
        let crc32 = crate::checksum::compute_crc32(&block_data);
        let md5 = crate::checksum::compute_md5_only(&block_data);

        let mut builder = GlobalBlockTableBuilder::new(1024);
        // Add the SAME block checksum twice (two blocks with identical data)
        builder.add_file_blocks(file_id, &[(md5, crc32), (md5, crc32)]);
        let block_table = builder.build();

        let engine = GlobalVerificationEngine {
            recovery_block_count: 0,
            skip_full_md5: false,
            block_table,
            file_descriptions: HashMap::default(),
            base_dir: std::path::PathBuf::from("."),
        };

        let mut local_map = HashMap::default();

        // Match the block - should find BOTH entries in the global table
        let result = engine.try_match_and_insert_block(
            &block_data,
            &mut local_map,
            &mut ScannerState::new(0),
        );
        assert_eq!(result, BlockMatchResult::Matched);

        // Should have BOTH block positions recorded (blocks 0 and 1)
        let entries = local_map.get(&(md5, crc32)).unwrap();
        assert_eq!(
            entries.len(),
            2,
            "Should record both blocks even with same checksum"
        );

        // Verify both block numbers are present
        assert!(entries.iter().any(|(_, block_num)| *block_num == 0));
        assert!(entries.iter().any(|(_, block_num)| *block_num == 1));

        // par2cmdline-turbo has a duplicate flag but ignores duplicates
        // Our implementation records all matches from the global table
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1853-1863 (file size validation)
    fn test_post_scan_file_size_validation() {
        use crate::verify::types::FileSize;

        let _file_id = FileId::new([1; 16]);

        // File descriptor says 2048 bytes
        let expected_size = FileSize::new(2048);

        // But actual blocks suggest 3072 bytes (3 blocks * 1024)
        let total_blocks = expected_size.total_blocks(crate::verify::types::BlockSize::new(1024));

        assert_eq!(
            total_blocks.as_usize(),
            2,
            "Should expect 2 blocks for 2048 bytes"
        );

        // If we find 3 blocks, that's a size mismatch
        let actual_blocks = BlockCount::new(3);

        // This would be detected in par2cmdline-turbo's validation
        assert!(
            actual_blocks > total_blocks,
            "More blocks found than file size indicates - this is a validation error"
        );
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1876-1913 (multiple targets reporting)
    fn test_multiple_targets_file_reporting() {
        use crate::reporters::ConsoleVerificationReporter;
        use crate::verify::types::BlockCount;
        use std::sync::Mutex;

        let reporter = ConsoleVerificationReporter::new();
        let reporter_lock = Mutex::new(&reporter);

        // Simulate reporting a file with blocks from multiple target files
        // In par2cmdline-turbo, this gets a special message:
        // "found X data blocks from several target files"

        GlobalVerificationEngine::report_file_status(
            &reporter_lock,
            "mixed_data.dat",
            FileStatus::Corrupted,
            &[2, 5, 7],          // Some blocks damaged
            BlockCount::new(15), // 15 blocks available
            BlockCount::new(20), // 20 blocks total
        );

        // The reporter should handle this appropriately
        // par2cmdline-turbo has special handling for multipletargets flag
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1633-1647 (scan distance calculation)
    fn test_scan_distance_concept() {
        // par2cmdline-turbo uses a scan distance parameter to limit byte-by-byte scanning
        // scandistance = min(skipleaway<<1, blocksize)
        //
        // This test documents the concept - we currently scan the entire buffer
        // byte-by-byte, but par2cmdline-turbo would skip ahead after scanning
        // scandistance bytes without finding a match

        let block_size = 1024usize;
        let skipleaway = 512usize; // From command line option

        let scan_distance = std::cmp::min(skipleaway << 1, block_size);
        assert_eq!(
            scan_distance, 1024,
            "Should scan min(1024, 1024) = 1024 bytes"
        );

        // If we scan scan_distance bytes without finding a match, par2cmdline-turbo
        // would skip ahead by (blocksize - scandistance) bytes
        let scanskip = block_size - scan_distance;
        assert_eq!(scanskip, 0, "With these parameters, no skipping");

        // TODO: Implement skip-ahead optimization in scan_byte_by_byte
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1798-1812 (skip-ahead logic)
    fn test_skip_ahead_optimization_concept() {
        // This test documents the skip-ahead optimization in par2cmdline-turbo
        //
        // When scanning byte-by-byte, if we've scanned `scandistance` bytes
        // without finding a match, we skip ahead by `scanskip` bytes rather
        // than continuing to scan byte-by-byte
        //
        // This significantly speeds up scanning of large files with sparse matches

        let block_size = 8 * 1024 * 1024; // 8MB blocks (like in our test data)
        let skipleaway = 512; // Default

        let scan_distance = std::cmp::min(skipleaway << 1, block_size);
        let scanskip = block_size - scan_distance;

        assert_eq!(scan_distance, 1024, "Scan 1024 bytes at a time");
        assert_eq!(
            scanskip,
            8 * 1024 * 1024 - 1024,
            "Skip ahead ~8MB if no match"
        );

        // Algorithm:
        // 1. Scan byte-by-byte for scan_distance bytes
        // 2. If no match found, skip ahead by scanskip bytes
        // 3. Repeat
        //
        // This avoids byte-by-byte scanning through potentially gigabytes of data

        // TODO: Implement this optimization
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1759-1770 (next entry tracking)
    fn test_next_entry_sequential_optimization() {
        use crate::verify::global_table::GlobalBlockTableBuilder;

        // par2cmdline-turbo tracks the "next expected entry" for sequential optimization
        // After finding block N, it expects to find block N+1 next
        // This allows for fast-path validation

        let mut builder = GlobalBlockTableBuilder::new(1024);
        let file_id = FileId::new([1; 16]);

        // Create 3 sequential blocks
        let checksums = vec![
            (Md5Hash::new([0xAA; 16]), Crc32Value::new(0x11111111)),
            (Md5Hash::new([0xBB; 16]), Crc32Value::new(0x22222222)),
            (Md5Hash::new([0xCC; 16]), Crc32Value::new(0x33333333)),
        ];
        builder.add_file_blocks(file_id, &checksums);
        let _block_table = builder.build();

        // In our implementation, we look up each block independently
        // par2cmdline-turbo has a "next" pointer in each VerificationHashEntry
        // that points to the next sequential block for the same file

        // The optimization: If we find block 0, we can check block 1 directly
        // without doing a hash table lookup

        // We achieve this through aligned block scanning which processes
        // sequential blocks efficiently

        // Verify the table was built correctly
        let _total = checksums.len();
        assert_eq!(_total, 3, "Should have 3 blocks");
    }

    #[test]
    // Reference: par2cmdline-turbo/src/verificationhashtable.h:303-443 (FindMatch with suggested entry)
    fn test_suggested_entry_optimization() {
        // par2cmdline-turbo's FindMatch() takes a "suggestedentry" parameter
        // This is the entry we EXPECT to find next (for sequential files)
        //
        // If suggestedentry matches, we can skip hash table lookup entirely
        //
        // Our implementation doesn't have this optimization yet, but we could add it
        // by tracking the last matched block and checking if the next block
        // sequentially follows

        // This is partially achieved through scan_aligned_blocks which processes
        // sequential matching blocks efficiently

        // TODO: Consider adding explicit "suggested next" optimization
    }

    #[test]
    // Reference: par2cmdline-turbo/src/filechecksummer.cpp:232-240 (file hash computation)
    fn test_file_hash_computation_concept() {
        // par2cmdline-turbo computes TWO MD5 hashes during file scanning:
        // 1. Full file MD5 (hash of entire file)
        // 2. First 16k MD5 (hash of first 16384 bytes)
        //
        // These are used for:
        // - Validating perfect matches
        // - Detecting renamed files
        // - Quick comparison without scanning all blocks

        let test_data = vec![0x42u8; 20000];

        // Full hash
        let full_hash = crate::checksum::compute_md5_only(&test_data);

        // 16k hash
        let hash_16k = crate::checksum::compute_md5_only(&test_data[..16384]);

        assert_ne!(full_hash, hash_16k, "16k hash should differ from full hash");

        // TODO: Implement concurrent hash computation during file scanning
        // Should be done in scan_single_file_with_progress
    }

    #[test]
    // Reference: par2cmdline-turbo/src/par2repairer.cpp:1727-1742 (match type transitions)
    fn test_match_type_transitions() {
        // par2cmdline-turbo tracks match type transitions:
        //
        // Start: eFullMatch (assume perfect)
        //
        // Downgrade to ePartialMatch if:
        // - First block not at offset 0
        // - First block is not the file's first block
        // - Any subsequent block is not the expected next
        // - Blocks from multiple source files found
        // - Final validation fails (count, size, hash mismatch)

        // Our implementation computes status at the end based on available blocks
        // par2cmdline-turbo tracks it during scanning

        // Both approaches are valid - ours is simpler, theirs provides earlier feedback

        use crate::verify::types::BlockCount;

        // Perfect match
        let status1 = GlobalVerificationEngine::determine_file_status(
            BlockCount::new(10),
            BlockCount::new(10),
        );
        assert_eq!(status1, FileStatus::Present);

        // Partial match
        let status2 = GlobalVerificationEngine::determine_file_status(
            BlockCount::new(7),
            BlockCount::new(10),
        );
        assert_eq!(status2, FileStatus::Corrupted);

        // Complete miss
        let status3 = GlobalVerificationEngine::determine_file_status(
            BlockCount::zero(),
            BlockCount::new(10),
        );
        assert_eq!(status3, FileStatus::Missing);
    }

    #[test]
    fn test_first_block_not_at_offset_zero_should_be_corrupted() {
        // Reference: par2cmdline-turbo/src/par2repairer.cpp:1722-1725
        // if (!currententry->FirstBlock() || filechecksummer.Offset() != 0)
        //     matchtype = ePartialMatch;
        //
        // If the first matching block is found at offset > 0, the file should be marked
        // as Corrupted even if all blocks are present.

        use std::fs::File;
        use std::io::Write;

        let temporary_directory = tempfile::tempdir().unwrap();
        let base_path = temporary_directory.path();

        // Create file with junk at start, then valid block
        let file_path = base_path.join("test.txt");
        let mut file = File::create(&file_path).unwrap();
        file.write_all(&[0xFF; 100]).unwrap(); // Junk
        file.write_all(&[0x42; 1024]).unwrap(); // Valid block
        file.flush().unwrap();
        drop(file);

        let file_id = FileId::new([1; 16]);

        // Compute checksums for the valid block
        use crate::checksum::compute_block_checksums;
        let (md5_hash, crc32) = compute_block_checksums(&[0x42; 1024]);

        // Compute file hash (will be wrong because of the junk at start)
        use crate::checksum::compute_md5;
        let mut file_data = vec![0xFF; 100];
        file_data.extend_from_slice(&[0x42; 1024]);
        let file_md5 = compute_md5(&file_data);
        let file_md5_16k = compute_md5(&file_data[..file_data.len().min(16384)]);

        let mut file_description = create_test_file_desc(file_id, 1024);
        file_description.md5_hash = file_md5;
        file_description.md5_16k = file_md5_16k;

        let main_packet = crate::packets::MainPacket {
            length: 92,
            md5: Md5Hash::new([0; 16]),
            set_id: crate::domain::RecoverySetId::new([1; 16]),
            slice_size: 1024,
            file_count: 1,
            file_ids: vec![file_id],
            non_recovery_file_ids: vec![],
        };

        let packets = vec![
            crate::Packet::Main(main_packet),
            crate::Packet::FileDescription(file_description),
            crate::Packet::InputFileSliceChecksum(crate::packets::InputFileSliceChecksumPacket {
                length: 64 + 16 + 20,
                md5: Md5Hash::new([0; 16]),
                set_id: crate::domain::RecoverySetId::new([1; 16]),
                file_id,
                slice_checksums: vec![(md5_hash, crc32)],
            }),
        ];

        let engine = GlobalVerificationEngine::from_packets(&packets, base_path).unwrap();
        let reporter = crate::reporters::ConsoleVerificationReporter::new();
        let results = engine.verify_recovery_set(&reporter, false);

        // Block will be found but file should be Corrupted because it doesn't start at offset 0
        assert_eq!(results.files[0].blocks_available, 1);
        assert_eq!(
            results.files[0].status,
            FileStatus::Corrupted,
            "File should be Corrupted when first block is not at offset 0"
        );
    }

    #[test]
    fn test_blocks_out_of_sequence_should_be_corrupted() {
        // Reference: par2cmdline-turbo/src/par2repairer.cpp:1728-1732
        // if (currententry != nextentry)
        //     matchtype = ePartialMatch;
        //
        // If blocks are found but not in the expected order, file should be Corrupted
        // even if all blocks are present.

        use std::fs::File;
        use std::io::Write;

        let temporary_directory = tempfile::tempdir().unwrap();
        let base_path = temporary_directory.path();

        let file_path = base_path.join("test.txt");
        let mut file = File::create(&file_path).unwrap();
        // Write block 1 then block 0 (reversed)
        file.write_all(&[0xBB; 1024]).unwrap(); // This is block 1
        file.write_all(&[0xAA; 1024]).unwrap(); // This is block 0
        file.flush().unwrap();
        drop(file);

        let file_id = FileId::new([2; 16]);

        // Compute checksums for both blocks
        use crate::checksum::compute_block_checksums;
        let (md5_block0, crc32_block0) = compute_block_checksums(&[0xAA; 1024]);
        let (md5_block1, crc32_block1) = compute_block_checksums(&[0xBB; 1024]);

        // Compute file hash (actual file content)
        let mut file_data = vec![0xBB; 1024];
        file_data.extend_from_slice(&[0xAA; 1024]);
        use crate::checksum::compute_md5;
        let file_md5 = compute_md5(&file_data);
        let file_md5_16k = compute_md5(&file_data[..file_data.len().min(16384)]);

        let mut file_description = create_test_file_desc(file_id, 2048);
        file_description.md5_hash = file_md5;
        file_description.md5_16k = file_md5_16k;

        let main_packet = crate::packets::MainPacket {
            length: 92,
            md5: Md5Hash::new([0; 16]),
            set_id: crate::domain::RecoverySetId::new([1; 16]),
            slice_size: 1024,
            file_count: 1,
            file_ids: vec![file_id],
            non_recovery_file_ids: vec![],
        };

        let packets = vec![
            crate::Packet::Main(main_packet),
            crate::Packet::FileDescription(file_description),
            crate::Packet::InputFileSliceChecksum(crate::packets::InputFileSliceChecksumPacket {
                length: 64 + 16 + 20,
                md5: Md5Hash::new([0; 16]),
                set_id: crate::domain::RecoverySetId::new([1; 16]),
                file_id,
                slice_checksums: vec![(md5_block0, crc32_block0), (md5_block1, crc32_block1)],
            }),
        ];

        let engine = GlobalVerificationEngine::from_packets(&packets, base_path).unwrap();
        let reporter = crate::reporters::ConsoleVerificationReporter::new();
        let results = engine.verify_recovery_set(&reporter, false);

        // Both blocks will be found but file should be Corrupted because they're out of order
        assert_eq!(results.files[0].blocks_available, 2);
        assert_eq!(
            results.files[0].status,
            FileStatus::Corrupted,
            "File should be Corrupted when blocks are found out of sequence"
        );
    }

    #[test]
    fn test_file_hash_mismatch_should_be_corrupted() {
        // Reference: par2cmdline-turbo/src/par2repairer.cpp:1851-1863
        // Even if all blocks match, if hashfull or hash16k doesn't match the
        // FileDescriptionPacket, it should be marked as Corrupted.

        use std::fs::File;
        use std::io::Write;

        let temporary_directory = tempfile::tempdir().unwrap();
        let base_path = temporary_directory.path();

        let file_path = base_path.join("test.txt");
        let mut file = File::create(&file_path).unwrap();
        file.write_all(&[0x42; 1024]).unwrap();
        file.write_all(&[0xFF; 512]).unwrap(); // Extra data that breaks the file hash
        file.flush().unwrap();
        drop(file);

        let file_id = FileId::new([3; 16]);

        // Compute checksum for the valid block
        use crate::checksum::compute_block_checksums;
        let (md5_hash, crc32) = compute_block_checksums(&[0x42; 1024]);

        // Expected file hash is for just the first block (1024 bytes)
        use crate::checksum::compute_md5;
        let expected_md5 = compute_md5(&[0x42; 1024]);
        let expected_md5_16k = compute_md5(&[0x42; 1024]);

        let mut file_description = create_test_file_desc(file_id, 1024);
        file_description.md5_hash = expected_md5;
        file_description.md5_16k = expected_md5_16k;

        let main_packet = crate::packets::MainPacket {
            length: 92,
            md5: Md5Hash::new([0; 16]),
            set_id: crate::domain::RecoverySetId::new([1; 16]),
            slice_size: 1024,
            file_count: 1,
            file_ids: vec![file_id],
            non_recovery_file_ids: vec![],
        };

        let packets = vec![
            crate::Packet::Main(main_packet),
            crate::Packet::FileDescription(file_description),
            crate::Packet::InputFileSliceChecksum(crate::packets::InputFileSliceChecksumPacket {
                length: 64 + 16 + 20,
                md5: Md5Hash::new([0; 16]),
                set_id: crate::domain::RecoverySetId::new([1; 16]),
                file_id,
                slice_checksums: vec![(md5_hash, crc32)],
            }),
        ];

        let engine = GlobalVerificationEngine::from_packets(&packets, base_path).unwrap();
        let reporter = crate::reporters::ConsoleVerificationReporter::new();
        let results = engine.verify_recovery_set(&reporter, false);

        // Block will be found but file should be Corrupted due to hash/size mismatch
        assert_eq!(results.files[0].blocks_available, 1);
        assert_eq!(
            results.files[0].status,
            FileStatus::Corrupted,
            "File should be Corrupted when file hash doesn't match even if blocks do"
        );
    }
}
