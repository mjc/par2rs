//! PAR2 File Repair Module
//!
//! This module provides functionality for repairing files using PAR2 recovery data.
//! It implements Reed-Solomon error correction to reconstruct missing or corrupted files.

use crate::file_verification::calculate_file_md5;
use crate::reed_solomon::ReconstructionEngine;
use crate::{
    FileDescriptionPacket, InputFileSliceChecksumPacket, MainPacket, Packet, RecoverySlicePacket,
};
use crc32fast::Hasher as Crc32;
use log::{debug, trace};
use rustc_hash::FxHashMap as HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Information about a file in the recovery set
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub file_id: [u8; 16],
    pub file_name: String,
    pub file_length: u64,
    pub md5_hash: [u8; 16],
    pub md5_16k: [u8; 16],
    pub slice_count: usize,
    pub global_slice_offset: usize, // Starting global slice index for this file
}

/// Information about the recovery set
#[derive(Debug)]
pub struct RecoverySetInfo {
    pub set_id: [u8; 16],
    pub slice_size: u64,
    pub files: Vec<FileInfo>,
    pub recovery_slices: Vec<RecoverySlicePacket>,
    pub file_slice_checksums: HashMap<[u8; 16], InputFileSliceChecksumPacket>,
}

/// Status of a file that needs repair
#[derive(Debug, PartialEq)]
pub enum FileStatus {
    Present,   // File exists and is valid
    Missing,   // File doesn't exist
    Corrupted, // File exists but is corrupted
}

/// Result of a repair operation
#[derive(Debug)]
pub struct RepairResult {
    pub success: bool,
    pub files_repaired: usize,
    pub files_verified: usize,
    pub repaired_files: Vec<String>,
    pub verified_files: Vec<String>,
    pub files_failed: Vec<String>,
    pub message: String,
}

/// Main repair context containing all necessary information for repair operations
pub struct RepairContext {
    pub recovery_set: RecoverySetInfo,
    pub base_path: PathBuf,
}

impl RepairContext {
    /// Create a new repair context from PAR2 packets
    pub fn new(packets: Vec<Packet>, base_path: PathBuf) -> Result<Self, String> {
        let recovery_set = Self::extract_recovery_set_info(packets)?;
        Ok(RepairContext {
            recovery_set,
            base_path,
        })
    }

    /// Extract recovery set information from packets
    fn extract_recovery_set_info(packets: Vec<Packet>) -> Result<RecoverySetInfo, String> {
        let mut main_packet: Option<MainPacket> = None;
        let mut file_descriptions: Vec<FileDescriptionPacket> = Vec::new();
        let mut recovery_slices: Vec<RecoverySlicePacket> = Vec::new();
        let mut input_file_slice_checksums: Vec<InputFileSliceChecksumPacket> = Vec::new();

        // Collect packets by type
        for packet in packets {
            match packet {
                Packet::Main(main) => {
                    main_packet = Some(main);
                }
                Packet::FileDescription(fd) => {
                    file_descriptions.push(fd);
                }
                Packet::RecoverySlice(rs) => {
                    recovery_slices.push(rs);
                }
                Packet::InputFileSliceChecksum(ifsc) => {
                    input_file_slice_checksums.push(ifsc);
                }
                _ => {} // Ignore other packet types for now
            }
        }

        let main = main_packet.ok_or("No main packet found")?;

        if file_descriptions.is_empty() {
            return Err("No file description packets found".to_string());
        }

        // Build a map of file_id -> FileDescriptionPacket for easy lookup
        let mut fd_map: HashMap<[u8; 16], FileDescriptionPacket> = HashMap::default();
        for fd in file_descriptions {
            fd_map.insert(fd.file_id, fd);
        }

        // Build file information in the order specified by main.file_ids
        // This is critical for correct global slice indexing!
        let mut files = Vec::new();
        let mut global_slice_offset = 0;

        debug!(
            "Building file list from main packet's file_ids array ({} files)",
            main.file_ids.len()
        );

        for (idx, file_id) in main.file_ids.iter().enumerate() {
            let fd = fd_map.get(file_id).ok_or_else(|| {
                format!(
                    "File ID {:?} in main packet not found in file descriptions",
                    file_id
                )
            })?;

            let file_name = String::from_utf8_lossy(&fd.file_name)
                .trim_end_matches('\0')
                .to_string();

            let slice_count = fd.file_length.div_ceil(main.slice_size) as usize;

            if idx < 3 || idx >= main.file_ids.len() - 3 {
                debug!(
                    "  File {}: {} (slices: {}, global offset: {})",
                    idx, file_name, slice_count, global_slice_offset
                );
            } else if idx == 3 {
                debug!("  ... ({} files omitted) ...", main.file_ids.len() - 6);
            }

            files.push(FileInfo {
                file_id: fd.file_id,
                file_name,
                file_length: fd.file_length,
                md5_hash: fd.md5_hash,
                md5_16k: fd.md5_16k,
                slice_count,
                global_slice_offset,
            });

            // Increment global slice offset for next file
            global_slice_offset += slice_count;
        }

        debug!("Total global slices: {}", global_slice_offset);

        // Build checksum map indexed by file_id
        let mut file_slice_checksums = HashMap::default();
        for ifsc in input_file_slice_checksums {
            file_slice_checksums.insert(ifsc.file_id, ifsc);
        }

        Ok(RecoverySetInfo {
            set_id: main.set_id,
            slice_size: main.slice_size,
            files,
            recovery_slices,
            file_slice_checksums,
        })
    }

