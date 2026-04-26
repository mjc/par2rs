//! Repair context management

use super::error::{RepairError, Result};
use super::error_helpers::delete_file;
use super::progress::{ConsoleReporter, ProgressReporter};
use super::types::{FileInfo, RecoverySetInfo};
use crate::domain::{BlockCount, BlockSize, FileId, FileSize, GlobalSliceIndex};
use crate::packets::{FileDescriptionPacket, Packet, RecoverySliceMetadata};
use log::{debug, warn};
use rustc_hash::FxHashMap as HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Main repair context containing all necessary information for repair operations
pub struct RepairContext {
    pub recovery_set: RecoverySetInfo,
    pub base_path: PathBuf,
    pub memory_limit: Option<usize>,
    reporter: Box<dyn ProgressReporter>,
    repair_created_backups: Mutex<Vec<PathBuf>>,
}

impl RepairContext {
    /// Create a new repair context from PAR2 packets with default console reporter
    pub fn new(packets: Vec<Packet>, base_path: PathBuf) -> Result<Self> {
        Self::new_with_reporter(packets, base_path, Box::new(ConsoleReporter::new(false)))
    }

    /// Create a new repair context with memory-efficient metadata loading and default console reporter
    pub fn new_with_metadata(
        packets: Vec<Packet>,
        metadata: Vec<RecoverySliceMetadata>,
        base_path: PathBuf,
    ) -> Result<Self> {
        Self::new_with_metadata_and_reporter(
            packets,
            metadata,
            base_path,
            Box::new(ConsoleReporter::new(false)),
        )
    }

    /// Create a new repair context with a custom progress reporter
    pub fn new_with_reporter(
        packets: Vec<Packet>,
        base_path: PathBuf,
        reporter: Box<dyn ProgressReporter>,
    ) -> Result<Self> {
        let recovery_set = Self::extract_recovery_set_info(packets)?;
        Ok(RepairContext {
            recovery_set,
            base_path,
            memory_limit: None,
            reporter,
            repair_created_backups: Mutex::new(Vec::new()),
        })
    }

    /// Create a new repair context with metadata and custom reporter
    pub fn new_with_metadata_and_reporter(
        packets: Vec<Packet>,
        metadata: Vec<RecoverySliceMetadata>,
        base_path: PathBuf,
        reporter: Box<dyn ProgressReporter>,
    ) -> Result<Self> {
        let mut recovery_set = Self::extract_recovery_set_info(packets)?;
        recovery_set.recovery_slices_metadata = metadata;
        Ok(RepairContext {
            recovery_set,
            base_path,
            memory_limit: None,
            reporter,
            repair_created_backups: Mutex::new(Vec::new()),
        })
    }

    pub(super) fn set_memory_limit(&mut self, memory_limit: Option<usize>) {
        self.memory_limit = memory_limit;
    }

    /// Get a reference to the progress reporter
    pub(super) fn reporter(&self) -> &dyn ProgressReporter {
        self.reporter.as_ref()
    }

    pub(super) fn record_repair_created_backup(&self, path: PathBuf) {
        let mut backups = self
            .repair_created_backups
            .lock()
            .unwrap_or_else(|poisoned| {
                warn!("repair backup tracking mutex was poisoned; recovering state");
                poisoned.into_inner()
            });
        backups.push(path);
    }

    /// Extract recovery set information from packets
    fn extract_recovery_set_info(packets: Vec<Packet>) -> Result<RecoverySetInfo> {
        // Use functional packet processing for clean separation
        let (main_packet, file_descriptions, input_file_slice_checksums, _recovery_count) =
            crate::packets::processing::separate_packets(packets);

        let main = main_packet.ok_or(RepairError::NoMainPacket)?;

        if file_descriptions.is_empty() {
            return Err(RepairError::NoFileDescriptions);
        }

        // Build lookup map for O(1) access
        let fd_map: HashMap<FileId, FileDescriptionPacket> = file_descriptions
            .into_iter()
            .map(|fd| (fd.file_id, fd))
            .collect();

        // Build file information in the order specified by main.file_ids
        // This is critical for correct global slice indexing!
        let mut files = Vec::new();
        let mut global_slice_offset = 0;

        debug!(
            "Building file list from main packet's file_ids array ({} files)",
            main.file_ids.len()
        );

        for (idx, file_id) in main.file_ids.iter().enumerate() {
            let fd = fd_map
                .get(file_id)
                .ok_or_else(|| RepairError::MissingFileDescription(format!("{:?}", file_id)))?;

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
                file_name: file_name.clone(),
                file_length: FileSize::new(fd.file_length),
                md5_hash: fd.md5_hash,
                md5_16k: fd.md5_16k,
                slice_count: BlockCount::new(slice_count as u32),
                global_slice_offset: GlobalSliceIndex::new(global_slice_offset),
            });

