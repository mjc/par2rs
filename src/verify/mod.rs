//! File verification module
//!
//! This module provides comprehensive PAR2 file verification functionality,
//! including whole-file MD5 verification and block-level validation.

mod config;
mod error;
mod file_verification;
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
pub use types::{BlockVerificationResult, FileStatus, FileVerificationResult, VerificationResults};
pub use utils::extract_file_name;
pub use validation::{
    validate_blocks_md5_crc32, validate_slices_crc32, validate_slices_crc32_with_progress,
};
pub use verifier::FileVerifier;

use crate::domain::{Crc32Value, FileId, Md5Hash};
use crate::packets::processing::*;
use crate::reporters::{ConsoleVerificationReporter, VerificationReporter};
use rayon::prelude::*;
use rustc_hash::FxHashMap as HashMap;

/// Comprehensive verification function with configuration and reporter support
///
/// This function performs detailed verification similar to par2cmdline:
/// 1. Verifies files at the whole-file level using MD5 hashes (SINGLE PASS)
/// 2. For corrupted files, performs block-level verification using slice checksums
/// 3. Reports which blocks are broken and calculates repair requirements
/// 4. Determines if repair is possible with available recovery blocks
pub fn comprehensive_verify_files_with_config_and_reporter<R: VerificationReporter>(
    packets: Vec<crate::Packet>,
    config: &VerificationConfig,
    reporter: &R,
) -> VerificationResults {
    // Configure rayon thread pool for compute-intensive operations
    let threads = config.effective_threads();
    if threads > 0 {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()
            .unwrap_or_else(|_| {
                eprintln!(
                    "Warning: Could not set thread count to {}, using default",
                    threads
                );
            });
    }

    // All operations use the configured thread pool for parallel processing
    comprehensive_verify_files_impl(packets, config.parallel, reporter)
}

/// Comprehensive verification function based on par2cmdline approach
///
/// This function performs detailed verification similar to par2cmdline:
/// 1. Verifies files at the whole-file level using MD5 hashes (SINGLE PASS)
/// 2. For corrupted files, performs block-level verification using slice checksums
/// 3. Reports which blocks are broken and calculates repair requirements
/// 4. Determines if repair is possible with available recovery blocks
pub fn comprehensive_verify_files(packets: Vec<crate::Packet>) -> VerificationResults {
    let config = VerificationConfig::default();
    let reporter = ConsoleVerificationReporter::new();
    comprehensive_verify_files_with_config_and_reporter(packets, &config, &reporter)
}

/// Comprehensive verification function with configuration support
///
/// Uses console reporter by default. For custom reporting, use the full function.
pub fn comprehensive_verify_files_with_config(
    packets: Vec<crate::Packet>,
    config: &VerificationConfig,
) -> VerificationResults {
    let reporter = ConsoleVerificationReporter::new();
    comprehensive_verify_files_with_config_and_reporter(packets, config, &reporter)
}

/// Functional helpers for results aggregation
mod results_aggregation {
    use super::*;

    /// Aggregate file verification results into final verification results
    pub fn aggregate_verification_results(
        file_results: Vec<SingleFileVerificationResult>,
        recovery_blocks_available: usize,
    ) -> VerificationResults {
        let (files, blocks, stats) = file_results.into_iter().fold(
            (Vec::new(), Vec::new(), FileStats::default()),
            |(mut files, mut blocks, stats), file_result| {
                // Functionally update stats based on file status
                let updated_stats = stats.with_status_update(
                    &file_result.status,
                    file_result.total_blocks,
                    file_result.blocks_available,
                    file_result.damaged_blocks.len(),
                );

                // Collect results
                blocks.extend(file_result.block_results);
                files.push(file_result.file_info);

                (files, blocks, updated_stats)
            },
        );

        VerificationResults {
            files,
            blocks,
            present_file_count: stats.present_files,
            renamed_file_count: stats.renamed_files,
            corrupted_file_count: stats.corrupted_files,
            missing_file_count: stats.missing_files,
            available_block_count: stats.available_blocks,
            missing_block_count: stats.missing_blocks,
            total_block_count: stats.total_blocks,
            recovery_blocks_available,
            repair_possible: recovery_blocks_available >= stats.missing_blocks,
            blocks_needed_for_repair: stats.missing_blocks,
        }
    }

