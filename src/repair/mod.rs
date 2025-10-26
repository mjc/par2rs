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
mod progress;
mod types;

// Re-export public API
pub use builder::RepairContextBuilder;
pub use context::RepairContext;
pub use error::{RepairError, Result};
pub use progress::{ConsoleReporter, ProgressReporter, SilentReporter};
pub use types::{
    FileInfo, FileStatus, ReconstructedSlices, RecoverySetInfo, RepairResult, ValidationCache,
    VerificationResult,
};

use crate::domain::{FileId, LocalSliceIndex, Md5Hash};
use crate::slice_provider::{ActualDataSize, LogicalSliceSize};
use crate::RecoverySlicePacket;
use log::debug;
use rayon::prelude::*;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

impl RepairContext {
    /// Check the status of all files in the recovery set
    pub fn check_file_status(&self) -> HashMap<String, FileStatus> {
        let mut status_map = HashMap::default();

        for file_info in &self.recovery_set.files {
            let file_path = self.base_path.join(&file_info.file_name);

            // Report file opening
            self.reporter().report_file_opening(&file_info.file_name);

            let status = self.determine_file_status(&file_path, file_info);

            // Report determined status
            self.reporter()
                .report_file_status(&file_info.file_name, status);

            status_map.insert(file_info.file_name.clone(), status);
        }

        status_map
    }

