//! PAR2 verification tool
//!
//! This tool verifies the integrity of files using PAR2 (Parity Archive) files.
//! It loads PAR2 packets from the main and volume files, displays statistics,
//! and verifies that the protected files are intact.
//!
//! This implementation follows the par2cmdline approach:
//! - Performs whole-file verification using MD5 hashes
//! - For damaged files, performs block-level verification
//! - Reports which blocks are broken and calculates repair requirements
//! - Determines if repair is possible with available recovery blocks

use anyhow::{Context, Result};
use par2rs::{analysis, file_ops, verify};
use std::path::Path;

fn main() -> Result<()> {
    // Initialize the logger
    env_logger::Builder::from_default_env()
        .format_timestamp(None)
        .format_module_path(false)
        .format_target(false)
        .init();

    let matches = par2rs::parse_args();

    let input_file = matches
        .get_one::<String>("input")
        .expect("input is required by clap");

    let file_path = Path::new(input_file);

    // Validate file exists
    anyhow::ensure!(file_path.exists(), "File does not exist: {}", input_file);

    // Change to parent directory for file resolution
    if let Some(parent) = file_path.parent() {
        std::env::set_current_dir(parent)
            .with_context(|| format!("Failed to set current directory to {}", parent.display()))?;
    }

    // Collect all PAR2 files in the set
    let par2_files = file_ops::collect_par2_files(file_path);

    // Parse all packets including recovery slices for verification (in parallel)
    println!("Loading PAR2 files...\n");
    let all_packets = file_ops::load_all_par2_packets(&par2_files);

    let total_recovery_blocks = all_packets
        .iter()
        .filter(|p| matches!(p, par2rs::Packet::RecoverySlice(_)))
        .count();

    println!(); // Blank line after loading

    // Show summary statistics
    let stats = analysis::calculate_par2_stats(&all_packets, total_recovery_blocks);
    analysis::print_summary_stats(&stats);

    // Perform comprehensive verification
    println!("\nVerifying source files:\n");
    let verification_results = verify::comprehensive_verify_files(all_packets);

    // Print detailed results
    verify::print_verification_results(&verification_results);

    // Return success if no repair is needed, error if repair is required
    anyhow::ensure!(
        verification_results.missing_block_count == 0,
        "Repair required: {} blocks are missing or damaged",
        verification_results.missing_block_count
    );

    Ok(())
}
