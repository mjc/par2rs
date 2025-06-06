//! PAR2 verification tool
//!
//! This tool verifies the integrity of files using PAR2 (Parity Archive) files.
//! It loads PAR2 packets from the main and volume files, displays statistics,
//! and verifies that the protected files are intact.

use par2rs::{analysis, file_ops, file_verification};
use std::path::Path;

// ============================================================================
// Main Function and Program Flow
// ============================================================================

/// Handle verification results and print appropriate messages
fn handle_verification_results(
    file_descriptors_for_broken_files: Vec<par2rs::Packet>,
) -> Result<(), ()> {
    if file_descriptors_for_broken_files.is_empty() {
        println!("All files are correct, repair is not required.");
        Ok(())
    } else {
        println!(
            "Quick check failed for {} files. Attempting to verify packets...",
            file_descriptors_for_broken_files.len()
        );
        Err(())
    }
}

/// Verify source files and print progress information
fn verify_source_files_with_progress(packets: Vec<par2rs::Packet>) -> Vec<par2rs::Packet> {
    // Collect file information from FileDescription packets
    let file_info = analysis::collect_file_info_from_packets(&packets);

    // Verify each file and collect results
    let verification_results =
        file_verification::verify_files_and_collect_results(&file_info, true);

    // Collect file IDs for broken files
    let broken_file_ids: Vec<[u8; 16]> = verification_results
        .iter()
        .filter(|result| !result.is_valid)
        .map(|result| result.file_id)
        .collect();

    // Return FileDescription packets for broken files
    file_verification::find_broken_file_descriptors(packets, &broken_file_ids)
}

/// Verify packet integrity (placeholder implementation)
fn verify_packets(packets: Vec<par2rs::Packet>) -> Vec<par2rs::Packet> {
    packets // For now, just return all packets without verification
}

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

    let verified_packets = verify_packets(all_packets);

    // Verification phase
    println!("\nVerifying source files:\n");
    let file_descriptors_for_broken_files = verify_source_files_with_progress(verified_packets);

    handle_verification_results(file_descriptors_for_broken_files)
}