    /// Determine the status of a single file
    fn determine_file_status(&self, file_path: &Path, file_info: &FileInfo) -> FileStatus {
        if !file_path.exists() {
            return FileStatus::Missing;
        }

        // Check file size
        if let Ok(metadata) = fs::metadata(file_path) {
            if metadata.len() != file_info.file_length {
                return FileStatus::Corrupted;
            }
        } else {
            return FileStatus::Corrupted;
        }

        // ULTRA-FAST filter: Check 16KB MD5 first (0.016GB vs 38GB = 2375x faster!)
        // For large datasets, this avoids hashing 38GB when files are intact
        use crate::file_verification::{calculate_file_md5, calculate_file_md5_16k};
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

    /// Perform repair operation
    pub fn repair_with_slices(&self) -> Result<RepairResult> {
        debug!("repair_with_slices");
        let mut file_status = self.check_file_status();
        debug!("  File statuses: {:?}", file_status);

        // Build validation cache by validating all files once upfront
        // PERFORMANCE: Use fast CRC32 slice validation instead of slow full-file MD5
        // PERFORMANCE: Validate files in parallel for maximum I/O throughput
        let validation_cache: ValidationCache = {
            let map: HashMap<FileId, HashSet<usize>> = self
                .recovery_set
                .files
                .par_iter()
                .map(|file_info| {
                    let status = file_status
                        .get(&file_info.file_name)
                        .unwrap_or(&FileStatus::Missing);

                    let valid_slices = match status {
                        FileStatus::Missing => {
                            // Empty set for missing files
                            HashSet::default()
                        }
                        FileStatus::Present => {
                            // Already validated - all slices valid
                            (0..file_info.slice_count).collect()
                        }
                        FileStatus::Corrupted => {
                            // Show progress for large files
                            if file_info.slice_count > 100 {
                                self.reporter().report_scanning(&file_info.file_name);
                            }

                            let valid_slices =
                                self.validate_file_slices(file_info).unwrap_or_default();

                            // Clear progress line for large files
                            if file_info.slice_count > 100 {
                                self.reporter().clear_scanning(&file_info.file_name);
                            }

                            valid_slices
                        }
                    };

                    (file_info.file_id, valid_slices)
                })
                .collect();

            let mut cache = ValidationCache::new();
            for (file_id, valid_slices) in map {
                cache.insert(file_id, valid_slices);
            }
            cache
        };

        // Update file_status based on validation results
        let mut total_damaged_blocks = 0;
        for file_info in &self.recovery_set.files {
            let valid_count = validation_cache.valid_count(&file_info.file_id);

            if valid_count == file_info.slice_count {
                // All slices are valid
                file_status.insert(file_info.file_name.clone(), FileStatus::Present);
                debug!(
                    "  All {} slices valid for {} - marking as Present",
                    valid_count, file_info.file_name
                );
            } else {
                let damaged_slices = file_info.slice_count - valid_count;
                total_damaged_blocks += damaged_slices;
                debug!(
                    "  {} damaged slices found in {}",
                    damaged_slices, file_info.file_name
                );
            }
        }

        // Check if repair is needed (after validation)
        let needs_repair = file_status.values().any(|s| *s != FileStatus::Present);
        debug!("  needs_repair: {}", needs_repair);
        if !needs_repair {
            let verified_files: Vec<String> = file_status.keys().cloned().collect();
            let files_verified = verified_files.len();
            return Ok(RepairResult::NoRepairNeeded {
                files_verified,
                verified_files,
                message: "All files are already present and valid.".to_string(),
            });
        }

        debug!(
            "  total_damaged_blocks: {}, recovery_blocks: {}",
            total_damaged_blocks,
            self.recovery_set.recovery_slices_metadata.len()
        );

        // Report recovery block information
        if total_damaged_blocks > 0 {
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
        }

        // Perform the actual repair with validation cache
        self.perform_reed_solomon_repair(&file_status, &validation_cache)
    }

    /// Perform repair operation
    pub fn repair(&self) -> Result<RepairResult> {
        self.repair_with_slices()
    }

    /// Perform Reed-Solomon repair
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

            let missing_slices: Vec<usize> = (0..file_info.slice_count)
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
        let reconstructed_data: HashMap<usize, Vec<u8>> =
            self.reconstruct_all_missing_slices(&files_to_repair, validation_cache)?;

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

            let file_path = self.base_path.join(&file_info.file_name);
            match self.write_repaired_file(
                &file_path,
                file_info,
                valid_slice_indices,
                &file_reconstructed,
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

        // CRITICAL: After repair, we MUST verify that repaired files are now correct
        // If we repaired files but can't verify them, that's a FAILURE
        if files_repaired_count > 0 {
            self.reporter().report_verification_header();

            let mut verified_after_repair = Vec::new();
            let mut failed_verification = Vec::new();

            for repaired_file in &repaired_files {
                // Find the file info for this repaired file
                let file_info = self
                    .recovery_set
                    .files
                    .iter()
                    .find(|f| &f.file_name == repaired_file)
                    .expect("Repaired file must exist in file list");

                eprintln!(
                    "Looking up repaired file '{}', found FileInfo: file_id={:?}, md5={}, global_offset={}",
                    repaired_file,
                    file_info.file_id,
                    hex::encode(file_info.md5_hash.as_bytes()),
                    file_info.global_slice_offset.as_usize()
                );

                let file_path = self.base_path.join(&file_info.file_name);

                // Verify the MD5 hash of the repaired file
                match crate::file_verification::calculate_file_md5(&file_path) {
                    Ok(computed_hash) if computed_hash == file_info.md5_hash => {
                        verified_after_repair.push(repaired_file.clone());
                        self.reporter().report_verification(
                            &file_info.file_name,
                            VerificationResult::Verified,
                        );
                    }
                    Ok(computed_hash) => {
                        failed_verification.push(repaired_file.clone());
                        eprintln!(
                            "MD5 mismatch after repair for {}: expected {}, got {}",
                            file_info.file_name,
                            hex::encode(file_info.md5_hash.as_bytes()),
                            hex::encode(computed_hash.as_bytes())
                        );
                        debug!(
                            "MD5 mismatch after repair for {}: expected {:?}, got {:?}",
                            file_info.file_name, file_info.md5_hash, computed_hash
                        );
                        self.reporter().report_verification(
                            &file_info.file_name,
                            VerificationResult::HashMismatch,
                        );
                    }
                    Err(e) => {
                        failed_verification.push(repaired_file.clone());
                        debug!(
                            "Failed to verify {} after repair: {}",
                            file_info.file_name, e
                        );
                        self.reporter().report_verification(
                            &file_info.file_name,
                            VerificationResult::SizeMismatch {
                                expected: file_info.file_length,
                                actual: 0,
                            }, // Using SizeMismatch as a generic error indicator
                        );
                    }
                }
            }

            // If any repaired files failed verification, that's a FAILURE
            if !failed_verification.is_empty() {
                let message = format!(
                    "Repair failed: {} file(s) repaired but {} failed verification",
                    files_repaired_count,
                    failed_verification.len()
                );
                return Ok(RepairResult::Failed {
                    files_failed: failed_verification,
                    files_verified: verified_files.len() + verified_after_repair.len(),
                    verified_files: [verified_files, verified_after_repair].concat(),
                    message,
                });
            }

            // Success: all repaired files verified correctly
            let total_verified = verified_files.len() + verified_after_repair.len();
            return Ok(RepairResult::Success {
                files_repaired: files_repaired_count,
                files_verified: total_verified,
                repaired_files,
                verified_files: [verified_files, verified_after_repair].concat(),
                message: format!("Successfully repaired {} file(s)", files_repaired_count),
            });
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
    ) -> Result<HashMap<usize, Vec<u8>>> {
        use crate::slice_provider::{ChunkedSliceProvider, RecoverySliceProvider, SliceLocation};
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
        let mut input_provider = ChunkedSliceProvider::new(self.recovery_set.slice_size as usize);

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

            for slice_index in 0..file_info.slice_count {
                if !valid_slices.contains(&slice_index) {
                    continue; // Skip invalid slices
                }

                let global_index = file_info.local_to_global(LocalSliceIndex::new(slice_index));
                let offset = (slice_index * self.recovery_set.slice_size as usize) as u64;
                let actual_size = if slice_index == file_info.slice_count - 1 {
                    let remaining = file_info.file_length % self.recovery_set.slice_size;
                    if remaining == 0 {
                        self.recovery_set.slice_size as usize
                    } else {
                        remaining as usize
                    }
                } else {
                    self.recovery_set.slice_size as usize
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
                        file_path: file_path.clone(),
                        offset,
                        actual_size: ActualDataSize::new(actual_size),
                        logical_size: LogicalSliceSize::new(self.recovery_set.slice_size as usize),
                        expected_crc,
                    },
                );
            }
        }

        // Build recovery slice provider
        let mut recovery_provider =
            RecoverySliceProvider::new(self.recovery_set.slice_size as usize);

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

        let total_input_slices: usize = self.recovery_set.files.iter().map(|f| f.slice_count).sum();
        let reconstruction_engine = crate::reed_solomon::ReconstructionEngine::new(
            self.recovery_set.slice_size as usize,
            total_input_slices,
            dummy_recovery_slices,
        );

        // Create output buffers for all missing slices
        let mut output_buffers: HashMap<usize, Cursor<Vec<u8>>> = HashMap::default();
        for &global_idx in &all_missing_global {
            output_buffers.insert(global_idx, Cursor::new(Vec::new()));
        }

        // Perform reconstruction
        let result = reconstruction_engine.reconstruct_missing_slices_chunked(
            &mut input_provider,
            &recovery_provider,
            &all_missing_global,
            &mut output_buffers,
            64 * 1024,
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
        let valid_slices = crate::validation::validate_slices_crc32(
            &file_path,
            &crc_checksums,
            self.recovery_set.slice_size as usize,
            file_info.file_length,
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
    ) -> Result<()> {
        debug!("Writing repaired file with streaming I/O: {:?}", file_path);

        // Write to temp file first, then rename to avoid corrupting source while reading
        let temp_path = file_path.with_extension("par2_tmp");

        // Open source file for reading valid slices
        let source_path = self.base_path.join(&file_info.file_name);
        let mut source_file = if source_path.exists() {
            Some(
                File::open(&source_path).map_err(|source| RepairError::FileOpenError {
                    file: source_path.clone(),
                    source,
                })?,
            )
        } else {
            None
        };

        // Create temp output file
        let file = File::create(&temp_path).map_err(|source| RepairError::FileCreateError {
            file: temp_path.clone(),
            source,
        })?;
        let mut writer = std::io::BufWriter::with_capacity(1024 * 1024, file);

        let slice_size = self.recovery_set.slice_size as usize;
        let mut slice_buffer = vec![0u8; slice_size];
        let mut bytes_written = 0u64;
        let mut next_expected_offset: Option<u64> = Some(0);

        for slice_index in 0..file_info.slice_count {
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
                writer
                    .write_all(&reconstructed_data[..actual_size])
                    .map_err(|e| RepairError::SliceWriteError {
                        file: temp_path.clone(),
                        slice_index,
                        source: e,
                    })?;
                bytes_written += actual_size as u64;
                // Mark that we've broken the sequential read pattern
                next_expected_offset = None;
            } else if valid_slice_indices.contains(&slice_index) {
                // Read from source file
                if let Some(ref mut file) = source_file {
                    let offset = (slice_index * slice_size) as u64;

                    // Only seek if we're not already at the right position (optimize sequential reads)
                    if next_expected_offset != Some(offset) {
                        file.seek(SeekFrom::Start(offset)).map_err(|e| {
                            RepairError::FileSeekError {
                                file: file_path.to_path_buf(),
                                offset,
                                source: e,
                            }
                        })?;
                    }

                    file.read_exact(&mut slice_buffer[..actual_size])
                        .map_err(|e| RepairError::SliceReadError {
                            file: file_path.to_path_buf(),
                            slice_index,
                            source: e,
                        })?;
                    writer
                        .write_all(&slice_buffer[..actual_size])
                        .map_err(|e| RepairError::SliceWriteError {
                            file: temp_path.clone(),
                            slice_index,
                            source: e,
                        })?;
                    bytes_written += actual_size as u64;
                    next_expected_offset = Some(offset + actual_size as u64);
                } else {
                    return Err(RepairError::ValidSliceMissingSource(slice_index));
                }
            } else {
                return Err(RepairError::SliceNotAvailable(slice_index));
            }
        }

        writer.flush().map_err(|e| RepairError::FileFlushError {
            file: temp_path.clone(),
            source: e,
        })?;
        drop(writer); // Close the file before rename
        drop(source_file); // Close source file before rename

        if bytes_written != file_info.file_length {
            return Err(RepairError::ByteCountMismatch {
                written: bytes_written,
                expected: file_info.file_length,
            });
        }

        // Rename temp file to final destination
        fs::rename(&temp_path, file_path).map_err(|e| RepairError::FileRenameError {
            temp_path: temp_path.clone(),
            final_path: file_path.to_path_buf(),
            source: e,
        })?;

        debug!("Wrote {} bytes to {:?}", bytes_written, file_path);
        Ok(())
    }
}

/// High-level repair function - loads PAR2 files and performs repair
///
/// This is the main entry point for repair operations. It loads the PAR2 file,
/// creates a repair context with a console reporter, and performs the repair operation.
///
/// # Arguments
/// * `par2_file` - Path to the PAR2 file
///
/// # Returns
/// * `Ok((RepairContext, RepairResult))` - Repair operation completed with context and result
/// * `Err(...)` - Failed to load PAR2 files or create repair context
pub fn repair_files(par2_file: &str) -> Result<(RepairContext, RepairResult)> {
    repair_files_with_reporter(par2_file, Box::new(ConsoleReporter::new(false)))
}

/// High-level repair function with custom progress reporter
///
/// Allows specifying a custom progress reporter (e.g., SilentReporter for tests,
/// ConsoleReporter with quiet flag, or custom implementations).
///
/// # Arguments
/// * `par2_file` - Path to the PAR2 file
/// * `reporter` - Progress reporter implementation
///
/// # Returns
/// * `Ok((RepairContext, RepairResult))` - Repair operation completed with context and result
/// * `Err(...)` - Failed to load PAR2 files or create repair context
pub fn repair_files_with_reporter(
    par2_file: &str,
    reporter: Box<dyn ProgressReporter>,
) -> Result<(RepairContext, RepairResult)> {
    let par2_path = Path::new(par2_file);

    // Validate file exists
    if !par2_path.exists() {
        return Err(RepairError::FileNotFound(par2_file.to_string()));
    }

    // Collect all PAR2 files in the set
    let par2_files = crate::file_ops::collect_par2_files(par2_path);

    // Load metadata for memory-efficient recovery slice loading
    let metadata = crate::file_ops::parse_recovery_slice_metadata(&par2_files, false);

    // Load packets WITHOUT recovery slices (they're loaded via metadata on-demand)
    let packets = crate::file_ops::load_par2_packets(&par2_files, true);

    if packets.is_empty() {
        return Err(RepairError::NoValidPackets);
    }

    // Get the base directory for file resolution
    let base_path = par2_path.parent().unwrap_or(Path::new(".")).to_path_buf();

    // Create repair context using builder
    let repair_context = RepairContextBuilder::new()
        .packets(packets)
        .metadata(metadata)
        .base_path(base_path)
        .reporter(reporter)
        .build()?;

    // Report statistics before starting
    repair_context
        .reporter()
        .report_statistics(&repair_context.recovery_set);

    let result = repair_context.repair()?;

    Ok((repair_context, result))
}
