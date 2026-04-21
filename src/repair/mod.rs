//! PAR2 File Repair Module
//!
//! This module provides functionality for repairing files using PAR2 recovery data.
//! It implements Reed-Solomon error correction to reconstruct missing or corrupted files
//! using the Vandermonde polynomial 0x1100B for GF(2^16) operations.
//!
//! ## Performance
//!
//! Parallel Reed-Solomon reconstruction with SIMD optimizations (PSHUFB on x86_64, NEON on ARM64,
//! portable_simd cross-platform) achieve significant speedups over par2cmdline.
//!
//! See `docs/SIMD_OPTIMIZATION.md` and `docs/BENCHMARK_RESULTS.md` for detailed analysis.

mod builder;
mod context;
mod error;
pub mod error_helpers;
mod md5_writer;
mod progress;
mod recovery_loader;
pub(crate) mod slice_provider;
mod types;
pub mod validate;

// Re-export public API
pub use builder::RepairContextBuilder;
pub use context::RepairContext;
pub use error::{RepairError, Result};
pub use md5_writer::Md5Writer;
pub use progress::{ConsoleReporter, ProgressReporter, SilentReporter};
pub use recovery_loader::{FileSystemLoader, RecoveryDataLoader};
pub use slice_provider::{
    ActualDataSize, ChunkedSliceProvider, LogicalSliceSize, RecoverySliceProvider,
    Result as SliceProviderResult, SliceLocation, SliceProvider, SliceProviderError,
    DEFAULT_CHUNK_SIZE,
};
pub use types::{
    FileInfo, FileStatus, ReconstructedSlices, RecoverySetInfo, RepairResult, ValidationCache,
    VerificationResult,
};

// Expose validation helper moved here
pub use validate::validate_blocks_md5_crc32;

use crate::domain::{FileId, LocalSliceIndex, Md5Hash};
use crate::verify::BlockSource;
use crate::RecoverySlicePacket;
use error_helpers::*;
use log::debug;
use rayon::prelude::*;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::fs;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};

pub(crate) fn calculate_repair_chunk_size(
    slice_size: usize,
    memory_limit: Option<usize>,
) -> Result<usize> {
    let Some(limit) = memory_limit else {
        return Ok(slice_size);
    };
    if limit == 0 {
        return Err(RepairError::ContextCreation(
            "Memory limit must be greater than 0".to_string(),
        ));
    }

    let capped = limit.min(slice_size);
    if capped < 4 {
        if slice_size <= 4 {
            return Ok(capped);
        }
        return Err(RepairError::ContextCreation(format!(
            "Memory limit {} bytes is too small; repair requires at least 4 bytes",
            limit
        )));
    }
    if capped == 4 {
        return Ok(capped);
    }

    Ok(capped & !3)
}

impl RepairContext {
    /// Check the status of all files in the recovery set (in parallel)
    pub fn check_file_status(&self) -> HashMap<String, FileStatus> {
        self.recovery_set
            .files
            .par_iter()
            .map(|file_info| {
                let file_path = self.base_path.join(&file_info.file_name);

                // Report file opening (thread-safe)
                self.reporter().report_file_opening(&file_info.file_name);

                let status = self.determine_file_status(&file_path, file_info);

                // Report determined status (thread-safe)
                self.reporter()
                    .report_file_status(&file_info.file_name, status);

                (file_info.file_name.clone(), status)
            })
            .collect()
    }

    /// Determine the status of a single file
    fn determine_file_status(&self, file_path: &Path, file_info: &FileInfo) -> FileStatus {
        if !file_path.exists() {
            return FileStatus::Missing;
        }

        // Check file size
        if let Ok(metadata) = fs::metadata(file_path) {
            if metadata.len() != file_info.file_length.as_u64() {
                return FileStatus::Corrupted;
            }
        } else {
            return FileStatus::Corrupted;
        }

        // ULTRA-FAST filter: Check 16KB MD5 first (0.016GB vs 38GB = 2375x faster!)
        // For large datasets, this avoids hashing 38GB when files are intact
        use crate::checksum::{calculate_file_md5, calculate_file_md5_16k};
        if let Ok(md5_16k) = calculate_file_md5_16k(file_path) {
            if md5_16k != file_info.md5_16k {
                // 16KB doesn't match - file is definitely corrupted
                return FileStatus::Corrupted;
            }
            // 16KB matches - very likely valid, but verify full hash to be certain
        }

        // Full MD5 check (only if 16KB hash matched or couldn't be read)
        if let Ok(file_md5) = calculate_file_md5(file_path) {
            if file_md5 == file_info.md5_hash {
                return FileStatus::Present;
            }
        }

        FileStatus::Corrupted
    }

    /// Move exact wrong-name extra files back to their protected target paths.
    pub(crate) fn restore_renamed_files(
        &self,
        verification_results: &crate::verify::VerificationResults,
    ) -> Result<Vec<String>> {
        let mut restored = Vec::new();

        for file_result in &verification_results.files {
            if file_result.status != crate::verify::FileStatus::Renamed {
                continue;
            }

            let Some(matched_path) = &file_result.matched_path else {
                continue;
            };

            let Some(file_info) = self
                .recovery_set
                .files
                .iter()
                .find(|file_info| file_info.file_name == file_result.file_name)
            else {
                continue;
            };

            let target_path = self.base_path.join(&file_info.file_name);
            if target_path.exists() {
                let backup_path = Self::next_backup_path(&target_path);
                rename_file(&target_path, &backup_path)?;
                self.record_repair_created_backup(backup_path);
            }

            rename_file(matched_path, &target_path)?;

            restored.push(file_info.file_name.clone());
        }

        Ok(restored)
    }

