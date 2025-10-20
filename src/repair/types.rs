//! Data types for PAR2 repair operations

use crate::domain::{FileId, GlobalSliceIndex, LocalSliceIndex, Md5Hash, RecoverySetId};
use crate::{InputFileSliceChecksumPacket, RecoverySliceMetadata};
use rustc_hash::FxHashMap as HashMap;

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
        let global_usize = global.as_usize();
        let offset_usize = self.global_slice_offset.as_usize();
        if global_usize >= offset_usize && global_usize < offset_usize + self.slice_count {
            Some(LocalSliceIndex::new(global_usize - offset_usize))
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
    /// Memory-efficient metadata for recovery slices (lazy loading)
    pub recovery_slices_metadata: Vec<RecoverySliceMetadata>,
    pub file_slice_checksums: HashMap<FileId, InputFileSliceChecksumPacket>,
}

impl RecoverySetInfo {
    /// Calculate the total number of data blocks across all files
    pub fn total_blocks(&self) -> usize {
        self.files.iter().map(|f| f.slice_count).sum()
    }

    /// Calculate the total size of all data files
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.file_length).sum()
    }

    /// Print statistics in par2cmdline format
    pub fn print_statistics(&self) {
        println!();
        println!(
            "There are {} recoverable files and {} other files.",
            self.files.len(),
            0
        );
        println!("The block size used was {} bytes.", self.slice_size);
        println!("There are a total of {} data blocks.", self.total_blocks());
        println!(
            "The total size of the data files is {} bytes.",
            self.total_size()
        );
        println!();
        println!("Verifying source files:");
        println!();
    }
}

/// Status of a file that needs repair
#[derive(Debug, Clone, Copy, PartialEq)]
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
    },
}

impl RepairResult {
    /// Returns true if repair succeeded or wasn't needed
    pub fn is_success(&self) -> bool {
        matches!(
            self,
            RepairResult::Success { .. } | RepairResult::NoRepairNeeded { .. }
        )
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

    /// Print result in par2cmdline format
    pub fn print_result(&self) {
        println!();
        match self {
            RepairResult::NoRepairNeeded { files_verified, .. } => {
                println!("All files are correct, repair is not required.");
                println!("Verified {} file(s).", files_verified);
            }
            RepairResult::Success {
                files_repaired,
                files_verified,
                ..
            } => {
                println!("Repair complete.");
                println!("Repaired {} file(s).", files_repaired);
                println!("Verified {} file(s).", files_verified);
            }
            RepairResult::Failed {
                files_failed,
                message,
                ..
            } => {
                println!("Repair FAILED: {}", message);
                println!("Failed to repair {} file(s).", files_failed.len());
            }
        }
    }
}

/// Type-safe validation cache for tracking valid slices per file
#[derive(Debug, Clone)]
pub struct ValidationCache {
    valid_slices: HashMap<FileId, rustc_hash::FxHashSet<usize>>,
}

impl ValidationCache {
    /// Create a new empty validation cache
    pub fn new() -> Self {
        Self {
            valid_slices: HashMap::default(),
        }
    }

    /// Check if a slice is valid
    pub fn is_valid(&self, file_id: &FileId, slice_index: usize) -> bool {
        self.valid_slices
            .get(file_id)
            .map(|slices| slices.contains(&slice_index))
            .unwrap_or(false)
    }

    /// Get the count of valid slices for a file
    pub fn valid_count(&self, file_id: &FileId) -> usize {
        self.valid_slices
            .get(file_id)
            .map(|slices| slices.len())
            .unwrap_or(0)
    }

    /// Insert valid slices for a file
    pub fn insert(&mut self, file_id: FileId, valid_slices: rustc_hash::FxHashSet<usize>) {
        self.valid_slices.insert(file_id, valid_slices);
    }

    /// Get the valid slices set for a file
    pub fn get(&self, file_id: &FileId) -> Option<&rustc_hash::FxHashSet<usize>> {
        self.valid_slices.get(file_id)
    }
}

impl Default for ValidationCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Type-safe wrapper for reconstructed slice data
///
/// Maps file-local slice indices to their reconstructed data.
/// This makes it explicit that indices are LocalSliceIndex values
/// and encapsulates the reconstruction result.
#[derive(Debug, Default)]
pub struct ReconstructedSlices {
    slices: rustc_hash::FxHashMap<usize, Vec<u8>>,
}

impl ReconstructedSlices {
    /// Create a new empty reconstruction result
    pub fn new() -> Self {
        Self {
            slices: rustc_hash::FxHashMap::default(),
        }
    }

    /// Insert a reconstructed slice
    pub fn insert(&mut self, index: usize, data: Vec<u8>) {
        self.slices.insert(index, data);
    }

    /// Get reconstructed slice data
    pub fn get(&self, index: usize) -> Option<&[u8]> {
        self.slices.get(&index).map(|v| v.as_slice())
    }

    /// Number of reconstructed slices
    pub fn len(&self) -> usize {
        self.slices.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.slices.is_empty()
    }

    /// Iterate over (index, data) pairs
    pub fn iter(&self) -> impl Iterator<Item = (usize, &[u8])> + '_ {
        self.slices.iter().map(|(k, v)| (*k, v.as_slice()))
    }
}