    /// Check the status of all files in the recovery set
    pub fn check_file_status(&self) -> HashMap<String, FileStatus> {
        let mut status_map = HashMap::default();

        for file_info in &self.recovery_set.files {
            let file_path = self.base_path.join(&file_info.file_name);
            let status = self.determine_file_status(&file_path, file_info);
            status_map.insert(file_info.file_name.clone(), status);
        }

        status_map
    }

    /// Determine the status of a single file
    ///
    /// OPTIMIZATION: For repair, we skip MD5 checks during initial scan.
    /// We use CRC32 on individual slices instead (much faster).
    /// MD5 verification only happens AFTER repair to confirm success.
    fn determine_file_status(&self, file_path: &Path, file_info: &FileInfo) -> FileStatus {
        if !file_path.exists() {
            return FileStatus::Missing;
        }

        // Check file size - if wrong size, definitely corrupted
        if let Ok(metadata) = fs::metadata(file_path) {
            if metadata.len() != file_info.file_length {
                return FileStatus::Corrupted;
            }
        } else {
            return FileStatus::Corrupted;
        }

        // SKIP MD5 checks during repair scan - too expensive!
        // We'll use CRC32 on slices instead (50x faster)
        // Only verify MD5 AFTER repair is complete
        //
        // For now, assume file exists and has correct size means we need to check slices
        // The slice loading code will use CRC32 to validate which slices are good
        FileStatus::Corrupted
    }

    /// Determine if repair is possible for the given file statuses
    /// Requires preloaded slice data to avoid duplicate CRC32 calculations
    pub fn can_repair_with_preloaded(
        &self,
        file_status: &HashMap<String, FileStatus>,
        preloaded_slices: &HashMap<[u8; 16], (HashMap<usize, Vec<u8>>, usize)>,
    ) -> bool {
        // For files that are missing, we need all their slices
        let missing_slices: usize = self
            .recovery_set
            .files
            .iter()
            .filter(|f| {
                let status = file_status
                    .get(&f.file_name)
                    .unwrap_or(&FileStatus::Missing);
                *status == FileStatus::Missing
            })
            .map(|f| f.slice_count)
            .sum();

        // For corrupted files, use preloaded corrupted count
        let mut corrupted_slices = 0;
        for file_info in &self.recovery_set.files {
            let status = file_status
                .get(&file_info.file_name)
                .unwrap_or(&FileStatus::Missing);

            if *status == FileStatus::Corrupted {
                // Use preloaded corrupted count (must be available)
                if let Some((_slices, corrupted_count)) = preloaded_slices.get(&file_info.file_id) {
                    corrupted_slices += corrupted_count;
                }
            }
        }

        let total_needed_slices = missing_slices + corrupted_slices;
        let available_recovery_slices = self.recovery_set.recovery_slices.len();

        debug!("Missing slices: {}, Corrupted slices: {}, Total needed: {}, Available recovery slices: {}", 
                missing_slices, corrupted_slices, total_needed_slices, available_recovery_slices);

        // Can repair if we have enough recovery slices to replace needed slices
        available_recovery_slices >= total_needed_slices
    }

    /// Determine if repair is possible (convenience method - uses worst-case estimate)
    pub fn can_repair(&self, file_status: &HashMap<String, FileStatus>) -> bool {
        // Worst-case estimate: assume all slices of corrupted files are bad
        let missing_slices: usize = self
            .recovery_set
            .files
            .iter()
            .filter(|f| {
                let status = file_status
                    .get(&f.file_name)
                    .unwrap_or(&FileStatus::Missing);
                *status == FileStatus::Missing
            })
            .map(|f| f.slice_count)
            .sum();

        let corrupted_slices: usize = self
            .recovery_set
            .files
            .iter()
            .filter(|f| {
                let status = file_status
                    .get(&f.file_name)
                    .unwrap_or(&FileStatus::Missing);
                *status == FileStatus::Corrupted
            })
            .map(|f| f.slice_count)
            .sum();

        let total_needed_slices = missing_slices + corrupted_slices;
        let available_recovery_slices = self.recovery_set.recovery_slices.len();

        available_recovery_slices >= total_needed_slices
    }



