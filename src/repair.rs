//! PAR2 File Repair Module
//!
//! This module provides functionality for repairing files using PAR2 recovery data.
//! It implements Reed-Solomon error correction to reconstruct missing or corrupted files
//! using the Vandermonde polynomial 0x1100B for GF(2^16) operations.
//!
//! ## Performance
//!
//! SIMD-optimized Reed-Solomon operations achieve **1.66x speedup** over par2cmdline:
//! - par2rs: 0.607s average (100MB file repair)
//! - par2cmdline: 1.008s average
//!
//! See `docs/SIMD_OPTIMIZATION.md` for detailed benchmarks and implementation notes.
//!
//! ## Type Safety
//!
//! This module uses newtype wrappers to prevent common mistakes:
//! - **FileId, RecoverySetId, Md5Hash**: Prevents mixing 3 different [u8; 16] identifiers
//! - **Crc32Value**: Prevents mixing CRC checksums with sizes/counts/other u32 values
//! - **GlobalSliceIndex, LocalSliceIndex**: Prevents off-by-one errors in multi-file repair

use crate::file_verification::calculate_file_md5;
use crate::{
    FileDescriptionPacket, InputFileSliceChecksumPacket, MainPacket, Packet, RecoverySlicePacket,
};
use crc32fast::Hasher as Crc32;
use log::{debug, trace};
use rustc_hash::FxHashMap as HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Type-safe wrapper for PAR2 file identifiers (16-byte MD5)
/// Prevents accidentally mixing file IDs with other 16-byte values like hashes or set IDs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId([u8; 16]);

