//! CreateContext - Main context for PAR2 creation
//!
//! Reference: par2cmdline-turbo/src/par2creator.h Par2Creator class

use super::error::{CreateError, CreateResult};
use super::hashing::hash_all_source_files;
use super::packet_generator::generate_recovery_set_id;
use super::progress::CreateReporter;
use super::source_file::SourceFileInfo;
use super::types::CreateConfig;
use crate::domain::RecoverySetId;

/// Main context for PAR2 creation
///
/// This structure manages the entire PAR2 creation process:
/// 1. Scanning and validating source files
/// 2. Computing optimal block size
/// 3. Hashing files and blocks
/// 4. Generating Reed-Solomon recovery blocks
/// 5. Writing PAR2 files
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp Par2Creator::Process()
pub struct CreateContext {
    /// Configuration
    config: CreateConfig,

    /// Progress reporter
    reporter: Box<dyn CreateReporter>,

    /// Recovery set ID (generated from source files)
    #[allow(dead_code)] // Will be used when implementing packet generation
    recovery_set_id: Option<RecoverySetId>,

    /// Source file information
    source_files: Vec<SourceFileInfo>,

    /// Calculated block size (bytes)
    block_size: u64,

    /// Total number of source blocks across all files
    source_block_count: u32,

    /// Number of recovery blocks to generate
    recovery_block_count: u32,

    /// Generated recovery blocks (exponent, data)
    recovery_blocks: Vec<(u16, Vec<u8>)>,

    /// Output PAR2 files created
    output_files: Vec<String>,
}

impl CreateContext {
    /// Create a new CreateContext
    ///
    /// Called by CreateContextBuilder after validation
    /// Performs initial setup: scans files, calculates block size and recovery count
    pub(super) fn new(
        config: CreateConfig,
        reporter: Box<dyn CreateReporter>,
    ) -> CreateResult<Self> {
        let mut context = CreateContext {
            config,
            reporter,
            recovery_set_id: None,
            source_files: Vec::new(),
            block_size: 0,
            source_block_count: 0,
            recovery_block_count: 0,
            recovery_blocks: Vec::new(),
            output_files: Vec::new(),
        };

        // Perform initial setup
        context.scan_source_files()?;
        context.calculate_block_size()?;
        context.calculate_recovery_blocks()?;

        Ok(context)
    }

    /// Execute the PAR2 creation process
    ///
    /// This is the main entry point that orchestrates all creation steps
    /// Note: Initial setup (file scanning, block size calculation) is done during build()
    ///
    /// Reference: par2cmdline-turbo/src/par2creator.cpp Par2Creator::Process()
    pub fn create(&mut self) -> CreateResult<()> {
        // Step 1: Compute file hashes and block checksums
        self.hash_source_files()?;

        // Step 2: Generate recovery set ID
        self.generate_recovery_set_id()?;

        // Step 3: Generate recovery blocks
        self.generate_recovery_blocks()?;

        // Step 4: Write PAR2 files
        self.write_par2_files()?;

        // Report completion
        self.reporter.report_complete(&self.output_files);

        Ok(())
    }

    /// Scan source files and validate accessibility
    ///
    /// Reference: par2cmdline-turbo/src/par2creator.cpp OpenSourceFiles()
    fn scan_source_files(&mut self) -> CreateResult<()> {
        let total_files = self.config.source_files.len();

        for (index, path) in self.config.source_files.iter().enumerate() {
            self.reporter.report_scanning_files(
                index + 1,
                total_files,
                path.to_str().unwrap_or(""),
            );

            // Check file exists
            if !path.exists() {
                return Err(CreateError::FileNotFound(
                    path.to_string_lossy().to_string(),
                ));
            }

            // Get file metadata
            let metadata = std::fs::metadata(path).map_err(|e| CreateError::FileReadError {
                file: path.to_string_lossy().to_string(),
                source: e,
            })?;

            let size = metadata.len();
            let source_info = SourceFileInfo::new(path.clone(), size, index);

            self.source_files.push(source_info);
        }

        // Validate we have at least some data
        let total_size: u64 = self.source_files.iter().map(|f| f.size).sum();
        if total_size == 0 {
            return Err(CreateError::EmptySourceFiles);
        }

        Ok(())
    }

