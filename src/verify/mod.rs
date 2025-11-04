//! File verification module
//!
//! This module provides comprehensive PAR2 file verification functionality,
//! including whole-file MD5 verification and block-level validation.

mod config;
mod error;
mod file_verification;
mod global_engine;
mod global_table;
#[cfg(test)]
mod test_global;
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
pub use types::{BlockVerificationResult, FileStatus, FileVerificationResult, VerificationResults};
pub use utils::extract_file_name;
pub use validation::{validate_slices_crc32, validate_slices_crc32_with_progress};
pub use verifier::FileVerifier;

use crate::reporters::{ConsoleVerificationReporter, VerificationReporter};
use std::path::Path;

/// Comprehensive verification with global block table approach
///
/// This function performs detailed verification using global block table approach:
/// 1. Builds a global block table from all slice checksums  
/// 2. Scans all available files with rolling CRC to find blocks anywhere
/// 3. Reports which blocks are available and calculates repair requirements
/// 4. Determines if repair is possible with available recovery blocks
pub fn comprehensive_verify_files_with_config_and_reporter<R: VerificationReporter>(
    packet_set: crate::par2_files::PacketSet,
    config: &VerificationConfig,
    reporter: &R,
) -> VerificationResults {
    let base_dir = packet_set.base_dir.clone();
    comprehensive_verify_files_with_config_and_reporter_in_dir(
        packet_set, config, reporter, base_dir,
    )
}

pub fn comprehensive_verify_files_with_config_and_reporter_in_dir<R: VerificationReporter>(
    packet_set: crate::par2_files::PacketSet,
    config: &VerificationConfig,
    reporter: &R,
    base_dir: impl AsRef<Path>,
) -> VerificationResults {
    // Note: Rayon thread pool is configured at program start in main binary
    // (see src/bin/par2.rs handle_verify function)

    reporter.report_verification_start(config.parallel);

    // Create global verification engine
    let engine = match GlobalVerificationEngine::from_packets(&packet_set.packets, &base_dir) {
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

    if config.parallel {
        eprintln!(
            "Global block table: {} total blocks, {} unique, {} duplicates",
            stats.total_blocks, stats.unique_checksums, stats.duplicate_blocks
        );
    }

    // Perform verification using global table
    // Use should_parallelize() to avoid Rayon overhead when threads=1
    let mut results = engine.verify_recovery_set(reporter, config.should_parallelize());

    // Use the recovery block count from the packet set
    results.recovery_blocks_available = packet_set.recovery_block_count;
    results.repair_possible = packet_set.recovery_block_count >= results.missing_block_count;

    results
}

/// Comprehensive verification function based on par2cmdline approach
///
/// This function performs detailed verification similar to par2cmdline:
/// 1. Verifies files at the whole-file level using MD5 hashes (SINGLE PASS)
/// 2. For corrupted files, performs block-level verification using slice checksums
/// 3. Reports which blocks are broken and calculates repair requirements
/// 4. Determines if repair is possible with available recovery blocks
pub fn comprehensive_verify_files(packets: Vec<crate::Packet>) -> VerificationResults {
    // Count recovery blocks from packets for backward compatibility
    let packet_set = crate::par2_files::PacketSet::from_packets(packets);
    let base_dir = packet_set.base_dir.clone();
    comprehensive_verify_files_in_dir(packet_set, base_dir)
}

/// Comprehensive verification function with base directory support
pub fn comprehensive_verify_files_in_dir(
    packet_set: crate::par2_files::PacketSet,
    base_dir: impl AsRef<Path>,
) -> VerificationResults {
    let config = VerificationConfig::default();
    let reporter = ConsoleVerificationReporter::new();
    comprehensive_verify_files_with_config_and_reporter_in_dir(
        packet_set, &config, &reporter, base_dir,
    )
}

/// Comprehensive verification function with configuration support
///
/// Uses console reporter by default. For custom reporting, use the full function.
pub fn comprehensive_verify_files_with_config(
    packet_set: crate::par2_files::PacketSet,
    config: &VerificationConfig,
) -> VerificationResults {
    let reporter = ConsoleVerificationReporter::new();
    comprehensive_verify_files_with_config_and_reporter(packet_set, config, &reporter)
}

/// Print verification results in par2cmdline style (legacy function)
pub fn print_verification_results(results: &VerificationResults) {
    let reporter = ConsoleVerificationReporter::new();
    reporter.report_verification_results(results);
}
