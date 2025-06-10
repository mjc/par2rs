//! PAR2 File Repair Module
//!
//! This module provides functionality for repairing files using PAR2 recovery data.
//! It implements Reed-Solomon error correction to reconstruct missing or corrupted files.

use crate::file_verification::calculate_file_md5;
use crate::reed_solomon::{ReconstructionEngine};
use crate::{FileDescriptionPacket, InputFileSliceChecksumPacket, MainPacket, Packet, RecoverySlicePacket};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
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

        // Build file information from descriptions
        let mut files = Vec::new();
        for fd in file_descriptions {
            let file_name = String::from_utf8_lossy(&fd.file_name)
                .trim_end_matches('\0')
                .to_string();

            let slice_count = fd.file_length.div_ceil(main.slice_size) as usize;

            files.push(FileInfo {
                file_id: fd.file_id,
                file_name,
                file_length: fd.file_length,
                md5_hash: fd.md5_hash,
                md5_16k: fd.md5_16k,
                slice_count,
            });
        }

        // Build checksum map indexed by file_id
        let mut file_slice_checksums = HashMap::new();
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
        let mut status_map = HashMap::new();

        for file_info in &self.recovery_set.files {
            let file_path = self.base_path.join(&file_info.file_name);
            let status = self.determine_file_status(&file_path, file_info);
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

        // Check MD5 hash
        if let Ok(file_md5) = calculate_file_md5(file_path) {
            if file_md5 == file_info.md5_hash {
                FileStatus::Present
            } else {
                FileStatus::Corrupted
            }
        } else {
            FileStatus::Corrupted
        }
    }

    /// Determine if repair is possible for the given file statuses
    pub fn can_repair(&self, file_status: &HashMap<String, FileStatus>) -> bool {
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

        // For corrupted files, we need to check each slice individually
        let mut corrupted_slices = 0;
        for file_info in &self.recovery_set.files {
            let status = file_status
                .get(&file_info.file_name)
                .unwrap_or(&FileStatus::Missing);
            
            if *status == FileStatus::Corrupted {
                // Count actually corrupted slices in this file
                let file_path = self.base_path.join(&file_info.file_name);
                corrupted_slices += self.count_corrupted_slices(&file_path, file_info);
            }
        }

        let total_needed_slices = missing_slices + corrupted_slices;
        let available_recovery_slices = self.recovery_set.recovery_slices.len();

        println!("Debug: Missing slices: {}, Corrupted slices: {}, Total needed: {}, Available recovery slices: {}", 
                missing_slices, corrupted_slices, total_needed_slices, available_recovery_slices);

        // Can repair if we have enough recovery slices to replace needed slices
        available_recovery_slices >= total_needed_slices
    }

    /// Count the number of corrupted slices in a file
    fn count_corrupted_slices(&self, file_path: &Path, file_info: &FileInfo) -> usize {
        // Get the input file slice checksum packet for this file
        let slice_checksums = match self.recovery_set.file_slice_checksums.get(&file_info.file_id) {
            Some(checksums) => checksums,
            None => {
                println!("Warning: No slice checksums found for file {}", file_info.file_name);
                return file_info.slice_count; // Assume all slices are corrupted if no checksums
            }
        };

        let mut corrupted_count = 0;
        let slice_size = self.recovery_set.slice_size;

        // Open the file and check each slice
        if let Ok(mut file) = File::open(file_path) {
            for slice_index in 0..file_info.slice_count.min(slice_checksums.slice_checksums.len()) {
                let slice_offset = slice_index as u64 * slice_size;
                let slice_end = ((slice_index + 1) as u64 * slice_size).min(file_info.file_length);
                let slice_length = slice_end - slice_offset;

                // Read the slice data
                if file.seek(SeekFrom::Start(slice_offset)).is_err() {
                    corrupted_count += 1;
                    continue;
                }

                let mut slice_data = vec![0u8; slice_length as usize];
                if file.read_exact(&mut slice_data).is_err() {
                    corrupted_count += 1;
                    continue;
                }

                // Calculate MD5 hash of the slice
                let slice_md5 = md5::compute(&slice_data).0;
                let expected_md5 = slice_checksums.slice_checksums[slice_index].0;

                if slice_md5 != expected_md5 {
                    corrupted_count += 1;
                    if slice_index < 10 { // Only log first few for debugging
                        println!("Slice {} corrupted in {}", slice_index, file_info.file_name);
                    }
                }
            }
        } else {
            // Cannot read file, assume all slices are corrupted
            corrupted_count = file_info.slice_count;
        }

        println!("Found {} corrupted slices in {}", corrupted_count, file_info.file_name);
        corrupted_count
    }

    /// Perform repair operation
    pub fn repair(&self) -> Result<RepairResult, Box<dyn std::error::Error>> {
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
        if !self.can_repair(&file_status) {
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
        self.perform_reed_solomon_repair(&file_status)
    }

    /// Perform Reed-Solomon repair using available recovery data
    fn perform_reed_solomon_repair(
        &self,
        file_status: &HashMap<String, FileStatus>,
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

            // Attempt to repair the file using Reed-Solomon reconstruction
            match self.repair_single_file(file_info, status) {
                Ok(repaired) => {
                    if repaired {
                        repaired_files.push(file_info.file_name.clone());
                        println!("Successfully repaired: {}", file_info.file_name);
                    } else {
                        verified_files.push(file_info.file_name.clone());
                        println!("File was already valid: {}", file_info.file_name);
                    }
                }
                Err(e) => {
                    files_failed.push(file_info.file_name.clone());
                    eprintln!("Failed to repair {}: {}", file_info.file_name, e);
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
    fn repair_single_file(
        &self,
        file_info: &FileInfo,
        status: &FileStatus,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let file_path = self.base_path.join(&file_info.file_name);
        
        // Load slices from the existing file (if it exists and has valid slices)
        let mut file_slices = self.load_file_slices(file_info)?;
        
        // Check which slices are missing or corrupted
        let missing_slices = self.identify_missing_slices(&file_slices, file_info)?;
        
        if missing_slices.is_empty() {
            // All slices are present and valid, but check if the overall file is still corrupted
            if *status == FileStatus::Corrupted {
                println!("All slices are valid but file MD5 doesn't match - attempting repair");
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
        
        println!("File {} has {} missing/corrupted slices out of {} total", 
                file_info.file_name, missing_slices.len(), file_info.slice_count);
        
        // Check if we have enough recovery data
        if missing_slices.len() > self.recovery_set.recovery_slices.len() {
            return Err(format!(
                "Cannot repair: {} missing slices but only {} recovery slices available",
                missing_slices.len(),
                self.recovery_set.recovery_slices.len()
            ).into());
        }
        
        // Reconstruct missing slices using Reed-Solomon
        let reconstructed_slices = self.reconstruct_slices(&file_slices, &missing_slices, file_info)?;
        
        // Update file_slices with reconstructed data
        for (slice_index, slice_data) in reconstructed_slices {
            file_slices.insert(slice_index, slice_data);
        }
        
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
    fn load_file_slices(&self, file_info: &FileInfo) -> Result<HashMap<usize, Vec<u8>>, Box<dyn std::error::Error>> {
        let file_path = self.base_path.join(&file_info.file_name);
        let mut slices = HashMap::new();
        
        if !file_path.exists() {
            return Ok(slices); // Return empty map for missing files
        }
        
        let mut file = File::open(&file_path)?;
        let slice_size = self.recovery_set.slice_size as usize;
        
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
            
            let mut slice_data = vec![0u8; actual_slice_size];
            file.seek(SeekFrom::Start((slice_index * slice_size) as u64))?;
            
            let bytes_read = file.read(&mut slice_data)?;
            if bytes_read == actual_slice_size {
                // Verify slice checksum if available
                if let Some(checksums) = self.recovery_set.file_slice_checksums.get(&file_info.file_id) {
                    if slice_index < checksums.slice_checksums.len() {
                        let expected_md5 = checksums.slice_checksums[slice_index].0;
                        let actual_md5 = md5::compute(&slice_data);
                        
                        if actual_md5.as_ref() == expected_md5 {
                            slices.insert(slice_index, slice_data);
                        } else {
                            println!("Slice {} failed checksum verification", slice_index);
                        }
                    } else {
                        // No checksum available for this slice, assume it's good if size matches
                        slices.insert(slice_index, slice_data);
                    }
                } else {
                    // No checksums available, assume slice is valid if size matches
                    slices.insert(slice_index, slice_data);
                }
            } else {
                println!("Slice {} has incorrect size: expected {}, got {}", 
                        slice_index, actual_slice_size, bytes_read);
            }
        }
        
        println!("Loaded {} valid slices out of {} total slices", slices.len(), file_info.slice_count);
        Ok(slices)
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
    
    /// Reconstruct missing slices using the Reed-Solomon module
    fn reconstruct_slices(
        &self,
        existing_slices: &HashMap<usize, Vec<u8>>,
        missing_slices: &[usize],
        file_info: &FileInfo,
    ) -> Result<HashMap<usize, Vec<u8>>, Box<dyn std::error::Error>> {
        let slice_size = self.recovery_set.slice_size as usize;
        let recovery_slices_count = self.recovery_set.recovery_slices.len();
        
        if missing_slices.len() > recovery_slices_count {
            return Err(format!(
                "Cannot repair: {} missing slices but only {} recovery slices available",
                missing_slices.len(),
                recovery_slices_count
            ).into());
        }

        println!("Reconstructing {} missing slices using {} recovery slices", 
                missing_slices.len(), recovery_slices_count);

        // For PAR2 Reed-Solomon, we only need input constants for this specific file
        // Not for all files in the recovery set (which was causing the stack overflow)
        let total_input_slices = file_info.slice_count;
        
        // Map file slices to global slice indices (for this file, they're the same as local indices)
        let mut global_slice_map = HashMap::new();
        for slice_index in 0..file_info.slice_count {
            global_slice_map.insert(slice_index, slice_index);
        }
        
        println!("Single-file Reed-Solomon reconstruction for {} with {} slices", 
                file_info.file_name, total_input_slices);

        // Create reconstruction engine with limited scope
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
            ).into());
        }

        // Perform reconstruction
        let result = reconstruction_engine.reconstruct_missing_slices(
            existing_slices,
            missing_slices,
            &global_slice_map,
        );

        if result.success {
            // Handle any warnings
            if let Some(warning) = &result.error_message {
                println!("Warning: {}", warning);
            }

            // Adjust slice sizes for the last slice if needed
            let mut final_reconstructed = HashMap::new();
            for (slice_index, mut slice_data) in result.reconstructed_slices {
                let actual_size = if slice_index == file_info.slice_count - 1 {
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
                final_reconstructed.insert(slice_index, slice_data);
                println!("Reconstructed slice {} with {} bytes", slice_index, actual_size);
            }

            Ok(final_reconstructed)
        } else {
            Err(result.error_message.unwrap_or_else(|| "Reed-Solomon reconstruction failed".to_string()).into())
        }
    }
    
    /// Write the repaired file to disk
    fn write_repaired_file(
        &self,
        file_path: &Path,
        slices: &HashMap<usize, Vec<u8>>,
        file_info: &FileInfo,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut file = File::create(file_path)?;
        
        // Write slices in order
        for slice_index in 0..file_info.slice_count {
            if let Some(slice_data) = slices.get(&slice_index) {
                file.write_all(slice_data)?;
            } else {
                return Err(format!("Missing slice {} when writing file", slice_index).into());
            }
        }
        
        file.flush()?;
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
            println!("File size mismatch: expected {}, got {}", file_info.file_length, metadata.len());
            return Ok(false);
        }
        
        // Check MD5 hash
        let file_md5 = calculate_file_md5(file_path)?;
        let matches = file_md5 == file_info.md5_hash;
        if !matches {
            println!("MD5 mismatch:");
            println!("  Expected: {:02x?}", file_info.md5_hash);
            println!("  Actual:   {:02x?}", file_md5);
        }
        Ok(matches)
    }

}

/// High-level repair function that can be called from the binary
pub fn repair_files(
    par2_file: &str,
    target_files: &[String],
    verbose: bool,
) -> Result<RepairResult, Box<dyn std::error::Error>> {
    let par2_path = Path::new(par2_file);

    if verbose {
        println!("Starting repair process for: {}", par2_path.display());
        if !target_files.is_empty() {
            println!("Target files specified: {:?}", target_files);
        }
    }

    // Load PAR2 files and packets
    let par2_files = crate::file_ops::collect_par2_files(par2_path);
    let (packets, recovery_blocks) = crate::file_ops::load_all_par2_packets(&par2_files, true);

    if packets.is_empty() {
        return Ok(RepairResult {
            success: false,
            files_repaired: 0,
            files_verified: 0,
            repaired_files: Vec::new(),
            verified_files: Vec::new(),
            files_failed: Vec::new(),
            message: "No valid PAR2 packets found".to_string(),
        });
    }

    if verbose {
        println!(
            "Loaded {} recovery blocks from {} PAR2 files",
            recovery_blocks,
            par2_files.len()
        );
    }

    // Get the base directory for file resolution
    let base_path = par2_path.parent().unwrap_or(Path::new(".")).to_path_buf();

    // Create repair context
    let repair_context = match RepairContext::new(packets, base_path) {
        Ok(ctx) => ctx,
        Err(e) => {
            return Ok(RepairResult {
                success: false,
                files_repaired: 0,
                files_verified: 0,
                repaired_files: Vec::new(),
                verified_files: Vec::new(),
                files_failed: Vec::new(),
                message: format!("Failed to create repair context: {}", e),
            });
        }
    };

    // Show recovery set information
    if verbose {
        println!(
            "Recovery set contains {} file(s):",
            repair_context.recovery_set.files.len()
        );
        for file_info in &repair_context.recovery_set.files {
            println!(
                "  - {} ({} bytes, {} slices)",
                file_info.file_name, file_info.file_length, file_info.slice_count
            );
        }
    }

    // Check file status
    let file_status = repair_context.check_file_status();
    if verbose {
        println!("\nFile status:");
        for (filename, status) in &file_status {
            let status_str = match status {
                FileStatus::Present => "OK",
                FileStatus::Missing => "MISSING",
                FileStatus::Corrupted => "CORRUPTED",
            };
            println!("  - {}: {}", filename, status_str);
        }
    }

    // If specific target files were provided, filter to only those
    let _files_to_process: Vec<&String> = if target_files.is_empty() {
        // Process all files from the recovery set
        repair_context
            .recovery_set
            .files
            .iter()
            .map(|f| &f.file_name)
            .collect()
    } else {
        // Only process specified target files
        target_files.iter().collect()
    };

    // Perform repair
    let mut result = repair_context.repair()?;

    // Filter results if specific files were requested
    if !target_files.is_empty() {
        result.repaired_files.retain(|f| target_files.contains(f));
        result.verified_files.retain(|f| target_files.contains(f));
        result.files_failed.retain(|f| target_files.contains(f));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_repair_files_function() {
        // Test with the repair scenario fixtures
        let par2_file = "tests/fixtures/repair_scenarios/testfile.par2";
        if Path::new(par2_file).exists() {
            let result = repair_files(par2_file, &[], false);
            // The result depends on the test fixtures, but it should not crash
            println!("Repair result: {:?}", result);
        }
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
