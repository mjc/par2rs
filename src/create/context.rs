//! CreateContext - Main context for PAR2 creation
//!
//! Reference: par2cmdline-turbo/src/par2creator.h Par2Creator class

use super::error::{CreateError, CreateResult};
use super::hashing::hash_all_source_files;
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
        // TODO: Generate RecoverySetId from main packet data
        // Reference: par2cmdline-turbo MainPacket

        Ok(())
    }

    /// Generate Reed-Solomon recovery blocks
    ///
    /// Reference: par2cmdline-turbo/src/par2creator.cpp ProcessData()
    fn generate_recovery_blocks(&mut self) -> CreateResult<()> {
        // TODO: Implement Reed-Solomon encoding
        // - Read source blocks in chunks
        // - Apply RS encoding using existing reed_solomon module
        // - Generate recovery blocks
        // - Compute recovery block checksums

        self.reporter
            .report_error("Recovery block generation not yet implemented");
        Err(CreateError::Other(
            "Recovery block generation not yet implemented".to_string(),
        ))
    }

    /// Write PAR2 files (index + volume files)
    ///
    /// Reference: par2cmdline-turbo/src/par2creator.cpp WriteCriticalPackets() and
    /// WriteRecoveryPacketHeaders()
    fn write_par2_files(&mut self) -> CreateResult<()> {
        // TODO: Implement PAR2 file writing
        // - Generate all packet structures
        // - Write main index file (base.par2)
        // - Write volume files (base.vol##.par2)
        // - Follow recovery file scheme

        self.reporter
            .report_error("PAR2 file writing not yet implemented");
        Err(CreateError::Other(
            "PAR2 file writing not yet implemented".to_string(),
        ))
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