impl FileId {
    pub fn new(bytes: [u8; 16]) -> Self {
        FileId(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl From<[u8; 16]> for FileId {
    fn from(bytes: [u8; 16]) -> Self {
        FileId::new(bytes)
    }
}

impl AsRef<[u8; 16]> for FileId {
    fn as_ref(&self) -> &[u8; 16] {
        &self.0
    }
}

impl PartialEq<[u8; 16]> for FileId {
    fn eq(&self, other: &[u8; 16]) -> bool {
        &self.0 == other
    }
}

impl PartialEq<FileId> for [u8; 16] {
    fn eq(&self, other: &FileId) -> bool {
        self == &other.0
    }
}

/// Type-safe wrapper for global slice indices (across all files in recovery set)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlobalSliceIndex(usize);

impl GlobalSliceIndex {
    pub fn new(index: usize) -> Self {
        GlobalSliceIndex(index)
    }

    pub fn as_usize(&self) -> usize {
        self.0
    }
}

impl From<usize> for GlobalSliceIndex {
    fn from(index: usize) -> Self {
        GlobalSliceIndex::new(index)
    }
}

impl std::ops::Add<usize> for GlobalSliceIndex {
    type Output = GlobalSliceIndex;

    fn add(self, rhs: usize) -> GlobalSliceIndex {
        GlobalSliceIndex(self.0 + rhs)
    }
}

impl std::ops::Sub for GlobalSliceIndex {
    type Output = usize;

    fn sub(self, rhs: GlobalSliceIndex) -> usize {
        self.0 - rhs.0
    }
}

impl std::fmt::Display for GlobalSliceIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type-safe wrapper for local slice indices (within a single file)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalSliceIndex(usize);

impl LocalSliceIndex {
    pub fn new(index: usize) -> Self {
        LocalSliceIndex(index)
    }

    pub fn as_usize(&self) -> usize {
        self.0
    }

    /// Convert to global index by adding file's global offset
    pub fn to_global(&self, offset: GlobalSliceIndex) -> GlobalSliceIndex {
        GlobalSliceIndex(offset.0 + self.0)
    }
}

impl From<usize> for LocalSliceIndex {
    fn from(index: usize) -> Self {
        LocalSliceIndex::new(index)
    }
}

impl std::fmt::Display for LocalSliceIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type-safe wrapper for recovery set identifiers (16-byte hash)
/// Distinct from FileId and Md5Hash to prevent mixing different ID types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecoverySetId([u8; 16]);

impl RecoverySetId {
    pub fn new(bytes: [u8; 16]) -> Self {
        RecoverySetId(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl From<[u8; 16]> for RecoverySetId {
    fn from(bytes: [u8; 16]) -> Self {
        RecoverySetId::new(bytes)
    }
}

impl AsRef<[u8; 16]> for RecoverySetId {
    fn as_ref(&self) -> &[u8; 16] {
        &self.0
    }
}

impl PartialEq<[u8; 16]> for RecoverySetId {
    fn eq(&self, other: &[u8; 16]) -> bool {
        &self.0 == other
    }
}

impl PartialEq<RecoverySetId> for [u8; 16] {
    fn eq(&self, other: &RecoverySetId) -> bool {
        self == &other.0
    }
}

/// Type-safe wrapper for MD5 hash values
/// Distinct from FileId to prevent confusion between different hash purposes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Md5Hash([u8; 16]);

impl Md5Hash {
    pub fn new(bytes: [u8; 16]) -> Self {
        Md5Hash(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    pub fn len(&self) -> usize {
        16
    }
}

impl From<[u8; 16]> for Md5Hash {
    fn from(bytes: [u8; 16]) -> Self {
        Md5Hash::new(bytes)
    }
}

impl AsRef<[u8; 16]> for Md5Hash {
    fn as_ref(&self) -> &[u8; 16] {
        &self.0
    }
}

impl PartialEq<[u8; 16]> for Md5Hash {
    fn eq(&self, other: &[u8; 16]) -> bool {
        &self.0 == other
    }
}

impl PartialEq<Md5Hash> for [u8; 16] {
    fn eq(&self, other: &Md5Hash) -> bool {
        self == &other.0
    }
}

/// Type-safe wrapper for CRC32 checksum values
/// Prevents mixing CRC values with other u32 values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Crc32Value(u32);

impl Crc32Value {
    pub fn new(value: u32) -> Self {
        Crc32Value(value)
    }

    pub fn as_u32(&self) -> u32 {
        self.0
    }

    pub fn to_le_bytes(&self) -> [u8; 4] {
        self.0.to_le_bytes()
    }
}

impl From<u32> for Crc32Value {
    fn from(value: u32) -> Self {
        Crc32Value::new(value)
    }
}

impl PartialEq<u32> for Crc32Value {
    fn eq(&self, other: &u32) -> bool {
        self.0 == *other
    }
}

impl PartialEq<Crc32Value> for u32 {
    fn eq(&self, other: &Crc32Value) -> bool {
        *self == other.0
    }
}

impl std::fmt::Display for Crc32Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

/// Information about a file in the recovery set
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub file_id: FileId,
    pub file_name: String,
    pub file_length: u64,
    pub md5_hash: Md5Hash,
    pub md5_16k: Md5Hash,
    pub slice_count: usize,
    pub global_slice_offset: GlobalSliceIndex, // Starting global slice index for this file
}

impl FileInfo {
    /// Convert a local slice index to global for this file
    pub fn local_to_global(&self, local: LocalSliceIndex) -> GlobalSliceIndex {
        local.to_global(self.global_slice_offset)
    }

    /// Convert a global slice index to local for this file, if it belongs to this file
    pub fn global_to_local(&self, global: GlobalSliceIndex) -> Option<LocalSliceIndex> {
        if global.0 >= self.global_slice_offset.0
            && global.0 < self.global_slice_offset.0 + self.slice_count
        {
            Some(LocalSliceIndex::new(global.0 - self.global_slice_offset.0))
        } else {
            None
        }
    }
}

/// Information about the recovery set
#[derive(Debug)]
pub struct RecoverySetInfo {
    pub set_id: RecoverySetId,
    pub slice_size: u64,
    pub files: Vec<FileInfo>,
    pub recovery_slices: Vec<RecoverySlicePacket>,
    pub file_slice_checksums: HashMap<FileId, InputFileSliceChecksumPacket>,
}

/// Status of a file that needs repair
#[derive(Debug, PartialEq)]
pub enum FileStatus {
    Present,   // File exists and is valid
    Missing,   // File doesn't exist
    Corrupted, // File exists but is corrupted
}

impl FileStatus {
    /// Returns true if the file needs repair (missing or corrupted)
    pub fn needs_repair(&self) -> bool {
        matches!(self, FileStatus::Missing | FileStatus::Corrupted)
    }
}

/// Result of verifying a repaired file
#[derive(Debug, PartialEq)]
pub enum VerificationResult {
    /// File verified successfully - matches expected hash and size
    Verified,
    /// File size doesn't match expected
    SizeMismatch { expected: u64, actual: u64 },
    /// File MD5 hash doesn't match expected
    HashMismatch,
}



/// Result of a repair operation - type-safe to prevent mismatched success/failure states
#[derive(Debug)]
pub enum RepairResult {
    /// All files were repaired and verified successfully
    Success {
        files_repaired: usize,
        files_verified: usize,
        repaired_files: Vec<String>,
        verified_files: Vec<String>,
        message: String,
    },
    /// Repair was not needed - all files already valid
    NoRepairNeeded {
        files_verified: usize,
        verified_files: Vec<String>,
        message: String,
    },
    /// Repair failed - insufficient recovery blocks or verification failed
    Failed {
        files_failed: Vec<String>,
        files_verified: usize,
        verified_files: Vec<String>,
        message: String,
        error: String,
    },
}

impl RepairResult {
    /// Returns true if repair succeeded or wasn't needed
    pub fn is_success(&self) -> bool {
        matches!(self, RepairResult::Success { .. } | RepairResult::NoRepairNeeded { .. })
    }
    
    /// Get the files that were successfully repaired
    pub fn repaired_files(&self) -> &[String] {
        match self {
            RepairResult::Success { repaired_files, .. } => repaired_files,
            _ => &[],
        }
    }
    
    /// Get the files that failed repair
    pub fn failed_files(&self) -> &[String] {
        match self {
            RepairResult::Failed { files_failed, .. } => files_failed,
            _ => &[],
        }
    }
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
        let mut fd_map: HashMap<FileId, FileDescriptionPacket> = HashMap::default();
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
                global_slice_offset: GlobalSliceIndex::new(global_slice_offset),
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
                return FileStatus::Present;
            }
        }
        
        FileStatus::Corrupted
    }

    /// Perform repair operation
    pub fn repair_with_slices(
        &self,
    ) -> Result<RepairResult, Box<dyn std::error::Error>> {
        debug!("repair_with_slices");
        let file_status = self.check_file_status();
        debug!("  File statuses: {:?}", file_status);

        // Check if repair is needed
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

        // Build validation cache by validating all files once upfront
        let mut validation_cache: HashMap<FileId, std::collections::HashSet<usize>> = HashMap::default();
        let mut total_damaged_blocks = 0;
        
        for file_info in &self.recovery_set.files {
            let status = file_status.get(&file_info.file_name).unwrap_or(&FileStatus::Missing);
            match status {
                FileStatus::Missing => {
                    total_damaged_blocks += file_info.slice_count;
                    // Empty set for missing files
                    validation_cache.insert(file_info.file_id, std::collections::HashSet::new());
                }
                FileStatus::Corrupted | FileStatus::Present => {
                    let valid_slices = self.validate_file_slices(file_info)?;
                    let damaged_slices = file_info.slice_count - valid_slices.len();
                    total_damaged_blocks += damaged_slices;
                    validation_cache.insert(file_info.file_id, valid_slices);
                }
            }
        }

        debug!("  total_damaged_blocks: {}, recovery_blocks: {}", 
               total_damaged_blocks, self.recovery_set.recovery_slices.len());

        if total_damaged_blocks > self.recovery_set.recovery_slices.len() {
            return Ok(RepairResult::Failed {
                files_failed: file_status.keys().cloned().collect(),
                files_verified: 0,
                verified_files: Vec::new(),
                message: format!("Insufficient recovery data: need {} blocks but only have {}", 
                                total_damaged_blocks, self.recovery_set.recovery_slices.len()),
                error: "Not enough recovery blocks available".to_string(),
            });
        }

        // Perform the actual repair with validation cache
        self.perform_reed_solomon_repair(&file_status, &validation_cache)
    }

    /// Perform repair operation
    pub fn repair(&self) -> Result<RepairResult, Box<dyn std::error::Error>> {
        let _file_status = self.check_file_status();
        self.repair_with_slices()
    }

    /// Perform Reed-Solomon repair
    fn perform_reed_solomon_repair(
        &self,
        file_status: &HashMap<String, FileStatus>,
        validation_cache: &HashMap<FileId, std::collections::HashSet<usize>>,
    ) -> Result<RepairResult, Box<dyn std::error::Error>> {
        debug!("perform_reed_solomon_repair: processing {} files", self.recovery_set.files.len());
        let mut repaired_files = Vec::new();
        let mut verified_files = Vec::new();
        let mut files_failed = Vec::new();

        // Process each file that needs repair
        for file_info in &self.recovery_set.files {
            debug!("  Checking file: {}", file_info.file_name);
            let status = file_status
                .get(&file_info.file_name)
                .unwrap_or(&FileStatus::Missing);

            if *status == FileStatus::Present {
                verified_files.push(file_info.file_name.clone());
                continue; // File is already good
            }

            // Attempt to repair the file using Reed-Solomon reconstruction
            match self.repair_single_file(file_info, status, validation_cache) {
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
                message: message.clone(),
                error: message,
            });
        }

        if files_repaired_count > 0 {
            Ok(RepairResult::Success {
                files_repaired: files_repaired_count,
                files_verified: files_verified_count,
                repaired_files,
                verified_files,
                message: format!("Successfully repaired {} file(s)", files_repaired_count),
            })
        } else {
            Ok(RepairResult::NoRepairNeeded {
                files_verified: files_verified_count,
                verified_files,
                message: format!("All {} file(s) verified as intact", files_verified_count),
            })
        }
    }

    /// Repair a single file using Reed-Solomon reconstruction
    fn repair_single_file(
        &self,
        file_info: &FileInfo,
        status: &FileStatus,
        validation_cache: &HashMap<FileId, std::collections::HashSet<usize>>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let file_path = self.base_path.join(&file_info.file_name);
        
        debug!("repair_single_file: {} (status: {:?})", file_info.file_name, status);

        let valid_slice_indices = validation_cache.get(&file_info.file_id)
            .ok_or_else(|| format!("No validation cache for file {}", file_info.file_name))?;
        debug!("  Have {} valid slices out of {} total (from cache)", 
               valid_slice_indices.len(), file_info.slice_count);
        
        // Identify missing slices
        let mut missing_slices = Vec::new();
        for slice_index in 0..file_info.slice_count {
            if !valid_slice_indices.contains(&slice_index) {
                missing_slices.push(slice_index);
            }
        }
        debug!("  Missing slices: {} out of {} total", missing_slices.len(), file_info.slice_count);

        if missing_slices.is_empty() {
            // All slices validated successfully
            if *status == FileStatus::Corrupted {
                debug!("All slices valid but file MD5 doesn't match - may have been externally modified");
                // Could try to rebuild from validated slices, but that requires loading them all
                // For now, treat as unrepairable
                return Err("File MD5 mismatch despite all slices being valid".into());
            }
            return Ok(false); // File is valid
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
        let missing_local: Vec<LocalSliceIndex> = missing_slices.iter()
            .map(|&idx| LocalSliceIndex::new(idx))
            .collect();
        let reconstructed_slices = self.reconstruct_slices(
            &missing_local,
            file_info,
            validation_cache
        )?;

        debug!("Reconstructed {} slices", reconstructed_slices.len());

        // Write repaired file
        self.write_repaired_file(&file_path, file_info, &valid_slice_indices, &reconstructed_slices)?;

        // Verify the repaired file
        match self.verify_repaired_file(&file_path, file_info)? {
            VerificationResult::Verified => Ok(true),
            result => Err(format!("Repaired file failed verification: {:?}", result).into()),
        }
    }

    /// Validate slices from an existing file
    /// Returns only the indices of valid slices, not the slice data itself
    pub fn validate_file_slices(
        &self,
        file_info: &FileInfo,
    ) -> Result<std::collections::HashSet<usize>, Box<dyn std::error::Error>> {
        let file_path = self.base_path.join(&file_info.file_name);
        let mut valid_slices = std::collections::HashSet::new();

        if !file_path.exists() {
            return Ok(valid_slices); // No valid slices for missing file
        }

        let file = File::open(&file_path)?;
        let mut reader = BufReader::with_capacity(1024 * 1024, file); // 1MB buffer
        let slice_size = self.recovery_set.slice_size as usize;

        // Reuse single buffer for all slices
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

            // Zero the buffer for this iteration (PAR2 spec: padding must be zeros)
            slice_data.fill(0);
            
            // Sequential read (no seeking needed with BufReader)
            if reader.read_exact(&mut slice_data[..actual_slice_size]).is_ok() {
                // Verify slice checksum if available
                if let Some(checksums) = self
                    .recovery_set
                    .file_slice_checksums
                    .get(&file_info.file_id)
                {
                    if slice_index < checksums.slice_checksums.len() {
                        // PAR2 CRC32 is computed on full slice with zero padding
                        let mut hasher = Crc32::new();
                        hasher.update(&slice_data[..slice_size]);
                        let slice_crc = hasher.finalize();
                        let expected_crc = checksums.slice_checksums[slice_index].1;

                        if slice_crc == expected_crc {
                            valid_slices.insert(slice_index);
                        } else {
                            trace!("Slice {} failed CRC32 verification", slice_index);
                        }
                    } else {
                        // No checksum available, assume valid
                        valid_slices.insert(slice_index);
                    }
                } else {
                    // No checksums available, assume valid
                    valid_slices.insert(slice_index);
                }
            } else {
                trace!("Slice {} failed to read {} bytes", slice_index, actual_slice_size);
            }
        }

        debug!(
            "Validated {} valid slices out of {} total slices",
            valid_slices.len(),
            file_info.slice_count
        );
        Ok(valid_slices)
    }




    /// Reconstruct missing slices using Reed-Solomon
    fn reconstruct_slices(
        &self,
        missing_slices: &[LocalSliceIndex],
        file_info: &FileInfo,
        validation_cache: &HashMap<FileId, std::collections::HashSet<usize>>,
    ) -> Result<HashMap<usize, Vec<u8>>, Box<dyn std::error::Error>> {
        use crate::slice_provider::{ChunkedSliceProvider, RecoverySliceProvider, SliceLocation};
        use std::io::Cursor;
        
        debug!("Reconstructing {} missing slices", missing_slices.len());
        
        // Build slice provider with all available slices
        let mut input_provider = ChunkedSliceProvider::new(self.recovery_set.slice_size as usize);
        
        for other_file_info in &self.recovery_set.files {
            let file_path = self.base_path.join(&other_file_info.file_name);
            if !file_path.exists() {
                continue;
            }
            
            let valid_slices = validation_cache.get(&other_file_info.file_id)
                .ok_or_else(|| format!("No validation cache for file {}", other_file_info.file_name))?;
            debug!("  File {} - using {} cached valid slices out of {}", 
                   other_file_info.file_name, valid_slices.len(), other_file_info.slice_count);
            
            // Add slices from this file
            for slice_index in 0..other_file_info.slice_count {
                // Skip slices that are not valid
                if !valid_slices.contains(&slice_index) {
                    continue;
                }
                
                let global_index = other_file_info.local_to_global(LocalSliceIndex::new(slice_index));
                let offset = (slice_index * self.recovery_set.slice_size as usize) as u64;
                let actual_size = if slice_index == other_file_info.slice_count - 1 {
                    let remaining = other_file_info.file_length % self.recovery_set.slice_size;
                    if remaining == 0 {
                        self.recovery_set.slice_size as usize
                    } else {
                        remaining as usize
                    }
                } else {
                    self.recovery_set.slice_size as usize
                };
                
                let expected_crc = self.recovery_set
                    .file_slice_checksums
                    .get(&other_file_info.file_id)
                    .and_then(|checksums| checksums.slice_checksums.get(slice_index))
                    .map(|(_, crc)| *crc);
                
                input_provider.add_slice(global_index.as_usize(), SliceLocation {
                    file_path: file_path.clone(),
                    offset,
                    size: actual_size,
                    expected_crc,
                });
            }
        }
        
        // Build recovery slice provider
        let mut recovery_provider = RecoverySliceProvider::new(self.recovery_set.slice_size as usize);
        for recovery_slice in &self.recovery_set.recovery_slices {
            recovery_provider.add_recovery_slice(
                recovery_slice.exponent as usize,
                recovery_slice.recovery_data.clone()
            );
        }
        
        // Convert file-local indices to global
        let global_missing_indices: Vec<usize> = missing_slices
            .iter()
            .map(|&idx| file_info.local_to_global(idx).as_usize())
            .collect();
        
        // Create reconstruction engine
        let total_input_slices: usize = self.recovery_set.files.iter().map(|f| f.slice_count).sum();
        let reconstruction_engine = crate::reed_solomon::ReconstructionEngine::new(
            self.recovery_set.slice_size as usize,
            total_input_slices,
            self.recovery_set.recovery_slices.clone(),
        );
        
        // Create output writers (in-memory buffers for now)
        let mut output_buffers: HashMap<usize, Cursor<Vec<u8>>> = HashMap::default();
        for &global_idx in &global_missing_indices {
            output_buffers.insert(global_idx, Cursor::new(Vec::new()));
        }
        
        // Perform reconstruction
        let result = reconstruction_engine.reconstruct_missing_slices_chunked(
            &mut input_provider,
            &recovery_provider,
            &global_missing_indices,
            &mut output_buffers,
            64 * 1024, // 64KB chunks
        );
        
        if !result.success {
            return Err(result.error_message.unwrap_or_else(|| "Reconstruction failed".to_string()).into());
        }
        
        // Convert global indices back to file-local and extract buffers
        let mut reconstructed = HashMap::default();
        for (global_idx, cursor) in output_buffers {
            let global_index = GlobalSliceIndex::new(global_idx);
            if let Some(file_local_idx) = file_info.global_to_local(global_index) {
                reconstructed.insert(file_local_idx.as_usize(), cursor.into_inner());
            }
        }
        
        Ok(reconstructed)
    }

    
    /// Write repaired file by streaming slices from disk and reconstructed data
    fn write_repaired_file(
        &self,
        file_path: &Path,
        file_info: &FileInfo,
        valid_slice_indices: &std::collections::HashSet<usize>,
        reconstructed_slices: &HashMap<usize, Vec<u8>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Writing repaired file with streaming I/O: {:?}", file_path);
        
        // Write to temp file first, then rename to avoid corrupting source while reading
        let temp_path = file_path.with_extension("par2_tmp");
        
        // Open source file for reading valid slices
        let source_path = self.base_path.join(&file_info.file_name);
        let mut source_file = if source_path.exists() {
            Some(File::open(&source_path)?)
        } else {
            None
        };
        
        // Create temp output file
        let file = File::create(&temp_path)?;
        let mut writer = std::io::BufWriter::with_capacity(1024 * 1024, file);
        
        let slice_size = self.recovery_set.slice_size as usize;
        let mut slice_buffer = vec![0u8; slice_size];
        let mut bytes_written = 0u64;
        
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
            if let Some(reconstructed_data) = reconstructed_slices.get(&slice_index) {
                // Write reconstructed slice
                writer.write_all(&reconstructed_data[..actual_size])?;
                bytes_written += actual_size as u64;
            } else if valid_slice_indices.contains(&slice_index) {
                // Read from source file at specific offset (need to seek for each slice)
                if let Some(ref mut file) = source_file {
                    let offset = (slice_index * slice_size) as u64;
                    file.seek(SeekFrom::Start(offset))?;
                    file.read_exact(&mut slice_buffer[..actual_size])?;
                    writer.write_all(&slice_buffer[..actual_size])?;
                    bytes_written += actual_size as u64;
                } else {
                    return Err(format!("Slice {} marked valid but source file not available", slice_index).into());
                }
            } else {
                return Err(format!("Slice {} neither reconstructed nor valid", slice_index).into());
            }
        }
        
        writer.flush()?;
        drop(writer); // Close the file before rename
        drop(source_file); // Close source file before rename
        
        if bytes_written != file_info.file_length {
            return Err(format!(
                "Wrote {} bytes but expected {}",
                bytes_written, file_info.file_length
            ).into());
        }
        
        // Rename temp file to final destination
        fs::rename(&temp_path, file_path)?;
        
        debug!("Wrote {} bytes to {:?}", bytes_written, file_path);
        Ok(())
    }








    /// Verify that the repaired file is correct
    fn verify_repaired_file(
        &self,
        file_path: &Path,
        file_info: &FileInfo,
    ) -> Result<VerificationResult, Box<dyn std::error::Error>> {
        // Check file size
        let metadata = fs::metadata(file_path)?;
        if metadata.len() != file_info.file_length {
            debug!(
                "File size mismatch: expected {}, got {}",
                file_info.file_length,
                metadata.len()
            );
            return Ok(VerificationResult::SizeMismatch {
                expected: file_info.file_length,
                actual: metadata.len(),
            });
        }

        // After repair, just check the full MD5 directly
        // (No point checking 16k first - we need to read the whole file anyway)
        let file_md5 = calculate_file_md5(file_path)?;
        if file_md5 == file_info.md5_hash {
            Ok(VerificationResult::Verified)
        } else {
            debug!("MD5 mismatch:");
            debug!("  Expected: {:02x?}", file_info.md5_hash);
            debug!("  Actual:   {:02x?}", file_md5);
            Ok(VerificationResult::HashMismatch)
        }
    }
}

/// High-level repair function that can be called from the binary
/// Output format matches par2cmdline
pub fn repair_files(
    par2_file: &str,
    _target_files: &[String],
) -> Result<RepairResult, Box<dyn std::error::Error>> {
    repair_files_with_output(par2_file, _target_files, true)
}

/// Repair files with configurable output
pub fn repair_files_with_output(
    par2_file: &str,
    _target_files: &[String],
    verbose: bool,
) -> Result<RepairResult, Box<dyn std::error::Error>> {
    let par2_path = Path::new(par2_file);

    // Load PAR2 files and packets
    let par2_files = crate::file_ops::collect_par2_files(par2_path);
    let (packets, _recovery_blocks) = crate::file_ops::load_all_par2_packets(&par2_files, verbose);

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

    if verbose {
        // Print statistics (matching par2cmdline format)
        println!();
        let total_blocks: usize = repair_context
            .recovery_set
            .files
            .iter()
            .map(|f| f.slice_count)
            .sum();
        
        println!(
            "There are {} recoverable files and {} other files.",
            repair_context.recovery_set.files.len(), 0
        );
        println!("The block size used was {} bytes.", repair_context.recovery_set.slice_size);
        println!("There are a total of {} data blocks.", total_blocks);
        
        let total_size: u64 = repair_context
            .recovery_set
            .files
            .iter()
            .map(|f| f.file_length)
            .sum();
        println!("The total size of the data files is {} bytes.", total_size);
        println!();
        println!("Verifying source files:");
        println!();
    }

    let result = repair_context.repair()?;
    
    // Print results in par2cmdline format
    if verbose {
        match &result {
            RepairResult::NoRepairNeeded { files_verified, .. } => {
                println!();
                println!("All files are correct, repair is not required.");
                println!("Verified {} file(s).", files_verified);
            }
            RepairResult::Success { files_repaired, files_verified, .. } => {
                println!();
                println!("Repair complete.");
                println!("Repaired {} file(s).", files_repaired);
                println!("Verified {} file(s).", files_verified);
            }
            RepairResult::Failed { files_failed, message, .. } => {
                println!();
                println!("Repair FAILED: {}", message);
                println!("Failed to repair {} file(s).", files_failed.len());
            }
        }
    }

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
