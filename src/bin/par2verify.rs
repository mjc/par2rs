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

use par2rs::{analysis, file_ops, verify};
use std::path::Path;

fn main() -> Result<(), ()> {
    let matches = par2rs::parse_args();

    let input_file = matches
        .get_one::<String>("input")
        .expect("Input file is required");

    let file_path = Path::new(input_file);
    if !file_path.exists() {
        eprintln!("File does not exist: {}", input_file);
        return Err(());
    }

    if let Some(parent) = file_path.parent() {
        if let Err(err) = std::env::set_current_dir(parent) {
            eprintln!(
                "Failed to set current directory to {}: {}",
                parent.display(),
                err
            );
            return Err(());
        }
    }

    let par2_files = file_ops::collect_par2_files(file_path);
    let (all_packets, total_recovery_blocks) = file_ops::load_all_par2_packets(&par2_files, true);

    // Show summary statistics
    let stats = analysis::calculate_par2_stats(&all_packets, total_recovery_blocks);
    analysis::print_summary_stats(&stats);

    // Perform comprehensive verification
    println!("\nVerifying source files:\n");
    let verification_results = verify::comprehensive_verify_files(all_packets);

    // Print detailed results
    verify::print_verification_results(&verification_results);

    // Return success if no repair is needed, error if repair is required
    if verification_results.missing_block_count == 0 {
        Ok(())
    } else {
        Err(())
    }
}