    /// Calculate optimal block size based on total file size
    ///
    /// Reference: par2cmdline-turbo/src/par2creator.cpp ComputeBlockCount()
    fn calculate_block_size(&mut self) -> CreateResult<()> {
        if let Some(block_size) = self.config.block_size {
            // User specified block size
            self.block_size = block_size;
        } else {
            // Auto-calculate based on total size
            let total_size: u64 = self.source_files.iter().map(|f| f.size).sum();

            // par2cmdline-turbo algorithm:
            // - Aim for 2000 blocks for optimal balance
            // - Round to multiple of 4 bytes (alignment)
            // - Minimum 512 bytes, maximum 16MB

            const TARGET_BLOCKS: u64 = 2000;
            const MIN_BLOCK_SIZE: u64 = 512;
            const MAX_BLOCK_SIZE: u64 = 16 * 1024 * 1024; // 16MB

            let calculated = total_size.div_ceil(TARGET_BLOCKS);
            let calculated = (calculated + 3) & !3; // Round up to multiple of 4

            self.block_size = calculated.clamp(MIN_BLOCK_SIZE, MAX_BLOCK_SIZE);
        }

        // Calculate total source block count
        self.source_block_count = self
            .source_files
            .iter()
            .map(|f| f.calculate_block_count(self.block_size))
            .sum();

        Ok(())
    }

    /// Calculate number of recovery blocks to generate
    fn calculate_recovery_blocks(&mut self) -> CreateResult<()> {
        if let Some(count) = self.config.recovery_block_count {
            // Explicit count specified
            self.recovery_block_count = count;
        } else if let Some(percent) = self.config.redundancy_percentage {
            // Calculate from percentage
            let count = (self.source_block_count as f64 * (percent as f64 / 100.0)).ceil() as u32;
            self.recovery_block_count = count.max(1); // At least 1 recovery block
        } else {
            return Err(CreateError::Other(
                "Must specify either recovery_block_count or redundancy_percentage".to_string(),
            ));
        }

        Ok(())
    }

    /// Compute MD5 hashes and checksums for all source files
    ///
    /// Reference: par2cmdline-turbo/src/par2creator.cpp OpenSourceFiles() and
    /// FinishFileHashComputation()
    fn hash_source_files(&mut self) -> CreateResult<()> {
        hash_all_source_files(&mut self.source_files, self.block_size, &*self.reporter)?;
        Ok(())
    }

    /// Generate recovery set ID
    fn generate_recovery_set_id(&mut self) -> CreateResult<()> {
        let set_id = generate_recovery_set_id(self.block_size, &self.source_files)?;
        self.recovery_set_id = Some(set_id);
        Ok(())
    }

    /// Generate Reed-Solomon recovery blocks
    ///
    /// Reference: par2cmdline-turbo/src/par2creator.cpp ProcessData()
    fn generate_recovery_blocks(&mut self) -> CreateResult<()> {
        use crate::reed_solomon::RecoveryBlockEncoder;
        use std::fs::File;
        use std::io::{Read, Seek, SeekFrom};

        if self.recovery_block_count == 0 {
            self.reporter.report_scanning_files(
                0,
                0,
                "No recovery blocks to generate (redundancy = 0%)",
            );
            return Ok(());
        }

        // Create encoder
        let encoder =
            RecoveryBlockEncoder::new(self.block_size as usize, self.source_block_count as usize);

        // Read all source blocks into memory
        // TODO: Implement chunked processing for very large files
        let mut source_blocks: Vec<Vec<u8>> = Vec::with_capacity(self.source_block_count as usize);

        for file in &self.source_files {
            let mut f = File::open(&file.path).map_err(|e| CreateError::FileReadError {
                file: file.path.to_string_lossy().to_string(),
                source: e,
            })?;

            let block_count = file.calculate_block_count(self.block_size);

            for block_idx in 0..block_count {
                let offset = block_idx as u64 * self.block_size;
                f.seek(SeekFrom::Start(offset))
                    .map_err(|e| CreateError::FileReadError {
                        file: file.path.to_string_lossy().to_string(),
                        source: e,
                    })?;

                let is_last_block = block_idx == block_count - 1;
                let block_len = if is_last_block && file.size % self.block_size != 0 {
                    (file.size % self.block_size) as usize
                } else {
                    self.block_size as usize
                };

                let mut block = vec![0u8; self.block_size as usize];
                f.read_exact(&mut block[..block_len])
                    .map_err(|e| CreateError::FileReadError {
                        file: file.path.to_string_lossy().to_string(),
                        source: e,
                    })?;

                source_blocks.push(block);
            }
        }

        // Generate recovery blocks
        let exponents: Vec<u16> = (0..self.recovery_block_count as u16).collect();
        let source_refs: Vec<&[u8]> = source_blocks.iter().map(|b| b.as_slice()).collect();

        self.reporter.report_scanning_files(
            0,
            self.recovery_block_count as usize,
            "Generating recovery blocks...",
        );

        let recovery_blocks = encoder
            .encode_recovery_blocks_parallel(&exponents, &source_refs)
            .map_err(|e| CreateError::Other(format!("Reed-Solomon encoding failed: {}", e)))?;

        self.reporter.report_scanning_files(
            self.recovery_block_count as usize,
            self.recovery_block_count as usize,
            "Recovery blocks generated",
        );

        // Store recovery blocks for writing
        self.recovery_blocks = recovery_blocks;

        Ok(())
    }