    fn next_backup_path(target_path: &Path) -> PathBuf {
        let parent = target_path.parent().unwrap_or_else(|| Path::new(""));
        let file_name = target_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| target_path.to_string_lossy().into_owned());

        for suffix in 1usize.. {
            let candidate = parent.join(format!("{file_name}.{suffix}"));
            if !candidate.exists() {
                return candidate;
            }
        }

        unreachable!("usize suffix space exhausted")
    }

    /// Perform repair using pre-computed verification results
    ///
    /// This method uses the comprehensive verification results (from byte-by-byte
    /// sliding window scanning) to determine which blocks are available, instead of
    /// using aligned CRC32 validation which fails for displaced blocks.
    ///
    /// # Arguments
    /// * `verification_results` - Results from comprehensive_verify_files_with_config_and_reporter
    pub fn repair(
        &self,
        verification_results: crate::verify::VerificationResults,
    ) -> Result<RepairResult> {
        debug!("repair");

        // Build validation cache from verification results
        // Convert FileVerificationResult to ValidationCache
        let mut validation_cache = ValidationCache::new();
        let mut file_status = HashMap::default();
        let mut block_sources_map: HashMap<FileId, HashMap<u32, BlockSource>> =
            HashMap::default();

        for file_result in &verification_results.files {
            // Build set of valid block indices
            // damaged_blocks contains the indices of DAMAGED blocks, so we need to invert
            let damaged_set: HashSet<usize> = file_result
                .damaged_blocks
                .iter()
                .map(|&idx| idx as usize)
                .collect();

            let valid_slices: HashSet<usize> = (0..file_result.total_blocks)
                .filter(|idx| !damaged_set.contains(idx))
                .collect();

            validation_cache.insert(file_result.file_id, valid_slices);

            let target_path = self.base_path.join(&file_result.file_name);
            let block_sources = if file_result.block_sources.is_empty() {
                file_result
                    .block_positions
                    .iter()
                    .map(|(block_number, offset)| {
                        (
                            *block_number,
                            BlockSource {
                                file_path: target_path.clone(),
                                offset: *offset,
                            },
                        )
                    })
                    .collect()
            } else {
                file_result.block_sources.clone()
            };
            block_sources_map.insert(file_result.file_id, block_sources);

            // Convert verify::FileStatus to repair::FileStatus
            let status = match file_result.status {
                crate::verify::FileStatus::Present => FileStatus::Present,
                crate::verify::FileStatus::Renamed => FileStatus::Corrupted, // Treat renamed as corrupted for repair
                crate::verify::FileStatus::Corrupted => FileStatus::Corrupted,
                crate::verify::FileStatus::Missing => FileStatus::Missing,
            };
            file_status.insert(file_result.file_name.clone(), status);

            debug!(
                "  File {}: {} valid blocks out of {} (status: {:?})",
                file_result.file_name,
                file_result.blocks_available,
                file_result.total_blocks,
                status
            );
        }

        // Recalculate total damaged blocks
        let mut total_damaged_blocks = 0;
        for file_info in &self.recovery_set.files {
            let valid_count = validation_cache.valid_count(&file_info.file_id);
            let damaged_count = file_info.slice_count - valid_count;
            total_damaged_blocks += damaged_count;
        }

        debug!(
            "  total_damaged_blocks: {}, recovery_blocks: {}",
            total_damaged_blocks,
            self.recovery_set.recovery_slices_metadata.len()
        );

        // Check if repair is needed
        if total_damaged_blocks == 0 {
            let verified_files: Vec<String> = file_status.keys().cloned().collect();
            let files_verified = verified_files.len();
            return Ok(RepairResult::NoRepairNeeded {
                files_verified,
                verified_files,
                message: "All files are already present and valid.".to_string(),
            });
        }

        // Report recovery block information
        self.reporter().report_recovery_info(
            self.recovery_set.recovery_slices_metadata.len(),
            total_damaged_blocks,
        );

        if total_damaged_blocks > self.recovery_set.recovery_slices_metadata.len() {
            return Ok(RepairResult::Failed {
                files_failed: file_status.keys().cloned().collect(),
                files_verified: 0,
                verified_files: Vec::new(),
                message: format!(
                    "Insufficient recovery data: need {} blocks but only have {}",
                    total_damaged_blocks,
                    self.recovery_set.recovery_slices_metadata.len()
                ),
            });
        }

        // Perform the actual repair with validation cache from comprehensive verification
        self.perform_reed_solomon_repair(&file_status, &validation_cache, &block_sources_map)
    }

    /// Perform Reed-Solomon repair
    ///
    /// CRITICAL: This method uses a unified reconstruction approach for ALL files.
    /// When multiple files have missing/damaged slices, we MUST reconstruct ALL
    /// missing slices together in ONE Reed-Solomon operation. Reconstructing files
    /// independently produces incorrect results because PAR2 recovery slices are
    /// XOR'd across ALL data slices in the set.
    ///
    /// Example of why per-file reconstruction fails:
    /// - file_a: slice 947 missing
    /// - file_b: slice 1473 damaged
    /// - If we reconstruct file_a alone: recovery_0 XOR (all slices except 947)
    ///   This includes the DAMAGED slice 1473, producing: reconstructed_947 = actual_947 XOR damaged_1473 ❌
    ///
    /// Correct approach:
    /// 1. Collect ALL missing slices: [947, 1473]
    /// 2. Build input provider with ONLY valid slices
    /// 3. ONE Reed-Solomon reconstruction for both
    /// 4. Distribute results to respective files
    fn perform_reed_solomon_repair(
        &self,
        file_status: &HashMap<String, FileStatus>,
        validation_cache: &ValidationCache,
        block_sources_map: &HashMap<FileId, HashMap<u32, BlockSource>>,
    ) -> Result<RepairResult> {
        debug!(
            "perform_reed_solomon_repair: processing {} files",
            self.recovery_set.files.len()
        );

        // Report repair header
        self.reporter().report_repair_header();

        debug!("File status check:");
        for (idx, file_info) in self.recovery_set.files.iter().enumerate() {
            let status = file_status
                .get(&file_info.file_name)
                .unwrap_or(&FileStatus::Missing);
            debug!(
                "  FileInfo[{}]: {} - offset: {}, slices: {}, status: {:?}",
                idx,
                file_info.file_name,
                file_info.global_slice_offset.as_usize(),
                file_info.slice_count,
                status
            );
        }

        // STEP 1: Identify all files needing repair and collect their missing slices
        let mut files_to_repair: Vec<(&FileInfo, Vec<usize>)> = Vec::new();
        let mut verified_files = Vec::new();

        for file_info in &self.recovery_set.files {
            let status = file_status
                .get(&file_info.file_name)
                .unwrap_or(&FileStatus::Missing);

            if *status == FileStatus::Present {
                verified_files.push(file_info.file_name.clone());
                continue;
            }

            // Determine which slices are missing for this file
            let valid_slice_indices = validation_cache
                .get(&file_info.file_id)
                .ok_or_else(|| RepairError::NoValidationCache(file_info.file_name.clone()))?;

            let missing_slices: Vec<usize> = (0..file_info.slice_count.as_usize())
                .filter(|idx| !valid_slice_indices.contains(idx))
                .collect();

            if missing_slices.is_empty() {
                // All slices validated, but file status says not Present
                if *status == FileStatus::Corrupted {
                    debug!(
                        "File {} has all valid slices but MD5 doesn't match",
                        file_info.file_name
                    );
                }
                verified_files.push(file_info.file_name.clone());
                continue;
            }

            debug!(
                "File {} needs {} missing slices repaired",
                file_info.file_name,
                missing_slices.len()
            );
            files_to_repair.push((file_info, missing_slices));
        }

        if files_to_repair.is_empty() {
            return Ok(RepairResult::NoRepairNeeded {
                files_verified: verified_files.len(),
                verified_files,
                message: "All files are already intact".to_string(),
            });
        }

        // STEP 2: Reconstruct ALL missing slices across ALL files in ONE operation
        let reconstructed_data: HashMap<usize, Vec<u8>> = self.reconstruct_all_missing_slices(
            &files_to_repair,
            validation_cache,
            block_sources_map,
        )?;

        // STEP 3: Write reconstructed data to each file
        let mut repaired_files = Vec::new();
        let mut files_failed = Vec::new();

        for (file_info, missing_slices) in &files_to_repair {
            self.reporter().report_repair_start(&file_info.file_name);

            // Extract this file's reconstructed slices from the combined result
            let mut file_reconstructed = ReconstructedSlices::new();
            for &local_idx in missing_slices {
                let global_idx = file_info.local_to_global(LocalSliceIndex::new(local_idx));
                if let Some(data) = reconstructed_data.get(&global_idx.as_usize()) {
                    file_reconstructed.insert(local_idx, data.clone());
                }
            }

            let valid_slice_indices = validation_cache
                .get(&file_info.file_id)
                .ok_or_else(|| RepairError::NoValidationCache(file_info.file_name.clone()))?;

            let block_sources = block_sources_map
                .get(&file_info.file_id)
                .cloned()
                .unwrap_or_default();

            let file_path = self.base_path.join(&file_info.file_name);
            match self.write_repaired_file(
                &file_path,
                file_info,
                valid_slice_indices,
                &file_reconstructed,
                &block_sources,
            ) {
                Ok(()) => {
                    self.reporter()
                        .report_repair_complete(&file_info.file_name, true);
                    repaired_files.push(file_info.file_name.clone());
                    debug!("Successfully repaired: {}", file_info.file_name);
                }
                Err(e) => {
                    self.reporter()
                        .report_repair_failed(&file_info.file_name, &e.to_string());
                    files_failed.push(file_info.file_name.clone());
                    debug!(
                        "Failed to write repaired file {}: {}",
                        file_info.file_name, e
                    );
                }
            }
        }

        let files_repaired_count = repaired_files.len();
        let files_verified_count = verified_files.len();

        if !files_failed.is_empty() {
            let message = format!(
                "Repaired {} file(s), verified {} file(s), failed to repair {} file(s)",
                files_repaired_count,
                files_verified_count,
                files_failed.len()
            );
            return Ok(RepairResult::Failed {
                files_failed,
                files_verified: files_verified_count,
                verified_files,
                message,
            });
        }

        // NOTE: No need to re-verify repaired files here.
        // write_repaired_file() already computes MD5 during streaming write
        // and validates it before returning success. If MD5 didn't match,
        // write_repaired_file() would have returned an error and the file
        // would be in files_failed, not repaired_files.
        //
        // This avoids redundant full-file MD5 computation (saves massive I/O).

        if files_repaired_count > 0 {
            self.reporter().report_verification_header();

            // Report all repaired files as verified (they were validated during write)
            for repaired_file in &repaired_files {
                let file_info = self
                    .recovery_set
                    .files
                    .iter()
                    .find(|f| &f.file_name == repaired_file)
                    .ok_or_else(|| {
                        RepairError::ContextCreation(format!(
                            "Repaired file '{}' not found in recovery set file list",
                            repaired_file
                        ))
                    })?;

                self.reporter()
                    .report_verification(&file_info.file_name, VerificationResult::Verified);
            }

            // Success: all repaired files verified during write
            let total_verified = verified_files.len() + repaired_files.len();
            let result = RepairResult::Success {
                files_repaired: files_repaired_count,
                files_verified: total_verified,
                repaired_files: repaired_files.clone(),
                verified_files: [verified_files, repaired_files].concat(),
                message: format!("Successfully repaired {} file(s)", files_repaired_count),
            };
            self.reporter().report_final_result(&result);
            return Ok(result);
        }

        // No files needed repair
        Ok(RepairResult::NoRepairNeeded {
            files_verified: files_verified_count,
            verified_files,
            message: format!("All {} file(s) verified as intact", files_verified_count),
        })
    }

    /// Reconstruct ALL missing slices across ALL files in a single Reed-Solomon operation
    ///
    /// This is the core fix for the multifile repair bug. We collect ALL missing/damaged
    /// slices from ALL files and reconstruct them together, ensuring the Reed-Solomon
    /// matrix uses only valid slices as input.
    ///
    /// Returns a HashMap mapping global slice index -> reconstructed data
    fn reconstruct_all_missing_slices(
        &self,
        files_to_repair: &[(&FileInfo, Vec<usize>)],
        validation_cache: &ValidationCache,
        block_sources_map: &HashMap<FileId, HashMap<u32, BlockSource>>,
    ) -> Result<HashMap<usize, Vec<u8>>> {
        use self::slice_provider::{ChunkedSliceProvider, RecoverySliceProvider, SliceLocation};
        use std::io::Cursor;

        // Collect all global missing indices
        let mut all_missing_global: Vec<usize> = Vec::new();
        for (file_info, missing_local) in files_to_repair {
            for &local_idx in missing_local {
                let global_idx = file_info.local_to_global(LocalSliceIndex::new(local_idx));
                all_missing_global.push(global_idx.as_usize());
            }
        }
        all_missing_global.sort();

        debug!(
            "Reconstructing {} total missing slices across {} files",
            all_missing_global.len(),
            files_to_repair.len()
        );

        // Check if we have enough recovery blocks
        if all_missing_global.len() > self.recovery_set.recovery_slices_metadata.len() {
            return Err(RepairError::InsufficientRecovery {
                missing: all_missing_global.len(),
                available: self.recovery_set.recovery_slices_metadata.len(),
            });
        }

        // Build slice provider with ONLY valid slices (excludes ALL missing/damaged)
        let mut input_provider = ChunkedSliceProvider::new(self.recovery_set.slice_size.as_usize());

        debug!("Building input provider (excluding ALL missing/damaged slices):");
        for file_info in &self.recovery_set.files {
            let file_path = self.base_path.join(&file_info.file_name);

            let valid_slices = validation_cache
                .get(&file_info.file_id)
                .ok_or_else(|| RepairError::NoValidationCache(file_info.file_name.clone()))?;

            if valid_slices.is_empty() {
                debug!(
                    "  File {} - no valid slices (skipping)",
                    file_info.file_name
                );
                continue;
            }

            debug!(
                "  File {} - using {} valid slices",
                file_info.file_name,
                valid_slices.len()
            );

            let file_block_sources = block_sources_map
                .get(&file_info.file_id)
                .cloned()
                .unwrap_or_default();

            for slice_index in 0..file_info.slice_count.as_usize() {
                if !valid_slices.contains(&slice_index) {
                    continue; // Skip invalid slices
                }

                let global_index = file_info.local_to_global(LocalSliceIndex::new(slice_index));

                let block_source = file_block_sources.get(&(slice_index as u32));
                let source_path = block_source
                    .map(|source| source.file_path.clone())
                    .unwrap_or_else(|| file_path.clone());

                let source_file_size = if source_path.exists() {
                    fs::metadata(&source_path).map(|m| m.len()).unwrap_or(0)
                } else {
                    0
                };

                // CRITICAL: Use actual source location from verification if available.
                // Otherwise fall back to expected target-file position.
                let offset = block_source
                    .map(|source| source.offset as u64)
                    .unwrap_or_else(|| {
                        (slice_index * self.recovery_set.slice_size.as_usize()) as u64
                    });

                // Calculate expected slice size based on PAR2 metadata
                let expected_slice_size = if slice_index == file_info.slice_count.as_usize() - 1 {
                    let remaining = file_info.file_length % self.recovery_set.slice_size;
                    if remaining == 0 {
                        self.recovery_set.slice_size.as_usize()
                    } else {
                        remaining as usize
                    }
                } else {
                    self.recovery_set.slice_size.as_usize()
                };

                // CRITICAL FIX: Calculate ACTUAL available bytes in the file for this slice
                // Handles truncated files where actual_file_size < expected file_length
                let actual_size = if offset >= source_file_size {
                    // Slice is entirely beyond EOF (file severely truncated)
                    debug!(
                        "  Slice {} is beyond EOF (offset {} >= file size {}), skipping",
                        slice_index, offset, source_file_size
                    );
                    continue; // Skip this slice entirely
                } else {
                    let bytes_available = (source_file_size - offset) as usize;
                    bytes_available.min(expected_slice_size)
                };

                let expected_crc = self
                    .recovery_set
                    .file_slice_checksums
                    .get(&file_info.file_id)
                    .and_then(|checksums| checksums.slice_checksums.get(slice_index))
                    .map(|(_, crc)| *crc);

                input_provider.add_slice(
                    global_index.as_usize(),
                    SliceLocation {
                        file_path: source_path,
                        offset,
                        actual_size: ActualDataSize::new(actual_size),
                        logical_size: LogicalSliceSize::new(
                            self.recovery_set.slice_size.as_usize(),
                        ),
                        expected_crc,
                    },
                );
            }
        }

        // Build recovery slice provider
        let mut recovery_provider =
            RecoverySliceProvider::new(self.recovery_set.slice_size.as_usize());

        for metadata in &self.recovery_set.recovery_slices_metadata {
            recovery_provider.add_recovery_metadata(metadata.exponent as usize, metadata.clone());
        }

        // Create reconstruction engine
        let dummy_recovery_slices: Vec<RecoverySlicePacket> = self
            .recovery_set
            .recovery_slices_metadata
            .iter()
            .map(|metadata| RecoverySlicePacket {
                length: 68,
                md5: Md5Hash::new([0u8; 16]),
                set_id: metadata.set_id,
                type_of_packet: *b"PAR 2.0\0RecvSlic",
                exponent: metadata.exponent,
                recovery_data: Vec::new(),
            })
            .collect();

        let total_input_slices: usize = self
            .recovery_set
            .files
            .iter()
            .map(|f| f.slice_count.as_usize())
            .sum();
        let reconstruction_engine = crate::reed_solomon::ReconstructionEngine::new(
            self.recovery_set.slice_size.as_usize(),
            total_input_slices,
            dummy_recovery_slices,
        );

        // Create output buffers for all missing slices
        let mut output_buffers: HashMap<usize, Cursor<Vec<u8>>> = HashMap::default();
        for &global_idx in &all_missing_global {
            output_buffers.insert(global_idx, Cursor::new(Vec::new()));
        }

        // Perform reconstruction
        // CRITICAL PERFORMANCE FIX: Use full slice_size as chunk size to minimize I/O
        //
        // Background: For a 25GB file with 2MB slices (~12,500 slices):
        // - Old approach (64KB chunks): Read ALL 12,500 slices for EACH of ~32 chunks
        //   = 32 × 12,500 × 64KB = 25GB+ of reads (reading same data 32 times!)
        // - New approach (2MB chunks): Read each slice ONCE in a single chunk
        //   = 12,500 × 2MB = 25GB total (each slice read exactly once)
        //
        // This reduces I/O from ~126GB to ~50GB (2x file size, not 5x)
        let optimal_chunk_size = calculate_repair_chunk_size(
            self.recovery_set.slice_size.as_usize(),
            self.memory_limit,
        )?;
        let result = reconstruction_engine.reconstruct_missing_slices_chunked(
            &mut input_provider,
            &recovery_provider,
            &all_missing_global,
            &mut output_buffers,
            optimal_chunk_size,
        );

        if !result.success {
            return Err(RepairError::ReconstructionFailed(
                result
                    .error_message
                    .unwrap_or_else(|| "Reconstruction failed".to_string()),
            ));
        }

        // Convert cursors to Vec<u8>
        let reconstructed: HashMap<usize, Vec<u8>> = output_buffers
            .into_iter()
            .map(|(idx, cursor)| (idx, cursor.into_inner()))
            .collect();

        debug!("Successfully reconstructed {} slices", reconstructed.len());
        Ok(reconstructed)
    }

    /// Validate slices from an existing file
    /// Returns only the indices of valid slices, not the slice data itself
    pub fn validate_file_slices(&self, file_info: &FileInfo) -> Result<HashSet<usize>> {
        let file_path = self.base_path.join(&file_info.file_name);

        if !file_path.exists() {
            return Ok(HashSet::default()); // No valid slices for missing file
        }

        // CRITICAL: If no checksums available, we CANNOT validate slices
        // Return empty set - treat all slices as corrupted (conservative approach)
        // This is correct behavior: without checksums, we must repair
        let checksums = match self
            .recovery_set
            .file_slice_checksums
            .get(&file_info.file_id)
        {
            Some(checksums) => checksums,
            None => {
                debug!(
                    "No slice checksums available for file {} - treating all slices as corrupted",
                    file_info.file_name
                );
                return Ok(HashSet::default()); // Empty set = all slices need repair
            }
        };

        // Extract just the CRC32 values for validation
        let crc_checksums: Vec<_> = checksums
            .slice_checksums
            .iter()
            .map(|(_, crc)| *crc)
            .collect();

        // Use shared validation module for efficient sequential I/O
        let valid_slices = crate::verify::validation::validate_slices_crc32(
            &file_path,
            &crc_checksums,
            self.recovery_set.slice_size.as_usize(),
            file_info.file_length.as_u64(),
        )?;

        debug!(
            "Validated {} valid slices out of {} total slices",
            valid_slices.len(),
            file_info.slice_count
        );

        Ok(valid_slices)
    }

    /// Write repaired file by streaming slices from disk and reconstructed data
    fn write_repaired_file(
        &self,
        file_path: &Path,
        file_info: &FileInfo,
        valid_slice_indices: &HashSet<usize>,
        reconstructed_slices: &ReconstructedSlices,
        block_sources: &HashMap<u32, BlockSource>,
    ) -> Result<()> {
        debug!("Writing repaired file with streaming I/O: {:?}", file_path);

        // Write to temp file first, then rename to avoid corrupting source while reading
        let temp_path = file_path.with_extension("par2_tmp");

        // Guard to clean up temp file on error
        struct TempFileGuard {
            path: std::path::PathBuf,
            keep: bool,
        }

        impl Drop for TempFileGuard {
            fn drop(&mut self) {
                if !self.keep && self.path.exists() {
                    let _ = std::fs::remove_file(&self.path);
                    debug!("Cleaned up temporary file: {:?}", self.path);
                }
            }
        }

        let mut temp_guard = TempFileGuard {
            path: temp_path.clone(),
            keep: false,
        };

        let target_source_path = self.base_path.join(&file_info.file_name);
        let mut source_files: HashMap<PathBuf, (std::fs::File, Option<u64>)> = HashMap::default();

        // Create temp output file
        let file = create_file(&temp_path)?;
        let buffered = std::io::BufWriter::with_capacity(1024 * 1024, file);
        let mut writer = md5_writer::Md5Writer::new(buffered);

        let slice_size = self.recovery_set.slice_size.as_usize();
        let mut slice_buffer = vec![0u8; slice_size];
        let mut bytes_written = 0u64;

        for slice_index in 0..file_info.slice_count.as_usize() {
            let actual_size = if slice_index == file_info.slice_count - 1 {
                let remaining = file_info.file_length % self.recovery_set.slice_size;
                if remaining == 0 {
                    slice_size
                } else {
                    remaining as usize
                }
            } else {
                slice_size
            };

            // Get slice data from either reconstructed or source file
            if let Some(reconstructed_data) = reconstructed_slices.get(slice_index) {
                // Write reconstructed slice
                debug!(
                    "Writing reconstructed slice {} (size: {}, first 8 bytes: {:02x?})",
                    slice_index,
                    actual_size,
                    &reconstructed_data[..8.min(reconstructed_data.len())]
                );
                write_slice_all(
                    &mut writer,
                    &reconstructed_data[..actual_size],
                    &temp_path,
                    slice_index,
                )?;
                bytes_written += actual_size as u64;
            } else if valid_slice_indices.contains(&slice_index) {
                let source = block_sources
                    .get(&(slice_index as u32))
                    .cloned()
                    .unwrap_or_else(|| BlockSource {
                        file_path: target_source_path.clone(),
                        offset: slice_index * slice_size,
                    });

                if source.file_path.exists() {
                    if !source_files.contains_key(&source.file_path) {
                        source_files.insert(
                            source.file_path.clone(),
                            (open_for_reading(&source.file_path)?, Some(0)),
                        );
                    }
                    let (file, next_expected_offset) = source_files
                        .get_mut(&source.file_path)
                        .ok_or(RepairError::ValidSliceMissingSource(slice_index))?;

                    let offset = source.offset as u64;
                    debug!(
                        "Reading slice {} from {:?} offset {} ({})",
                        slice_index,
                        source.file_path,
                        offset,
                        if block_sources.contains_key(&(slice_index as u32)) {
                            "source location from verification"
                        } else {
                            "expected aligned target position"
                        }
                    );

                    // Only seek if we're not already at the right position (optimize sequential reads)
                    if *next_expected_offset != Some(offset) {
                        seek_file(file, SeekFrom::Start(offset), &source.file_path)?;
                    }

                    read_slice_exact(
                        file,
                        &mut slice_buffer[..actual_size],
                        &source.file_path,
                        slice_index,
                    )?;
                    write_slice_all(
                        &mut writer,
                        &slice_buffer[..actual_size],
                        &temp_path,
                        slice_index,
                    )?;
                    bytes_written += actual_size as u64;
                    *next_expected_offset = Some(offset + actual_size as u64);
                } else {
                    return Err(RepairError::ValidSliceMissingSource(slice_index));
                }
            } else {
                return Err(RepairError::SliceNotAvailable(slice_index));
            }
        }

        flush_writer(&mut writer, &temp_path)?;

        // Finalize MD5 computation and get the hash
        let (mut buffered_writer, computed_md5) = writer.finalize();
        flush_writer(&mut buffered_writer, &temp_path)?;
        drop(buffered_writer); // Close the file before rename
        drop(source_files); // Close source files before rename

        if bytes_written != file_info.file_length.as_u64() {
            return Err(RepairError::ByteCountMismatch {
                written: bytes_written,
                expected: file_info.file_length.as_u64(),
            });
        }

        // Rename temp file to final destination
        rename_file(&temp_path, file_path)?;

        // Mark temp file as successfully renamed (no cleanup needed)
        temp_guard.keep = true;

        // Verify the MD5 hash matches expected (no re-read needed!)
        let expected_md5 = file_info.md5_hash.as_bytes();
        if computed_md5 != *expected_md5 {
            return Err(RepairError::Md5MismatchAfterRepair {
                file: file_path.to_path_buf(),
                expected: *expected_md5,
                computed: computed_md5,
            });
        }

        debug!(
            "✓ Wrote {} bytes to {:?}, MD5 verified: {:02x?}",
            bytes_written, file_path, computed_md5
        );

        Ok(())
    }
}

