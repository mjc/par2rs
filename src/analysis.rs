//! PAR2 analysis and statistics
//!
//! This module provides functionality for analyzing PAR2 packets,
//! extracting metadata, and calculating statistics.

use crate::Packet;

/// PAR2 statistics structure
#[derive(Debug, Clone)]
pub struct Par2Stats {
    pub file_count: usize,
    pub block_size: u32,
    pub total_blocks: usize,
    pub total_size: u64,
    pub recovery_blocks: usize,
}

/// Calculate comprehensive statistics for a PAR2 set
pub fn calculate_par2_stats(packets: &[Packet], recovery_blocks: usize) -> Par2Stats {
    let unique_files = crate::packets::processing::extract_filenames(packets);
    let (block_size, total_blocks) = crate::packets::processing::extract_main_stats(packets);
    let total_size = crate::packets::processing::extract_file_descriptions(packets)
        .into_iter()
        .map(|fd| fd.file_length)
        .sum();

    Par2Stats {
        file_count: unique_files.len(),
        block_size: block_size as u32,
        total_blocks,
        total_size,
        recovery_blocks,
    }
}

/// Print summary statistics about the PAR2 set
pub fn print_summary_stats(stats: &Par2Stats) {
    println!(
        "\nThere are {} recoverable files and 0 other files.",
        stats.file_count
    );
    println!("The block size used was {} bytes.", stats.block_size);
    println!("There are a total of {} data blocks.", stats.total_blocks);
    println!(
        "The total size of the data files is {} bytes.",
        stats.total_size
    );
}