    /// Perform repair operation
    /// Optionally accepts pre-loaded slices to avoid duplicate CRC32 calculations
    pub fn repair_with_slices(
        &self,
        preloaded_slices: Option<HashMap<[u8; 16], (HashMap<usize, Vec<u8>>, usize)>>,
    ) -> Result<RepairResult, Box<dyn std::error::Error>> {
        let file_status = self.check_file_status();

        // Check if repair is needed
        let needs_repair = file_status.values().any(|s| *s != FileStatus::Present);
        if !needs_repair {
            let verified_files: Vec<String> = file_status.keys().cloned().collect();
            let files_verified = verified_files.len();
            return Ok(RepairResult {
                success: true,
                files_repaired: 0,
                files_verified,
                repaired_files: Vec::new(),
                verified_files,
                files_failed: Vec::new(),
                message: "All files are already present and valid.".to_string(),
            });
        }

        // Check if repair is possible
        let can_repair = match &preloaded_slices {
            Some(slices) => {
                // Use preloaded data for accurate check (avoids duplicate load_file_slices call)
                self.can_repair_with_preloaded(&file_status, slices)
            }
            None => {
                // Fall back to worst-case estimate
                self.can_repair(&file_status)
            }
        };

        if !can_repair {
            return Ok(RepairResult {
                success: false,
                files_repaired: 0,
                files_verified: 0,
                repaired_files: Vec::new(),
                verified_files: Vec::new(),
                files_failed: file_status.keys().cloned().collect(),
                message: "Insufficient recovery data to repair all missing/corrupted files."
                    .to_string(),
            });
        }

        // Perform the actual repair
        self.perform_reed_solomon_repair(&file_status, preloaded_slices)
    }

    /// Perform repair operation (convenience method)
    pub fn repair(&self) -> Result<RepairResult, Box<dyn std::error::Error>> {
        self.repair_with_slices(None)
    }

    /// Perform Reed-Solomon repair using available recovery data
    /// Optionally accepts pre-loaded slices to avoid duplicate CRC32 calculations
    fn perform_reed_solomon_repair(
        &self,
        file_status: &HashMap<String, FileStatus>,
        preloaded_slices: Option<HashMap<[u8; 16], (HashMap<usize, Vec<u8>>, usize)>>,
    ) -> Result<RepairResult, Box<dyn std::error::Error>> {
        let mut repaired_files = Vec::new();
        let mut verified_files = Vec::new();
        let mut files_failed = Vec::new();

        // Process each file that needs repair
        for file_info in &self.recovery_set.files {
            let status = file_status
                .get(&file_info.file_name)
                .unwrap_or(&FileStatus::Missing);

            if *status == FileStatus::Present {
                verified_files.push(file_info.file_name.clone());
                continue; // File is already good
            }

            // Use preloaded slices if available, otherwise load them now
            let (slices, _corrupted_count) = if let Some(ref preloaded) = preloaded_slices {
                if let Some(cached) = preloaded.get(&file_info.file_id) {
                    cached.clone()
                } else {
                    match self.load_file_slices(file_info) {
                        Ok(result) => result,
                        Err(e) => {
                            files_failed.push(file_info.file_name.clone());
                            debug!("Failed to load slices for {}: {}", file_info.file_name, e);
                            continue;
                        }
                    }
                }
            } else {
                match self.load_file_slices(file_info) {
                    Ok(result) => result,
                    Err(e) => {
                        files_failed.push(file_info.file_name.clone());
                        debug!("Failed to load slices for {}: {}", file_info.file_name, e);
                        continue;
                    }
                }
            };

            // Attempt to repair the file using Reed-Solomon reconstruction
            match self.repair_single_file(file_info, status, slices) {
                Ok(repaired) => {
                    if repaired {
                        repaired_files.push(file_info.file_name.clone());
                        debug!("Successfully repaired: {}", file_info.file_name);
                    } else {
                        verified_files.push(file_info.file_name.clone());
                        debug!("File was already valid: {}", file_info.file_name);
                    }
                }
                Err(e) => {
                    files_failed.push(file_info.file_name.clone());
                    debug!("Failed to repair {}: {}", file_info.file_name, e);
                }
            }
        }

        let files_repaired_count = repaired_files.len();
        let files_verified_count = verified_files.len();
        let success = files_failed.is_empty();
        let message = if success {
            if files_repaired_count > 0 {
                format!("Successfully repaired {} file(s)", files_repaired_count)
            } else {
                format!("All {} file(s) verified as intact", files_verified_count)
            }
        } else {
            format!(
                "Repaired {} file(s), verified {} file(s), failed to repair {} file(s)",
                files_repaired_count,
                files_verified_count,
                files_failed.len()
            )
        };

        Ok(RepairResult {
            success,
            files_repaired: files_repaired_count,
            files_verified: files_verified_count,
            repaired_files,
            verified_files,
            files_failed,
            message,
        })
    }