    /// Helper struct to accumulate file statistics (immutable functional approach)
    #[derive(Default, Clone)]
    struct FileStats {
        present_files: usize,
        renamed_files: usize,
        corrupted_files: usize,
        missing_files: usize,
        available_blocks: usize,
        missing_blocks: usize,
        total_blocks: usize,
    }

    impl FileStats {
        /// Functionally update stats based on file status, returning new instance
        fn with_status_update(
            mut self,
            status: &FileStatus,
            total_blocks: usize,
            blocks_available: usize,
            damaged_count: usize,
        ) -> Self {
            self.total_blocks += total_blocks;

            match status {
                FileStatus::Missing => {
                    self.missing_files += 1;
                    self.missing_blocks += total_blocks;
                }
                FileStatus::Present => {
                    self.present_files += 1;
                    self.available_blocks += total_blocks;
                }
                FileStatus::Corrupted => {
                    self.corrupted_files += 1;
                    self.available_blocks += blocks_available;
                    self.missing_blocks += damaged_count;
                }
                FileStatus::Renamed => {
                    self.renamed_files += 1;
                }
            }
            self
        }
    }
}

/// Unified verification implementation that supports both parallel and sequential modes
fn comprehensive_verify_files_impl<R: VerificationReporter>(
    packets: Vec<crate::Packet>,
    parallel: bool,
    reporter: &R,
) -> VerificationResults {
    reporter.report_verification_start(parallel);

    // Extract packet information using functional helpers
    let block_size = extract_main_packet(&packets)
        .map(|m| m.slice_size)
        .unwrap_or(0);

    let recovery_blocks_available = count_recovery_blocks(&packets);
    let file_descriptions = extract_file_descriptions(&packets);
    let slice_checksums = extract_slice_checksums(&packets);

    reporter.report_files_found(file_descriptions.len());

    // Verify files using functional approach with parallel/sequential modes
    let file_results = verify_files_functional(
        &file_descriptions,
        &slice_checksums,
        block_size,
        parallel,
        reporter,
    );

    // Aggregate all results functionally
    results_aggregation::aggregate_verification_results(file_results, recovery_blocks_available)
}

/// Functional file verification that chooses parallel or sequential mode
fn verify_files_functional<R: VerificationReporter>(
    file_descriptions: &[&crate::packets::FileDescriptionPacket],
    slice_checksums: &HashMap<FileId, Vec<(Md5Hash, Crc32Value)>>,
    block_size: u64,
    parallel: bool,
    reporter: &R,
) -> Vec<SingleFileVerificationResult> {
    if parallel {
        let progress_reporter = crate::checksum::ConsoleProgressReporter::new();
        file_descriptions
            .par_iter()
            .map(|file_desc| {
                verify_single_file_impl(
                    file_desc,
                    slice_checksums,
                    block_size,
                    Some(&progress_reporter),
                    reporter,
                )
            })
            .collect()
    } else {
        file_descriptions
            .iter()
            .map(|file_desc| {
                verify_single_file_impl(
                    file_desc,
                    slice_checksums,
                    block_size,
                    None::<&crate::checksum::SilentProgressReporter>,
                    reporter,
                )
            })
            .collect()
    }
}

/// Result of verifying a single file (for parallel processing)
struct SingleFileVerificationResult {
    file_info: FileVerificationResult,
    block_results: Vec<BlockVerificationResult>,
    total_blocks: usize,
    blocks_available: usize,
    status: FileStatus,
    damaged_blocks: Vec<u32>,
}

/// Verify a single file (thread-safe for parallel execution) using functional patterns
///
/// This unified function consolidates the previous separate implementations
/// for progress and non-progress verification into a single, efficient function.
fn verify_single_file_impl<P: crate::checksum::ProgressReporter, R: VerificationReporter>(
    file_desc: &crate::packets::FileDescriptionPacket,
    slice_checksums: &HashMap<FileId, Vec<(Md5Hash, Crc32Value)>>,
    block_size: u64,
    progress: Option<&P>,
    reporter: &R,
) -> SingleFileVerificationResult {
    let file_name = extract_file_name(file_desc);
    let total_blocks = if block_size > 0 {
        file_desc.file_length.div_ceil(block_size) as usize
    } else {
        0
    };

    reporter.report_verifying_file(&file_name);

    // Determine file status functionally
    let file_status =
        verifier::single_file_verification::determine_file_status(file_desc, progress);
    reporter.report_file_status(&file_name, file_status.clone());

    // Process based on status using functional patterns
    match file_status {
        FileStatus::Present => create_present_file_result(file_desc, &file_name, total_blocks),
        FileStatus::Missing => create_missing_file_result(file_desc, &file_name, total_blocks),
        FileStatus::Corrupted | FileStatus::Renamed => create_corrupted_file_result(
            file_desc,
            &file_name,
            total_blocks,
            slice_checksums,
            block_size,
            reporter,
        ),
    }
}