            debug!(
                "  CONSTRUCTED FileInfo[{}]: {} - offset: {}, slices: {}, md5: {}",
                files.len() - 1,
                file_name,
                global_slice_offset,
                slice_count,
                hex::encode(fd.md5_hash.as_bytes())
            );

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
            slice_size: BlockSize::new(main.slice_size),
            files,
            recovery_slices_metadata: Vec::new(), // Populated later for memory-efficient loading
            file_slice_checksums,
        })
    }

    /// Purge backup files and PAR2 files after an actual successful repair.
    /// Matches par2cmdline's -p repair behavior.
    pub fn purge_files(&self, par2_file: &str) -> Result<()> {
        self.reporter.report_purge_backup_files();
        self.purge_backup_files()?;

        let backups = self
            .repair_created_backups
            .lock()
            .unwrap_or_else(|poisoned| {
                warn!("repair backup tracking mutex was poisoned during purge; recovering state");
                poisoned.into_inner()
            })
            .clone();
        for backup_path in backups {
            if backup_path.exists() {
                delete_file(&backup_path)?;

                self.reporter
                    .report_purge_remove(&backup_path.file_name().unwrap().to_string_lossy());
            }
        }

        self.purge_par_files(par2_file)
    }

    /// Purge PAR2 files without removing backups.
    ///
    /// par2cmdline-turbo uses this path for `verify -p` and for `repair -p`
    /// when all files are already correct.
    pub fn purge_par_files(&self, par2_file: &str) -> Result<()> {
        self.reporter.report_purge_par_files();
        Self::purge_par_files_for(par2_file)
    }

    /// Purge PAR2 files without a repair context.
    pub fn purge_par_files_for(par2_file: &str) -> Result<()> {
        for path in crate::par2_files::collect_par2_files(Path::new(par2_file)) {
            if path.exists() {
                delete_file(&path)?;
            }
        }

        Ok(())
    }

    fn purge_backup_files(&self) -> Result<()> {
        for file_info in &self.recovery_set.files {
            let file_path = self.base_path.join(&file_info.file_name);
            let Some(parent) = file_path.parent() else {
                continue;
            };
            let Some(file_name) = file_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }

                    let Some(candidate) = path.file_name().and_then(|name| name.to_str()) else {
                        continue;
                    };

                    if is_numeric_backup_for(candidate, file_name) {
                        delete_file(&path)?;

                        self.reporter
                            .report_purge_remove(&entry.file_name().to_string_lossy());
                    }
                }
            }
        }

        Ok(())
    }
}

fn is_numeric_backup_for(candidate: &str, original_name: &str) -> bool {
    let Some(suffix) = candidate.strip_prefix(original_name) else {
        return false;
    };

    suffix.strip_prefix('.').is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::FileId;
    use crate::packets::{MainPacket, Packet};
    use tempfile::TempDir;

    fn create_main_packet(file_ids: Vec<FileId>) -> MainPacket {
        MainPacket {
            length: 72 + (file_ids.len() as u64 * 16),
            md5: crate::domain::Md5Hash::new([0; 16]),
            slice_size: 1024,
            set_id: crate::domain::RecoverySetId::new([1; 16]),
            file_count: file_ids.len() as u32,
            file_ids,
            non_recovery_file_ids: Vec::new(),
        }
    }

    fn create_file_desc(file_id: FileId, file_name: &str) -> FileDescriptionPacket {
        FileDescriptionPacket {
            length: 120 + file_name.len() as u64,
            md5: crate::domain::Md5Hash::new([0; 16]),
            set_id: crate::domain::RecoverySetId::new([1; 16]),
            packet_type: *b"PAR 2.0\0FileDesc",
            file_id,
            file_length: 1024,
            md5_hash: crate::domain::Md5Hash::new([2; 16]),
            md5_16k: crate::domain::Md5Hash::new([3; 16]),
            file_name: file_name.as_bytes().to_vec(),
        }
    }

    #[test]
    fn poisoned_backup_tracking_still_records_and_purges_backups() {
        let dir = TempDir::new().unwrap();
        let file_id = FileId::new([4; 16]);
        let packets = vec![
            Packet::Main(create_main_packet(vec![file_id])),
            Packet::FileDescription(create_file_desc(file_id, "test.txt")),
        ];
        let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();

        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = context.repair_created_backups.lock().unwrap();
            panic!("poison backup tracking mutex");
        }));

        let backup = dir.path().join("test.1");
        let par2_file = dir.path().join("test.par2");
        std::fs::write(&backup, b"backup").unwrap();
        std::fs::write(&par2_file, b"par2").unwrap();

        context.record_repair_created_backup(backup.clone());
        context.purge_files(par2_file.to_str().unwrap()).unwrap();

        assert!(!backup.exists());
        assert!(!par2_file.exists());
    }
}
