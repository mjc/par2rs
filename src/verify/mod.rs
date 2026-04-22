//! File verification module
//!
//! This module provides comprehensive PAR2 file verification functionality,
//! including whole-file MD5 verification and block-level validation.

mod config;
mod error;
mod file_verification;
mod global_engine;
mod global_table;
mod scanner_state;
mod types;
mod utils;
pub(crate) mod validation;
mod verifier;

// Re-export public types
pub use config::VerificationConfig;
pub use error::{VerificationError, VerificationResult};
pub use file_verification::{
    calculate_file_md5, calculate_file_md5_16k, format_display_name,
    verify_files_and_collect_results_with_base_dir, verify_single_file,
    verify_single_file_with_base_dir,
};
pub use global_engine::{GlobalFileVerificationResult, GlobalVerificationEngine};
pub use global_table::{
    GlobalBlockEntry, GlobalBlockPosition, GlobalBlockTable, GlobalBlockTableBuilder,
};
pub use types::{
    BlockVerificationResult, FileScanMetadata, FileStatus, FileVerificationResult,
    VerificationResults,
};
pub use utils::extract_file_name;
pub use validation::{validate_slices_crc32, validate_slices_crc32_with_progress};
pub use verifier::FileVerifier;

use crate::reporters::VerificationReporter;
use std::path::{Path, PathBuf};

/// Comprehensive verification with global block table approach
///
/// This is the main verification function. All other verification APIs should use this.
///
/// This function performs detailed verification using global block table approach:
/// 1. Builds a global block table from all slice checksums  
/// 2. Scans all available files with rolling CRC to find blocks anywhere
/// 3. Reports which blocks are available and calculates repair requirements
/// 4. Determines if repair is possible with available recovery blocks
///
/// # Arguments
/// * `packet_set` - PAR2 packets and metadata
/// * `config` - Verification configuration (threading, parallel/sequential)
/// * `reporter` - Progress reporter for verification events
/// * `base_dir` - Base directory for resolving file paths
pub fn comprehensive_verify_files<R: VerificationReporter>(
    packet_set: crate::par2_files::PacketSet,
    config: &VerificationConfig,
    reporter: &R,
    base_dir: impl AsRef<Path>,
) -> VerificationResults {
    comprehensive_verify_files_with_extra_files(packet_set, config, reporter, base_dir, &[])
}

/// Comprehensive verification with additional user-supplied files to scan.
///
/// Extra files are scanned for matching data blocks but do not limit the target
/// recovery set. This mirrors par2cmdline's optional `[files]` arguments.
pub fn comprehensive_verify_files_with_extra_files<R: VerificationReporter>(
    packet_set: crate::par2_files::PacketSet,
    config: &VerificationConfig,
    reporter: &R,
    base_dir: impl AsRef<Path>,
    extra_files: &[PathBuf],
) -> VerificationResults {
    // Note: Rayon thread pool is configured at program start in main binary
    // (see src/bin/par2.rs handle_verify function)

    reporter.report_verification_start(config.parallel);

    // Create global verification engine
    let engine = match GlobalVerificationEngine::from_packets_with_config(
        &packet_set.packets,
        &base_dir,
        config,
    ) {
        Ok(engine) => engine,
        Err(err) => {
            // For empty packet lists or missing main packets, return empty results
            // but with repair_possible = true (0 missing blocks = mathematically repairable)
            if !config.parallel {
                eprintln!("Error creating verification engine: {}", err);
            }
            return VerificationResults {
                files: Vec::new(),
                blocks: Vec::new(),
                present_file_count: 0,
                renamed_file_count: 0,
                corrupted_file_count: 0,
                missing_file_count: 0,
                available_block_count: 0,
                missing_block_count: 0,
                total_block_count: 0,
                recovery_blocks_available: 0,
                repair_possible: true, // 0 missing blocks is mathematically repairable
                blocks_needed_for_repair: 0,
            };
        }
    };

    // Report global table statistics
    let stats = engine.block_table().stats();
    reporter.report_files_found(stats.file_count);

    // Perform verification using global table. When -T/--file-threads is set,
    // use a local Rayon pool so file-level scanning is bounded independently
    // of the process-wide CPU pool.
    let mut results = if config.should_parallelize() {
        if let Some(file_threads) = config.file_threads {
            match rayon::ThreadPoolBuilder::new()
                .num_threads(file_threads)
                .build()
            {
                Ok(pool) => pool.install(|| {
                    engine.verify_recovery_set_with_extra_files(reporter, true, extra_files)
                }),
                Err(_) => engine.verify_recovery_set_with_extra_files(reporter, true, extra_files),
            }
        } else {
            engine.verify_recovery_set_with_extra_files(reporter, true, extra_files)
        }
    } else {
        engine.verify_recovery_set_with_extra_files(reporter, false, extra_files)
    };

    // Use the recovery block count from the packet set
    results.recovery_blocks_available = packet_set.recovery_block_count;
    results.repair_possible = packet_set.recovery_block_count >= results.missing_block_count;

    results
}