/// Create result for present (valid) files
fn create_present_file_result(
    file_desc: &crate::packets::FileDescriptionPacket,
    file_name: &str,
    total_blocks: usize,
) -> SingleFileVerificationResult {
    let block_results = verifier::single_file_verification::generate_block_results(
        file_desc.file_id,
        0..total_blocks,
        true,
        None,
    );

    SingleFileVerificationResult {
        file_info: FileVerificationResult {
            file_name: file_name.to_string(),
            file_id: file_desc.file_id,
            status: FileStatus::Present,
            blocks_available: total_blocks,
            total_blocks,
            damaged_blocks: Vec::new(),
        },
        block_results,
        total_blocks,
        blocks_available: total_blocks,
        status: FileStatus::Present,
        damaged_blocks: Vec::new(),
    }
}

/// Create result for missing files
fn create_missing_file_result(
    file_desc: &crate::packets::FileDescriptionPacket,
    file_name: &str,
    total_blocks: usize,
) -> SingleFileVerificationResult {
    let block_results = verifier::single_file_verification::generate_block_results(
        file_desc.file_id,
        0..total_blocks,
        false,
        None,
    );

    SingleFileVerificationResult {
        file_info: FileVerificationResult {
            file_name: file_name.to_string(),
            file_id: file_desc.file_id,
            status: FileStatus::Missing,
            blocks_available: 0,
            total_blocks,
            damaged_blocks: Vec::new(),
        },
        block_results,
        total_blocks,
        blocks_available: 0,
        status: FileStatus::Missing,
        damaged_blocks: Vec::new(),
    }
}

/// Create result for corrupted files with optional block-level verification
fn create_corrupted_file_result<R: VerificationReporter>(
    file_desc: &crate::packets::FileDescriptionPacket,
    file_name: &str,
    total_blocks: usize,
    slice_checksums: &HashMap<FileId, Vec<(Md5Hash, Crc32Value)>>,
    block_size: u64,
    reporter: &R,
) -> SingleFileVerificationResult {
    slice_checksums
        .get(&file_desc.file_id)
        .map(|checksums| {
            // Perform block-level verification
            let (available_blocks, damaged_blocks) =
                validation::validate_blocks_md5_crc32(file_name, checksums, block_size as usize);

            if !damaged_blocks.is_empty() {
                reporter.report_damaged_blocks(file_name, &damaged_blocks);
            }

            let block_results = verifier::single_file_verification::create_corrupted_block_results(
                file_desc.file_id,
                checksums,
                &damaged_blocks,
            );

            SingleFileVerificationResult {
                file_info: FileVerificationResult {
                    file_name: file_name.to_string(),
                    file_id: file_desc.file_id,
                    status: FileStatus::Corrupted,
                    blocks_available: available_blocks,
                    total_blocks,
                    damaged_blocks: damaged_blocks.clone(),
                },
                block_results,
                total_blocks,
                blocks_available: available_blocks,
                status: FileStatus::Corrupted,
                damaged_blocks,
            }
        })
        .unwrap_or_else(|| {
            // No checksums available - assume all blocks damaged
            let damaged_blocks: Vec<u32> = (0..total_blocks as u32).collect();
            let block_results = verifier::single_file_verification::generate_block_results(
                file_desc.file_id,
                0..total_blocks,
                false,
                None,
            );

            SingleFileVerificationResult {
                file_info: FileVerificationResult {
                    file_name: file_name.to_string(),
                    file_id: file_desc.file_id,
                    status: FileStatus::Corrupted,
                    blocks_available: 0,
                    total_blocks,
                    damaged_blocks: damaged_blocks.clone(),
                },
                block_results,
                total_blocks,
                blocks_available: 0,
                status: FileStatus::Corrupted,
                damaged_blocks,
            }
        })
}

/// Print verification results in par2cmdline style (legacy function)
pub fn print_verification_results(results: &VerificationResults) {
    let reporter = ConsoleVerificationReporter::new();
    reporter.report_verification_results(results);
}