/// High-level repair function - loads PAR2 files and performs repair
///
/// This is the main entry point for repair operations. It loads the PAR2 file,
/// creates a repair context, and performs the repair operation.
///
/// # Arguments
/// * `par2_file` - Path to the PAR2 file
/// * `reporter` - Progress reporter implementation (use ConsoleReporter, SilentReporter, or custom)
/// * `verify_config` - Verification configuration (threading, parallel/sequential)
///
/// # Returns
/// * `Ok((RepairContext, RepairResult))` - Repair operation completed with context and result
/// * `Err(...)` - Failed to load PAR2 files or create repair context
pub fn repair_files(
    par2_file: &str,
    reporter: Box<dyn ProgressReporter>,
    verify_config: &crate::verify::VerificationConfig,
) -> Result<(RepairContext, RepairResult)> {
    repair_files_with_base_path(par2_file, reporter, verify_config, None)
}

/// Repair files with an optional base path override for resolving protected
/// file names stored in the PAR2 set.
pub fn repair_files_with_base_path(
    par2_file: &str,
    reporter: Box<dyn ProgressReporter>,
    verify_config: &crate::verify::VerificationConfig,
    base_path_override: Option<&Path>,
) -> Result<(RepairContext, RepairResult)> {
    let silent_reporter = crate::reporters::SilentVerificationReporter;
    repair_files_with_base_path_and_extra_files_and_verification_reporter(
        par2_file,
        reporter,
        verify_config,
        base_path_override,
        &[],
        &silent_reporter,
    )
}