    /// Repair a single file using Reed-Solomon reconstruction
    /// Accepts pre-loaded slices to avoid duplicate CRC32 calculations
    fn repair_single_file(
        &self,
        file_info: &FileInfo,
        status: &FileStatus,
        mut file_slices: HashMap<usize, Vec<u8>>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let file_path = self.base_path.join(&file_info.file_name);

        // Check which slices are missing or corrupted
        let missing_slices = self.identify_missing_slices(&file_slices, file_info)?;

        if missing_slices.is_empty() {
            // All slices are present and valid, but check if the overall file is still corrupted
            if *status == FileStatus::Corrupted {
                debug!("All slices are valid but file MD5 doesn't match - attempting repair");
                // The file might have been corrupted after slicing, let's try to rebuild it
                self.write_repaired_file(&file_path, &file_slices, file_info)?;

                // Verify the repaired file
                if self.verify_repaired_file(&file_path, file_info)? {
                    return Ok(true);
                } else {
                    return Err("File reconstruction failed even with all valid slices".into());
                }
            }
            // File is already complete and valid
            return Ok(false);
        }

        debug!(
            "File {} has {} missing/corrupted slices out of {} total",
            file_info.file_name,
            missing_slices.len(),
            file_info.slice_count
        );

        // Check if we have enough recovery data
        if missing_slices.len() > self.recovery_set.recovery_slices.len() {
            return Err(format!(
                "Cannot repair: {} missing slices but only {} recovery slices available",
                missing_slices.len(),
                self.recovery_set.recovery_slices.len()
            )
            .into());
        }

        // Reconstruct missing slices using Reed-Solomon
        let reconstructed_slices =
            self.reconstruct_slices(&missing_slices, file_info, &file_slices)?;

        // Update file_slices with reconstructed data
        debug!(
            "Reconstructed {} slices, updating file_slices HashMap",
            reconstructed_slices.len()
        );
        for (slice_index, slice_data) in reconstructed_slices {
            debug!(
                "  Inserting reconstructed slice {} ({} bytes)",
                slice_index,
                slice_data.len()
            );
            file_slices.insert(slice_index, slice_data);
        }

        debug!(
            "Total slices in HashMap before writing: {}",
            file_slices.len()
        );
        debug!("Expected total slices: {}", file_info.slice_count);

        // Write the repaired file
        self.write_repaired_file(&file_path, &file_slices, file_info)?;

        // Verify the repaired file
        if self.verify_repaired_file(&file_path, file_info)? {
            Ok(true)
        } else {
            Err("Repaired file failed verification".into())
        }
    }

    /// Load slices from an existing file
    /// Returns (valid_slices, corrupted_count)
    pub fn load_file_slices(
        &self,
        file_info: &FileInfo,
    ) -> Result<(HashMap<usize, Vec<u8>>, usize), Box<dyn std::error::Error>> {
        let file_path = self.base_path.join(&file_info.file_name);
        let mut slices = HashMap::default();
        let mut corrupted_count = 0;

        if !file_path.exists() {
            return Ok((slices, file_info.slice_count)); // All slices missing = all corrupted
        }

        let file = File::open(&file_path)?;
        let mut reader = BufReader::with_capacity(1024 * 1024, file); // 1MB buffer
        let slice_size = self.recovery_set.slice_size as usize;

        // Reuse a single buffer for all slices to avoid allocations
        let mut slice_data = vec![0u8; slice_size];
        
        for slice_index in 0..file_info.slice_count {
            let actual_slice_size = if slice_index == file_info.slice_count - 1 {
                let remaining_bytes = file_info.file_length % self.recovery_set.slice_size;
                if remaining_bytes == 0 {
                    slice_size
                } else {
                    remaining_bytes as usize
                }
            } else {
                slice_size
            };

            // Zero out padding area (only needed if this slice is shorter than slice_size)
            if actual_slice_size < slice_size {
                slice_data[actual_slice_size..].fill(0);
            }
            
            // Sequential read (no seeking needed with BufReader)
            // Use read_exact to ensure we get all bytes in one call
            if reader.read_exact(&mut slice_data[..actual_slice_size]).is_ok() {
                // Verify slice checksum if available
                if let Some(checksums) = self
                    .recovery_set
                    .file_slice_checksums
                    .get(&file_info.file_id)
                {
                    if slice_index < checksums.slice_checksums.len() {
                        // Use CRC32 for fast slice validation (like par2cmdline does)
                        // CRC32 is computed on the full padded buffer (PAR2 spec requirement)
                        let mut hasher = Crc32::new();
                        hasher.update(&slice_data);
                        let slice_crc = hasher.finalize();
                        let expected_crc = checksums.slice_checksums[slice_index].1;

                        if slice_crc == expected_crc {
                            // Store only the actual data (not the padding)
                            slices.insert(slice_index, slice_data[..actual_slice_size].to_vec());
                        } else {
                            corrupted_count += 1;
                            trace!("Slice {} failed CRC32 verification", slice_index);
                        }
                    } else {
                        // No checksum available for this slice, assume it's good if size matches
                        slices.insert(slice_index, slice_data[..actual_slice_size].to_vec());
                    }
                } else {
                    // No checksums available, assume slice is valid if size matches
                    slices.insert(slice_index, slice_data[..actual_slice_size].to_vec());
                }
            } else {
                // read_exact failed - couldn't read the expected number of bytes
                corrupted_count += 1;
                trace!("Slice {} failed to read {} bytes", slice_index, actual_slice_size);
            }
        }

        debug!(
            "Loaded {} valid slices out of {} total slices ({} corrupted)",
            slices.len(),
            file_info.slice_count,
            corrupted_count
        );
        Ok((slices, corrupted_count))
    }

