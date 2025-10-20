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
        .expect("Input file is required");

    let file_path = Path::new(input_file);

    // Validate file exists
    anyhow::ensure!(file_path.exists(), "File does not exist: {}", input_file);

    // Change to parent directory if it exists
    if let Some(parent) = file_path.parent() {
        std::env::set_current_dir(parent)
            .with_context(|| format!("Failed to set current directory to {}", parent.display()))?;
    }

    let par2_files = file_ops::collect_par2_files(file_path);

    // Parse all packets including recovery slices for verification
    let (all_packets, total_recovery_blocks): (Vec<_>, usize) = par2_files
        .iter()
        .map(|par2_file| {
            let file = std::fs::File::open(par2_file)
                .with_context(|| format!("Failed to open PAR2 file: {}", par2_file.display()))?;
            let mut reader = std::io::BufReader::new(file);
            Ok(par2rs::parse_packets(&mut reader))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .fold(
            (Vec::new(), 0),
            |(mut packets, mut recovery_count), parsed| {
                recovery_count += parsed
                    .iter()
                    .filter(|p| matches!(p, par2rs::Packet::RecoverySlice(_)))
                    .count();
                packets.extend(parsed);
                (packets, recovery_count)
            },
        );

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