    /// Write PAR2 files (index + volume files)
    ///
    /// Reference: par2cmdline-turbo/src/par2creator.cpp WriteCriticalPackets() and
    /// WriteRecoveryPacketHeaders()
    fn write_par2_files(&mut self) -> CreateResult<()> {
        use super::packet_generator::{
            generate_creator_packet, generate_file_description_packet,
            generate_file_verification_packet, generate_main_packet, write_creator_packet,
            write_file_description_packet, write_file_verification_packet, write_main_packet,
        };
        use std::fs::File;
        use std::io::Write;

        let recovery_set_id = self
            .recovery_set_id
            .ok_or_else(|| CreateError::Other("Recovery set ID not generated".to_string()))?;

        // Generate all critical packets
        // Reference: par2cmdline-turbo/src/par2creator.cpp CreateMainPacket(), CreateCreatorPacket()
        let main_packet =
            generate_main_packet(recovery_set_id, self.block_size, &self.source_files)?;
        let creator_packet = generate_creator_packet(recovery_set_id)?;

        let file_desc_packets: Vec<_> = self
            .source_files
            .iter()
            .map(|f| generate_file_description_packet(recovery_set_id, f))
            .collect::<CreateResult<_>>()?;

        let file_verif_packets: Vec<_> = self
            .source_files
            .iter()
            .map(|f| generate_file_verification_packet(recovery_set_id, f))
            .collect::<CreateResult<_>>()?;

        // Determine output directory and base name
        let output_path = std::path::Path::new(&self.config.output_name);
        let output_dir = output_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();

        let base_name = output_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output")
            .to_string();

        // Write index file (base.par2)
        let index_path = output_dir.join(format!("{}.par2", base_name));
        let mut index_file =
            File::create(&index_path).map_err(|e| CreateError::FileCreateError {
                file: index_path.to_string_lossy().to_string(),
                source: e,
            })?;

        // Write packets using proper serialization functions that compute MD5
        // Reference: par2cmdline-turbo/src/par2creator.cpp WriteCriticalPackets()
        write_main_packet(&mut index_file, &main_packet)
            .map_err(|e| CreateError::Other(format!("Failed to write main packet: {}", e)))?;

        write_creator_packet(&mut index_file, &creator_packet)
            .map_err(|e| CreateError::Other(format!("Failed to write creator packet: {}", e)))?;

        for packet in &file_desc_packets {
            write_file_description_packet(&mut index_file, packet).map_err(|e| {
                CreateError::Other(format!("Failed to write file description packet: {}", e))
            })?;
        }

        for packet in &file_verif_packets {
            write_file_verification_packet(&mut index_file, packet).map_err(|e| {
                CreateError::Other(format!("Failed to write file verification packet: {}", e))
            })?;
        }

        // Write recovery slice packets
        // Reference: par2cmdline-turbo/src/par2creator.cpp WriteRecoveryPackets()
        for (exponent, recovery_data) in &self.recovery_blocks {
            use crate::packets::RecoverySlicePacket;
            use binrw::BinWrite;

            // Calculate packet length
            let packet_length = 8 + 8 + 16 + 16 + 16 + 4 + recovery_data.len() as u64;

            // Build packet
            let packet = RecoverySlicePacket {
                length: packet_length,
                md5: crate::domain::Md5Hash::new([0u8; 16]), // Will be computed
                set_id: recovery_set_id,
                type_of_packet: *b"PAR 2.0\0RecvSlic",
                exponent: *exponent as u32,
                recovery_data: recovery_data.clone(),
            };

            // Compute MD5 of packet body
            let mut md5_data = Vec::new();
            md5_data.extend_from_slice(recovery_set_id.as_bytes());
            md5_data.extend_from_slice(b"PAR 2.0\0RecvSlic");
            md5_data.extend_from_slice(&(*exponent as u32).to_le_bytes());
            md5_data.extend_from_slice(recovery_data);

            let computed_md5 = crate::checksum::compute_md5_bytes(&md5_data);

            let packet_with_md5 = RecoverySlicePacket {
                md5: crate::domain::Md5Hash::new(computed_md5),
                ..packet
            };

            // Write packet
            packet_with_md5.write_le(&mut index_file).map_err(|e| {
                CreateError::Other(format!("Failed to write recovery packet: {}", e))
            })?;
        }

        index_file
            .flush()
            .map_err(|e| CreateError::FileCreateError {
                file: index_path.to_string_lossy().to_string(),
                source: e,
            })?;

        self.output_files
            .push(index_path.to_string_lossy().to_string());

        Ok(())
    }

    /// Get the list of created output files
    pub fn output_files(&self) -> &[String] {
        &self.output_files
    }

    /// Get block size
    pub fn block_size(&self) -> u64 {
        self.block_size
    }

    /// Get recovery block count
    pub fn recovery_block_count(&self) -> u32 {
        self.recovery_block_count
    }

    /// Get source block count
    pub fn source_block_count(&self) -> u32 {
        self.source_block_count
    }
}
