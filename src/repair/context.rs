//! Repair context management

use super::error::{RepairError, Result};
use super::progress::{ConsoleReporter, ProgressReporter};
use super::types::{FileInfo, RecoverySetInfo};
use crate::domain::{FileId, GlobalSliceIndex};
use crate::{
    FileDescriptionPacket, InputFileSliceChecksumPacket, MainPacket, Packet, RecoverySliceMetadata,
};
use log::debug;
use rustc_hash::FxHashMap as HashMap;
use std::path::PathBuf;

/// Main repair context containing all necessary information for repair operations
pub struct RepairContext {
    pub recovery_set: RecoverySetInfo,
    pub base_path: PathBuf,
    reporter: Box<dyn ProgressReporter>,
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
            reporter,
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
            reporter,
        })
    }

    /// Get a reference to the progress reporter
    pub(super) fn reporter(&self) -> &dyn ProgressReporter {
        self.reporter.as_ref()
    }

    /// Extract recovery set information from packets
    fn extract_recovery_set_info(packets: Vec<Packet>) -> Result<RecoverySetInfo> {
        let mut main_packet: Option<MainPacket> = None;
        let mut file_descriptions: Vec<FileDescriptionPacket> = Vec::new();
        let mut input_file_slice_checksums: Vec<InputFileSliceChecksumPacket> = Vec::new();

        // Collect packets by type (excluding RecoverySlice - handled via metadata)
        for packet in packets {
            match packet {
                Packet::Main(main) => {
                    main_packet = Some(main);
                }
                Packet::FileDescription(fd) => {
                    file_descriptions.push(fd);
                }
                Packet::RecoverySlice(_) => {
                    // Skip - recovery slices are loaded via metadata for memory efficiency
                }
                Packet::InputFileSliceChecksum(ifsc) => {
                    input_file_slice_checksums.push(ifsc);
                }
                _ => {} // Ignore other packet types for now
            }
        }

        let main = main_packet.ok_or(RepairError::NoMainPacket)?;

        if file_descriptions.is_empty() {
            return Err(RepairError::NoFileDescriptions);
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
                file_length: fd.file_length,
                md5_hash: fd.md5_hash,
                md5_16k: fd.md5_16k,
                slice_count,
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
            slice_size: main.slice_size,
            files,
            recovery_slices_metadata: Vec::new(), // Populated later for memory-efficient loading
            file_slice_checksums,
        })
    }

    /// Purge backup files and PAR2 files after successful repair
    /// Matches par2cmdline's -p flag behavior
    pub fn purge_files(&self, par2_file: &str) -> Result<()> {
        use std::fs;
        use std::path::Path;

        let par2_path = Path::new(par2_file);
        let par2_dir = par2_path
            .parent()
            .ok_or_else(|| RepairError::InvalidPath(par2_path.to_path_buf()))?;

        // Print purge backup files message
        println!("\nPurge backup files.");

        // Remove backup files (.1, .bak, etc.) for all files in the recovery set
        for file_info in &self.recovery_set.files {
            let file_path = self.base_path.join(&file_info.file_name);
            
            // Try common backup file extensions
            for ext in &["1", "bak"] {
                let backup_path = file_path.with_extension(ext);
                if backup_path.exists() {
                    fs::remove_file(&backup_path).map_err(|e| RepairError::FileDeleteError {
                        file: backup_path.clone(),
                        source: e,
                    })?;
                    
                    println!("Remove \"{}\".", backup_path.file_name().unwrap().to_string_lossy());
                }
            }
        }

        // Print purge par files message
        println!("\nPurge par files.");

        // Remove all PAR2 files in the directory
        if let Ok(entries) = fs::read_dir(par2_dir) {
            for entry in entries.flatten() {
                if let Some(ext) = entry.path().extension() {
                    if ext == "par2" {
                        fs::remove_file(&entry.path()).map_err(|e| RepairError::FileDeleteError {
                            file: entry.path(),
                            source: e,
                        })?;
                        
                        println!("Remove \"{}\".", entry.file_name().to_string_lossy());
                    }
                }
            }
        }

        Ok(())
    }
}