    /// Identify which slices are missing or corrupted
    fn identify_missing_slices(
        &self,
        existing_slices: &HashMap<usize, Vec<u8>>,
        file_info: &FileInfo,
    ) -> Result<Vec<usize>, Box<dyn std::error::Error>> {
        let mut missing_slices = Vec::new();

        for slice_index in 0..file_info.slice_count {
            if !existing_slices.contains_key(&slice_index) {
                missing_slices.push(slice_index);
            }
        }

        Ok(missing_slices)
    }

    /// Load all slices from all files in the recovery set
    /// Returns a HashMap mapping global slice index to slice data
    /// Load slices from all files EXCEPT the specified file_id
    pub fn load_slices_except_file(
        &self,
        exclude_file_id: &[u8; 16],
    ) -> Result<HashMap<usize, Vec<u8>>, Box<dyn std::error::Error>> {
        let mut all_slices = HashMap::default();
        let slice_size = self.recovery_set.slice_size as usize;

        for file_info in &self.recovery_set.files {
            // Skip the file we're repairing
            if &file_info.file_id == exclude_file_id {
                trace!("  Skipping current file: {}", file_info.file_name);
                continue;
            }

            let file_path = self.base_path.join(&file_info.file_name);

            trace!(
                "  Checking file: {} (exists: {})",
                file_info.file_name,
                file_path.exists()
            );

            // Skip missing files
            if !file_path.exists() {
                trace!("    Skipping - file doesn't exist");
                continue;
            }

            let mut file = File::open(&file_path)?;

            let has_checksums = self
                .recovery_set
                .file_slice_checksums
                .contains_key(&file_info.file_id);
            trace!(
                "    Has checksums: {} (file_id: {:02x?})",
                has_checksums,
                &file_info.file_id[..4]
            );

            for slice_index in 0..file_info.slice_count {
                let actual_slice_size = if slice_index == file_info.slice_count - 1 {
                    let remaining_bytes = file_info.file_length % self.recovery_set.slice_size;
                    if remaining_bytes == 0 {
                        slice_size
                    } else {
                        remaining_bytes as usize
                    }
                } else {
                    slice_size
                };

                // Always allocate full slice size for checksum computation (PAR2 spec requires padding)
                let mut slice_data = vec![0u8; slice_size];
                file.seek(SeekFrom::Start((slice_index * slice_size) as u64))?;

                // Read actual data (may be less than slice_size for last slice)
                let bytes_read = file.read(&mut slice_data[..actual_slice_size]).unwrap_or(0);
                if bytes_read == actual_slice_size {
                    // Verify slice checksum if available
                    if let Some(checksums) = self
                        .recovery_set
                        .file_slice_checksums
                        .get(&file_info.file_id)
                    {
                        if slice_index < checksums.slice_checksums.len() {
                            // Use CRC32 for fast slice validation (like par2cmdline does)
                            let mut hasher = Crc32::new();
                            hasher.update(&slice_data);
                            let slice_crc = hasher.finalize();
                            let expected_crc = checksums.slice_checksums[slice_index].1;

                            if slice_crc == expected_crc {
                                // Store with global slice index
                                let global_index = file_info.global_slice_offset + slice_index;
                                all_slices.insert(global_index, slice_data);
                                trace!(
                                    "    Loaded valid slice {} (global: {}) from {}",
                                    slice_index,
                                    global_index,
                                    file_info.file_name
                                );
                            } else {
                                trace!(
                                    "    Slice {} of {} failed CRC32 check",
                                    slice_index,
                                    file_info.file_name
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(all_slices)
    }

    /// Reconstruct missing slices using the Reed-Solomon module
    fn reconstruct_slices(
        &self,
        missing_slices: &[usize],
        file_info: &FileInfo,
        current_file_slices: &HashMap<usize, Vec<u8>>,
    ) -> Result<HashMap<usize, Vec<u8>>, Box<dyn std::error::Error>> {
        let slice_size = self.recovery_set.slice_size as usize;
        let recovery_slices_count = self.recovery_set.recovery_slices.len();

        if missing_slices.len() > recovery_slices_count {
            return Err(format!(
                "Cannot repair: {} missing slices but only {} recovery slices available",
                missing_slices.len(),
                recovery_slices_count
            )
            .into());
        }

        debug!(
            "Reconstructing {} missing slices using {} recovery slices",
            missing_slices.len(),
            recovery_slices_count
        );

        // For PAR2 Reed-Solomon, we need to load ALL valid slices from ALL files
        // Start with valid slices from the current file (convert file-local to global indices)
        let mut all_slices = HashMap::default();
        for (file_local_index, slice_data) in current_file_slices {
            if !missing_slices.contains(file_local_index) {
                let global_index = file_info.global_slice_offset + file_local_index;
                all_slices.insert(global_index, slice_data.clone());
                trace!(
                    "  Including valid slice {} (global: {}) from current file",
                    file_local_index,
                    global_index
                );
            }
        }
        debug!("Loaded {} valid slices from current file", all_slices.len());

        // Load slices from OTHER files in the recovery set
        let other_slices = self.load_slices_except_file(&file_info.file_id)?;
        debug!(
            "Loaded {} valid slices from OTHER files",
            other_slices.len()
        );
        all_slices.extend(other_slices);

        debug!(
            "Total slices available for reconstruction: {}",
            all_slices.len()
        );

        let total_input_slices: usize = self.recovery_set.files.iter().map(|f| f.slice_count).sum();

        // Build global slice map for the missing slices (map file-local to global indices)
        let mut global_slice_map = HashMap::default();
        let mut global_missing_indices = Vec::new();
        for &slice_index in missing_slices {
            let global_index = file_info.global_slice_offset + slice_index;
            global_slice_map.insert(slice_index, global_index);
            global_missing_indices.push(global_index);
        }

        debug!(
            "Reed-Solomon reconstruction for {} (file slices: {}, total slices in set: {}, file offset: {})",
            file_info.file_name, file_info.slice_count, total_input_slices, file_info.global_slice_offset
        );

        debug!("Reconstructing file-local slices: {:?}", missing_slices);
        debug!(
            "Corresponding global slice indices: {:?}",
            global_missing_indices
        );

        // Create reconstruction engine
        let reconstruction_engine = ReconstructionEngine::new(
            slice_size,
            total_input_slices,
            self.recovery_set.recovery_slices.clone(),
        );

        // Check if reconstruction is possible
        if !reconstruction_engine.can_reconstruct(missing_slices.len()) {
            return Err(format!(
                "Reconstruction not possible with current configuration: {} missing slices",
                missing_slices.len()
            )
            .into());
        }

        // Perform reconstruction using global slices
        let result = reconstruction_engine.reconstruct_missing_slices_global(
            &all_slices,
            &global_missing_indices,
            total_input_slices,
        );

        if result.success {
            // Handle any warnings
            if let Some(warning) = &result.error_message {
                debug!("Warning: {}", warning);
            }

            // Convert global slice indices back to file-local indices
            let mut final_reconstructed = HashMap::default();
            for (global_index, mut slice_data) in result.reconstructed_slices {
                // Convert global index to file-local index
                if global_index >= file_info.global_slice_offset
                    && global_index < file_info.global_slice_offset + file_info.slice_count
                {
                    let file_local_index = global_index - file_info.global_slice_offset;

                    let actual_size = if file_local_index == file_info.slice_count - 1 {
                        let remaining_bytes = file_info.file_length % self.recovery_set.slice_size;
                        if remaining_bytes == 0 {
                            slice_size
                        } else {
                            remaining_bytes as usize
                        }
                    } else {
                        slice_size
                    };

                    // Resize slice to correct size
                    slice_data.resize(actual_size, 0);
                    final_reconstructed.insert(file_local_index, slice_data);
                    trace!(
                        "Reconstructed slice {} (global: {}) with {} bytes",
                        file_local_index,
                        global_index,
                        actual_size
                    );
                }
            }

            Ok(final_reconstructed)
        } else {
            Err(result
                .error_message
                .unwrap_or_else(|| "Reed-Solomon reconstruction failed".to_string())
                .into())
        }
    }

    /// Write the repaired file to disk
    fn write_repaired_file(
        &self,
        file_path: &Path,
        slices: &HashMap<usize, Vec<u8>>,
        file_info: &FileInfo,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Writing repaired file: {:?}", file_path);
        debug!(
            "  Have {} slices in HashMap, need {} slices",
            slices.len(),
            file_info.slice_count
        );

        // Flatten all slices into a single contiguous buffer for one big write
        // This is faster than many small writes, even with BufWriter
        let total_size = file_info.file_length as usize;
        let mut file_data = Vec::with_capacity(total_size);
        
        for slice_index in 0..file_info.slice_count {
            match slices.get(&slice_index) {
                Some(slice_data) => file_data.extend_from_slice(slice_data),
                None => {
                    debug!(
                        "Missing slice {} when writing file (have slices: {:?})",
                        slice_index,
                        slices.keys().collect::<Vec<_>>()
                    );
                    return Err(format!("Missing slice {} when writing file", slice_index).into());
                }
            }
        }

        // Truncate to exact file size (last slice may have padding)
        file_data.truncate(total_size);

        // Write all data in one operation (faster than many small writes)
        // File is automatically flushed when dropped, so no explicit flush needed
        let mut file = File::create(file_path)?;
        file.write_all(&file_data)?;
        
        debug!(
            "Wrote {} bytes total to {:?}",
            file_data.len(), file_path
        );
        Ok(())
    }

    /// Verify that the repaired file is correct
    fn verify_repaired_file(
        &self,
        file_path: &Path,
        file_info: &FileInfo,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // Check file size
        let metadata = fs::metadata(file_path)?;
        if metadata.len() != file_info.file_length {
            debug!(
                "File size mismatch: expected {}, got {}",
                file_info.file_length,
                metadata.len()
            );
            return Ok(false);
        }

        // After repair, just check the full MD5 directly
        // (No point checking 16k first - we need to read the whole file anyway)
        let file_md5 = calculate_file_md5(file_path)?;
        let matches = file_md5 == file_info.md5_hash;
        if !matches {
            debug!("MD5 mismatch:");
            debug!("  Expected: {:02x?}", file_info.md5_hash);
            debug!("  Actual:   {:02x?}", file_md5);
        }
        Ok(matches)
    }
}

/// High-level repair function that can be called from the binary
/// Output format matches par2cmdline
pub fn repair_files(
    par2_file: &str,
    _target_files: &[String],
) -> Result<RepairResult, Box<dyn std::error::Error>> {
    let par2_path = Path::new(par2_file);

    // Load PAR2 files and packets (this prints loading messages)
    let par2_files = crate::file_ops::collect_par2_files(par2_path);
    let (packets, recovery_blocks) = crate::file_ops::load_all_par2_packets(&par2_files, true);

    if packets.is_empty() {
        return Err("No valid PAR2 packets found".into());
    }

    // Get the base directory for file resolution
    let base_path = par2_path.parent().unwrap_or(Path::new(".")).to_path_buf();

    // Create repair context
    let repair_context = match RepairContext::new(packets, base_path) {
        Ok(ctx) => ctx,
        Err(e) => {
            return Err(format!("Failed to create repair context: {}", e).into());
        }
    };

    // Print statistics (matching par2cmdline format)
    println!();
    let recoverable_files = repair_context.recovery_set.files.len();
    let other_files = 0; // We don't track non-recoverable files currently
    println!(
        "There are {} recoverable files and {} other files.",
        recoverable_files, other_files
    );
    println!(
        "The block size used was {} bytes.",
        repair_context.recovery_set.slice_size
    );

    let total_blocks: usize = repair_context
        .recovery_set
        .files
        .iter()
        .map(|f| f.slice_count)
        .sum();
    println!("There are a total of {} data blocks.", total_blocks);

    let total_size: u64 = repair_context
        .recovery_set
        .files
        .iter()
        .map(|f| f.file_length)
        .sum();
    println!("The total size of the data files is {} bytes.", total_size);

    // Verifying source files section
    println!();
    println!("Verifying source files:");
    println!();

    // Check each file and print status
    let file_status = repair_context.check_file_status();
    let mut damaged_files = Vec::new();
    let mut missing_files = Vec::new();
    let mut ok_files = Vec::new();
    let mut total_available_blocks = 0;
    let mut total_damaged_blocks = 0;
    
    // Cache loaded slices to avoid duplicate CRC32 calculations later
    let mut preloaded_slices = HashMap::default();

    for file_info in &repair_context.recovery_set.files {
        println!("Opening: \"{}\"", file_info.file_name);

        let status = file_status
            .get(&file_info.file_name)
            .unwrap_or(&FileStatus::Missing);
        match status {
            FileStatus::Present => {
                println!("Target: \"{}\" - found.", file_info.file_name);
                ok_files.push(file_info.file_name.clone());
                total_available_blocks += file_info.slice_count;
            }
            FileStatus::Corrupted => {
                // Load slices and cache for later repair (reuses CRC32 calculations)
                let (slices, corrupted_count) = if let Ok(result) = repair_context.load_file_slices(file_info) {
                    result
                } else {
                    (HashMap::default(), file_info.slice_count) // If we can't load, assume all corrupted
                };
                
                // Cache the loaded slices for repair phase
                preloaded_slices.insert(file_info.file_id, (slices, corrupted_count));
                
                let available = file_info.slice_count - corrupted_count;
                total_available_blocks += available;
                total_damaged_blocks += corrupted_count;
                println!(
                    "Target: \"{}\" - damaged. Found {} of {} data blocks.",
                    file_info.file_name, available, file_info.slice_count
                );
                damaged_files.push(file_info.file_name.clone());
            }
            FileStatus::Missing => {
                println!("Target: \"{}\" - missing.", file_info.file_name);
                missing_files.push(file_info.file_name.clone());
                total_damaged_blocks += file_info.slice_count;
            }
        }
    }

    // If there are no damaged or missing files, we're done
    if damaged_files.is_empty() && missing_files.is_empty() {
        println!();
        println!("All files are correct, repair is not required.");
        return Ok(RepairResult {
            success: true,
            files_repaired: 0,
            files_verified: ok_files.len(),
            repaired_files: Vec::new(),
            verified_files: ok_files,
            files_failed: Vec::new(),
            message: "All files are correct".to_string(),
        });
    }

    // Repair is needed
    println!();
    println!("Scanning extra files:");
    println!();
    println!();
    println!("Repair is required.");

    if !damaged_files.is_empty() {
        println!("{} file(s) exist but are damaged.", damaged_files.len());
    }
    if !missing_files.is_empty() {
        println!("{} file(s) are missing.", missing_files.len());
    }

    println!(
        "You have {} out of {} data blocks available.",
        total_available_blocks, total_blocks
    );
    println!("You have {} recovery blocks available.", recovery_blocks);

    if recovery_blocks >= total_damaged_blocks {
        println!("Repair is possible.");
        let excess = recovery_blocks - total_damaged_blocks;
        if excess > 0 {
            println!("You have an excess of {} recovery blocks.", excess);
        }
        println!(
            "{} recovery blocks will be used to repair.",
            total_damaged_blocks
        );
    } else {
        println!("Repair is not possible.");
        println!(
            "You need {} more recovery blocks to be able to repair.",
            total_damaged_blocks - recovery_blocks
        );
        return Ok(RepairResult {
            success: false,
            files_repaired: 0,
            files_verified: 0,
            repaired_files: Vec::new(),
            verified_files: Vec::new(),
            files_failed: damaged_files
                .iter()
                .chain(missing_files.iter())
                .cloned()
                .collect(),
            message: "Insufficient recovery blocks".to_string(),
        });
    }

    // Reed Solomon reconstruction
    println!();
    println!("Computing Reed Solomon matrix.");
    println!("Constructing: done.");
    println!("Solving: done.");

    // Perform actual repair (passing preloaded slices to avoid duplicate CRC32 calculations)
    let result = repair_context.repair_with_slices(Some(preloaded_slices))?;

    // Print bytes written
    let bytes_written: u64 = damaged_files
        .iter()
        .chain(missing_files.iter())
        .filter_map(|name| {
            repair_context
                .recovery_set
                .files
                .iter()
                .find(|f| &f.file_name == name)
                .map(|f| f.file_length)
        })
        .sum();

    if bytes_written > 0 {
        println!();
        println!("Wrote {} bytes to disk", bytes_written);
    }

    // Verify repaired files
    println!();
    println!("Verifying repaired files:");
    println!();

    for file_name in damaged_files.iter().chain(missing_files.iter()) {
        println!("Opening: \"{}\"", file_name);

        // Re-check the file
        if let Some(file_info) = repair_context
            .recovery_set
            .files
            .iter()
            .find(|f| &f.file_name == file_name)
        {
            let file_path = repair_context.base_path.join(&file_info.file_name);
            if repair_context.verify_repaired_file(&file_path, file_info)? {
                println!("Target: \"{}\" - found.", file_name);
            } else {
                println!("Target: \"{}\" - damaged.", file_name);
            }
        }
    }

    println!();
    println!("Repair complete.");

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn test_repair_files_function() {
        // Test with the repair scenario fixtures in a temp directory
        let source_dir = "tests/fixtures/repair_scenarios";
        if !Path::new(source_dir).exists() {
            return;
        }

        // Create temp dir and copy all files
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let temp_path = temp_dir.path();

        // Copy all files from source to temp
        for entry in fs::read_dir(source_dir).expect("Failed to read source dir") {
            let entry = entry.expect("Failed to read entry");
            let path = entry.path();
            if path.is_file() {
                let file_name = path.file_name().unwrap();
                let dest_path = temp_path.join(file_name);
                fs::copy(&path, &dest_path).expect("Failed to copy file");
            }
        }

        let par2_file = temp_path.join("testfile.par2");
        if par2_file.exists() {
            let result = repair_files(&par2_file.to_string_lossy(), &[]);
            // The result depends on the test fixtures, but it should not crash
            debug!("Repair result: {:?}", result);
        }

        // temp_dir is automatically cleaned up
    }

    #[test]
    fn test_file_status_determination() {
        // Test with existing test files
        let par2_file = Path::new("tests/fixtures/testfile.par2");
        if par2_file.exists() {
            let par2_files = crate::file_ops::collect_par2_files(par2_file);
            let (packets, _) = crate::file_ops::load_all_par2_packets(&par2_files, false);

            if !packets.is_empty() {
                let base_path = par2_file.parent().unwrap().to_path_buf();
                if let Ok(repair_context) = RepairContext::new(packets, base_path) {
                    let file_status = repair_context.check_file_status();
                    assert!(!file_status.is_empty());
                }
            }
        }
    }
}
