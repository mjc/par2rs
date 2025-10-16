//! PAR2 analysis and statistics
//!
//! This module provides functionality for analyzing PAR2 packets,
//! extracting metadata, and calculating statistics.

use crate::Packet;
use crate::repair::{FileId, Md5Hash};
use std::collections::HashMap;

/// Extract unique filenames from FileDescription packets
pub fn extract_unique_filenames(packets: &[Packet]) -> Vec<String> {
    packets
        .iter()
        .filter_map(|packet| match packet {
            Packet::FileDescription(fd) => std::str::from_utf8(&fd.file_name)
                .ok()
                .map(|s| s.trim_end_matches('\0').to_string()),
            _ => None,
        })
        .collect::<std::collections::HashSet<_>>() // Remove duplicates
        .into_iter()
        .collect()
}

/// Extract block size and total blocks from packets
pub fn extract_main_packet_stats(packets: &[Packet]) -> (u32, usize) {
    // Get block size from main packet
    let block_size = packets
        .iter()
        .find_map(|packet| match packet {
            Packet::Main(main_packet) => Some(main_packet.slice_size as u32),
            _ => None,
        })
        .unwrap_or(0);

    // Calculate total blocks from unique files only
    let total_blocks = if block_size > 0 {
        let mut unique_files = HashMap::new();

        // Collect unique FileDescription packets by file_id
        for packet in packets {
            if let Packet::FileDescription(fd) = packet {
                unique_files.insert(fd.file_id, fd.file_length);
            }
        }

        // Sum blocks for all unique files
        unique_files
            .values()
            .map(|&file_length| {
                // Calculate blocks needed for this file (round up)
                file_length.div_ceil(block_size as u64)
            })
            .sum()
    } else {
        0
    };

    (block_size, total_blocks as usize)
}

/// Calculate total size based on unique files only
pub fn calculate_total_size(packets: &[Packet]) -> u64 {
    let mut unique_files = HashMap::new();

    // Collect unique FileDescription packets by file_id to avoid counting duplicates
    for packet in packets {
        if let Packet::FileDescription(fd) = packet {
            unique_files.insert(fd.file_id, fd.file_length);
        }
    }

    // Sum up the file sizes for unique files only
    unique_files.values().sum()
}

/// Collect file information from FileDescription packets
/// Returns: HashMap<filename, (file_id, md5_hash, file_length)>
pub fn collect_file_info_from_packets(
    packets: &[Packet],
) -> HashMap<String, (FileId, Md5Hash, u64)> {
    let mut file_info = HashMap::new();

    for packet in packets {
        if let Packet::FileDescription(fd) = packet {
            if let Ok(file_name) = std::str::from_utf8(&fd.file_name) {
                let clean_name = file_name.trim_end_matches('\0').to_string();
                file_info.insert(clean_name, (fd.file_id, fd.md5_hash, fd.file_length));
            }
        }
    }

    file_info
}

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
    let unique_files = extract_unique_filenames(packets);
    let (block_size, total_blocks) = extract_main_packet_stats(packets);
    let total_size = calculate_total_size(packets);

    Par2Stats {
        file_count: unique_files.len(),
        block_size,
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