/// Repair files with optional base path and extra file scan inputs.
pub fn repair_files_with_base_path_and_extra_files(
    par2_file: &str,
    reporter: Box<dyn ProgressReporter>,
    verify_config: &crate::verify::VerificationConfig,
    base_path_override: Option<&Path>,
    extra_files: &[PathBuf],
) -> Result<(RepairContext, RepairResult)> {
    let silent_reporter = crate::reporters::SilentVerificationReporter;
    repair_files_with_base_path_and_extra_files_and_verification_reporter(
        par2_file,
        reporter,
        verify_config,
        base_path_override,
        extra_files,
        &silent_reporter,
    )
}

/// Repair files while reporting the pre-repair verification pass.
pub fn repair_files_with_base_path_and_extra_files_and_verification_reporter(
    par2_file: &str,
    reporter: Box<dyn ProgressReporter>,
    verify_config: &crate::verify::VerificationConfig,
    base_path_override: Option<&Path>,
    extra_files: &[PathBuf],
    verification_reporter: &dyn crate::reporters::VerificationReporter,
) -> Result<(RepairContext, RepairResult)> {
    let par2_path = Path::new(par2_file);

    // Validate file exists
    if !par2_path.exists() {
        return Err(RepairError::FileNotFound(par2_file.to_string()));
    }

    // Collect all PAR2 files in the set. Explicit PAR2 inputs are allowed here,
    // but packet loading filters out packets from foreign recovery sets.
    let mut par2_files = crate::par2_files::collect_par2_files(par2_path);
    par2_files.extend(
        verify_config
            .extra_files
            .iter()
            .filter(|path| is_par2_path(path))
            .cloned(),
    );
    crate::par2_files::sort_dedup_preserving_first(&mut par2_files);

    // Load metadata for memory-efficient recovery slice loading
    let metadata = crate::par2_files::parse_recovery_slice_metadata(&par2_files, false);

    // Load packets WITHOUT recovery slices (use metadata for lazy loading instead)
    // This saves ~1.5GB of memory for large PAR2 sets since recovery data is
    // loaded on-demand during reconstruction via RecoverySliceProvider
    let initial_packet_set = crate::par2_files::load_par2_packets(&par2_files, false, false);
    if initial_packet_set.packets.is_empty() {
        return Err(RepairError::NoValidPackets);
    }

    // Get the base directory for file resolution. An explicit caller override
    // wins over the CLI/configured base path.
    let base_path = base_path_override
        .map(Path::to_path_buf)
        .or_else(|| verify_config.base_path.clone())
        .unwrap_or_else(|| par2_path.parent().unwrap_or(Path::new(".")).to_path_buf());

    // Create repair context before verification so normal repair output prints
    // the set summary once before source verification.
    let mut repair_builder = RepairContextBuilder::new()
        .packets(initial_packet_set.packets)
        .metadata(metadata)
        .base_path(base_path.clone())
        .reporter(reporter);
    if let Some(memory_limit) = verify_config.memory_limit {
        repair_builder = repair_builder.memory_limit(memory_limit);
    }
    let repair_context = repair_builder.build()?;

    repair_context
        .reporter()
        .report_statistics(&repair_context.recovery_set);

    // CRITICAL FIX: Run comprehensive verification to get accurate block availability
    // Reference: par2cmdline-turbo uses byte-by-byte sliding window scanning (FileCheckSummer)
    // to find blocks at ANY position (displaced blocks), not just aligned positions.
    // The old aligned CRC32 validation only checked blocks at (0, block_size, 2*block_size...)
    // which FAILS when blocks are displaced (e.g., prepended data, non-aligned corruption).
    //
    // This ensures repair uses the SAME verification logic as the verify command,
    // preventing cases where verify says "487 blocks available" but repair only finds 121.
    //
    // OPTIMIZATION: Skip full file MD5 computation during pre-repair verification.
    // We only need block-level validation here - the full file MD5 will be computed
    // during the streaming write in write_repaired_file(), avoiding redundant I/O.
    let mut repair_verify_config = crate::verify::VerificationConfig::for_repair(
        verify_config.threads,
        verify_config.parallel,
    );
    repair_verify_config.extra_files = verify_config.extra_files.clone();
    repair_verify_config.base_path = Some(base_path.clone());
    repair_verify_config.file_threads = verify_config.file_threads;
    repair_verify_config.data_skipping = verify_config.data_skipping;
    repair_verify_config.skip_leeway = verify_config.skip_leeway;
    repair_verify_config.rename_only = verify_config.rename_only;
    if !extra_files.is_empty() || verify_config.rename_only {
        repair_verify_config.skip_full_file_md5 = false;
    }
    let mut verification_results = run_repair_verification(
        &par2_files,
        &repair_verify_config,
        &base_path,
        extra_files,
        verification_reporter,
    );
    verification_reporter.report_verification_results(&verification_results);

    let renamed_files = repair_context.restore_renamed_files(&verification_results)?;
    if !renamed_files.is_empty() {
        verification_results =
            run_repair_verification(
                &par2_files,
                &repair_verify_config,
                &base_path,
                extra_files,
                verification_reporter,
            );

        if repair_verification_is_complete(&verification_results) {
            return Ok((
                repair_context,
                RepairResult::Success {
                    files_repaired: renamed_files.len(),
                    files_verified: verification_results.present_file_count,
                    repaired_files: renamed_files.clone(),
                    verified_files: verification_results
                        .files
                        .iter()
                        .map(|file| file.file_name.clone())
                        .collect(),
                    message: format!(
                        "Successfully restored {} renamed file(s)",
                        renamed_files.len()
                    ),
                },
            ));
        }
    }

    if verify_config.rename_only {
        return Ok((
            repair_context,
            rename_only_repair_result(&verification_results, renamed_files),
        ));
    }

    let result = repair_context.repair(verification_results)?;

    Ok((repair_context, result))
}

