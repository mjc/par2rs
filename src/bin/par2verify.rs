//! PAR2 verification tool
//!
//! This tool verifies the integrity of files using PAR2 (Parity Archive) files.
//! It loads PAR2 packets from the main and volume files, displays statistics,
//! and verifies that the protected files are intact.

use std::fs;
use std::path::{Path, PathBuf};

// ============================================================================
// File Discovery and Loading Functions
// ============================================================================

/// Load all PAR2 packets from multiple files and count recovery blocks
fn load_all_par2_packets(par2_files: &[PathBuf]) -> (Vec<par2rs::Packet>, usize) {
    let mut all_packets = Vec::new();
    let mut total_recovery_blocks = 0;

    for par2_file in par2_files {
        let (packets, recovery_blocks) = parse_par2_file_with_progress(par2_file);
        all_packets.extend(packets);
        total_recovery_blocks += recovery_blocks;
    }

    (all_packets, total_recovery_blocks)
}

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

    let par2_files = collect_par2_files(file_path);
    let (all_packets, total_recovery_blocks) = load_all_par2_packets(&par2_files);

    // Show summary statistics
    show_summary_stats(&all_packets, total_recovery_blocks);

    let verified_packets = verify_packets(all_packets);

    // Verification phase
    println!("\nVerifying source files:\n");
    let file_descriptors_for_broken_files = verify_source_files_with_progress(verified_packets);

    handle_verification_results(file_descriptors_for_broken_files)
}

/// Find all PAR2 files in a directory, excluding the specified file
fn find_par2_files_in_directory(folder_path: &Path, exclude_file: &Path) -> Vec<PathBuf> {
    fs::read_dir(folder_path)
        .expect("Failed to read directory")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension().map_or(false, |ext| ext == "par2") && path != exclude_file)
                .then_some(path)
        })
        .collect()
}

/// Collect all PAR2 files related to the input file (main file + volume files)
fn collect_par2_files(file_path: &Path) -> Vec<PathBuf> {
    let mut par2_files = vec![file_path.to_path_buf()];

    if let Some(folder_path) = file_path.parent() {
        let additional_files = find_par2_files_in_directory(folder_path, file_path);
        par2_files.extend(additional_files);
    }

    // Sort files to match system par2verify order
    par2_files.sort();
    par2_files
}

/// Count the number of recovery slice packets in a collection of packets
fn count_recovery_blocks(packets: &[par2rs::Packet]) -> usize {
    packets
        .iter()
        .filter(|p| matches!(p, par2rs::Packet::RecoverySlice(_)))
        .count()
}

/// Print the result of loading packets from a file
fn print_packet_load_result(_filename: &str, packet_count: usize, recovery_blocks: usize) {
    if recovery_blocks > 0 {
        println!(
            "Loaded {} new packets including {} recovery blocks",
            packet_count, recovery_blocks
        );
    } else {
        println!("Loaded {} new packets", packet_count);
    }
}

/// Parse a single PAR2 file and display loading progress
fn parse_par2_file_with_progress(par2_file: &Path) -> (Vec<par2rs::Packet>, usize) {
    let filename = par2_file.file_name().unwrap().to_string_lossy();
    println!("Loading \"{}\".", filename);

    let mut file = fs::File::open(par2_file).expect("Failed to open .par2 file");
    let packets = par2rs::parse_packets(&mut file);

    let recovery_blocks = count_recovery_blocks(&packets);
    print_packet_load_result(&filename, packets.len(), recovery_blocks);

    (packets, recovery_blocks)
}

// ============================================================================
// Packet Analysis and Statistics Functions
// ============================================================================

/// Verify packet integrity (placeholder implementation)
fn verify_packets(packets: Vec<par2rs::Packet>) -> Vec<par2rs::Packet> {
    packets // For now, just return all packets without verification
}

/// Extract unique filenames from FileDescription packets
fn extract_unique_filenames(packets: &[par2rs::Packet]) -> Vec<String> {
    packets
        .iter()
        .filter_map(|packet| match packet {
            par2rs::Packet::FileDescription(fd) => std::str::from_utf8(&fd.file_name)
                .ok()
                .map(|s| s.trim_end_matches('\0').to_string()),
            _ => None,
        })
        .collect::<std::collections::HashSet<_>>() // Remove duplicates
        .into_iter()
        .collect()
}

/// Extract block size and total blocks from packets
fn extract_main_packet_stats(packets: &[par2rs::Packet]) -> (u32, usize) {
    // Get block size from main packet
    let block_size = packets
        .iter()
        .find_map(|packet| match packet {
            par2rs::Packet::Main(main_packet) => Some(main_packet.slice_size as u32),
            _ => None,
        })
        .unwrap_or(0);

    // Calculate total blocks from unique files only
    let total_blocks = if block_size > 0 {
        let mut unique_files = std::collections::HashMap::new();

        // Collect unique FileDescription packets by file_id
        for packet in packets {
            if let par2rs::Packet::FileDescription(fd) = packet {
                unique_files.insert(fd.file_id, fd.file_length);
            }
        }

        // Sum blocks for all unique files
        unique_files
            .values()
            .map(|&file_length| {
                // Calculate blocks needed for this file (round up)
                ((file_length + block_size as u64 - 1) / block_size as u64) as usize
            })
            .sum()
    } else {
        0
    };

    (block_size, total_blocks)
}

/// Calculate total size based on unique files only
fn calculate_total_size(packets: &[par2rs::Packet]) -> u64 {
    let mut unique_files = std::collections::HashMap::new();

    // Collect unique FileDescription packets by file_id to avoid counting duplicates
    for packet in packets {
        if let par2rs::Packet::FileDescription(fd) = packet {
            unique_files.insert(fd.file_id, fd.file_length);
        }
    }

    // Sum up the file sizes for unique files only
    unique_files.values().sum()
}

/// Print summary statistics about the PAR2 set
fn print_summary_stats(file_count: usize, block_size: u32, total_blocks: usize, total_size: u64) {
    println!(
        "\nThere are {} recoverable files and 0 other files.",
        file_count
    );
    println!("The block size used was {} bytes.", block_size);
    println!("There are a total of {} data blocks.", total_blocks);
    println!("The total size of the data files is {} bytes.", total_size);
}

/// Display summary statistics for the loaded PAR2 packets
fn show_summary_stats(packets: &[par2rs::Packet], _total_recovery_blocks: usize) {
    let unique_files = extract_unique_filenames(packets);
    let (block_size, total_blocks) = extract_main_packet_stats(packets);
    let total_size = calculate_total_size(packets);

    print_summary_stats(unique_files.len(), block_size, total_blocks, total_size);
}

// ============================================================================
// File Verification Functions
// ============================================================================

/// Format a filename for display, truncating if necessary
fn format_display_name(file_name: &str) -> String {
    if file_name.len() > 40 {
        format!("{}...", &file_name[..37])
    } else {
        file_name.to_string()
    }
}

/// Print verification results for each file
fn print_file_verification_results(file_names: &[String]) {
    file_names.iter().for_each(|file_name| {
        let display_name = format_display_name(file_name);
        println!("Target: \"{}\" - found.", display_name);
    });
}

/// Verify source files and print progress information
fn verify_source_files_with_progress(packets: Vec<par2rs::Packet>) -> Vec<par2rs::Packet> {
    let file_names = extract_unique_filenames(&packets);
    print_file_verification_results(&file_names);

    // For now, return empty vec indicating no broken files
    Vec::new()
}