fn run_repair_verification(
    par2_files: &[PathBuf],
    repair_verify_config: &crate::verify::VerificationConfig,
    base_path: &Path,
    extra_files: &[PathBuf],
    verification_reporter: &dyn crate::reporters::VerificationReporter,
) -> crate::verify::VerificationResults {
    let packet_set = crate::par2_files::load_par2_packets(par2_files, false, false);

    if extra_files.is_empty() {
        crate::verify::comprehensive_verify_files(
            packet_set,
            repair_verify_config,
            verification_reporter,
            base_path,
        )
    } else {
        crate::verify::comprehensive_verify_files_with_extra_files(
            packet_set,
            repair_verify_config,
            verification_reporter,
            base_path,
            extra_files,
        )
    }
}

fn repair_verification_is_complete(results: &crate::verify::VerificationResults) -> bool {
    results.renamed_file_count == 0
        && results.corrupted_file_count == 0
        && results.missing_file_count == 0
}

fn is_par2_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("par2"))
}

fn rename_only_repair_result(
    results: &crate::verify::VerificationResults,
    renamed_files: Vec<String>,
) -> RepairResult {
    let verified_files: Vec<_> = results
        .files
        .iter()
        .filter(|file| file.status == crate::verify::FileStatus::Present)
        .map(|file| file.file_name.clone())
        .collect();

    if repair_verification_is_complete(results) {
        if renamed_files.is_empty() {
            RepairResult::NoRepairNeeded {
                files_verified: verified_files.len(),
                verified_files,
                message: "All files are already present and valid.".to_string(),
            }
        } else {
            RepairResult::Success {
                files_repaired: renamed_files.len(),
                files_verified: verified_files.len(),
                repaired_files: renamed_files.clone(),
                verified_files,
                message: format!(
                    "Successfully restored {} renamed file(s)",
                    renamed_files.len()
                ),
            }
        }
    } else {
        RepairResult::Failed {
            files_failed: results
                .files
                .iter()
                .filter(|file| file.status != crate::verify::FileStatus::Present)
                .map(|file| file.file_name.clone())
                .collect(),
            files_verified: verified_files.len(),
            verified_files,
            message: "Rename-only repair could not restore all files.".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_chunk_size_respects_memory_limit() {
        assert_eq!(
            calculate_repair_chunk_size(1024 * 1024, None).unwrap(),
            1024 * 1024
        );
        assert_eq!(
            calculate_repair_chunk_size(1024 * 1024, Some(128 * 1024)).unwrap(),
            128 * 1024
        );
        assert!(calculate_repair_chunk_size(1024, Some(0)).is_err());
        assert!(calculate_repair_chunk_size(1024, Some(1)).is_err());
        assert_eq!(calculate_repair_chunk_size(1024, Some(7)).unwrap(), 4);
        assert_eq!(
            calculate_repair_chunk_size(1024 * 1024, Some(32 * 1024)).unwrap(),
            32 * 1024
        );
    }

    #[test]
    fn repair_chunk_size_never_exceeds_small_slice_limit() {
        assert_eq!(calculate_repair_chunk_size(1024, Some(1023)).unwrap(), 1020);
        assert_eq!(calculate_repair_chunk_size(3, Some(3)).unwrap(), 3);
    }
}
