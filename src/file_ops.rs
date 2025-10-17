//! File discovery and PAR2 file operations
//!
//! This module provides functionality for discovering PAR2 files,
//! loading packets from multiple files, and handling deduplication.

use crate::Packet;
use crate::repair::Md5Hash;
use rustc_hash::FxHashSet as HashSet;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

/// Find all PAR2 files in a directory, excluding the specified file
pub fn find_par2_files_in_directory(folder_path: &Path, exclude_file: &Path) -> Vec<PathBuf> {
    match fs::read_dir(folder_path) {
        Ok(entries) => entries
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                (path.extension().is_some_and(|ext| ext == "par2") && path != exclude_file)
                    .then_some(path)
            })
            .collect(),
        Err(_) => {
            eprintln!(
                "Warning: Failed to read directory: {}",
                folder_path.display()
            );
            Vec::new()
        }
    }
}

/// Collect all PAR2 files related to the input file (main file + volume files)
pub fn collect_par2_files(file_path: &Path) -> Vec<PathBuf> {
    let mut par2_files = vec![file_path.to_path_buf()];

    // Get the directory containing the PAR2 file
    let folder_path = if file_path.is_absolute() {
        // For absolute paths, use the parent directory
        file_path.parent().unwrap_or(Path::new("."))
    } else {
        // For relative paths, get the parent or use current directory
        match file_path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        }
    };

    let additional_files = find_par2_files_in_directory(folder_path, file_path);
    par2_files.extend(additional_files);

    // Sort files to match system par2verify order
    par2_files.sort();
    par2_files
}

/// Get a unique hash for a packet to detect duplicates
pub fn get_packet_hash(packet: &Packet) -> Md5Hash {
    match packet {
        Packet::Main(p) => p.md5,
        Packet::FileDescription(p) => p.md5,
        Packet::InputFileSliceChecksum(p) => p.md5,
        Packet::RecoverySlice(p) => p.md5,
        Packet::Creator(p) => p.md5,
        Packet::PackedMain(p) => p.md5,
    }
}

/// Parse a single PAR2 file and return new packets (with deduplication)
pub fn parse_par2_file(
    par2_file: &Path,
    seen_packet_hashes: &mut HashSet<Md5Hash>,
) -> Vec<Packet> {
    let file = fs::File::open(par2_file).expect("Failed to open .par2 file");
    // Use 1MB buffer - recovery slices can be 100KB+ each
    let mut buffered = BufReader::with_capacity(1024 * 1024, file);
    let all_packets = crate::parse_packets(&mut buffered);

    // Filter out packets we've already seen (based on packet MD5)
    let mut new_packets = Vec::new();
    for packet in all_packets {
        let packet_hash = get_packet_hash(&packet);
        if seen_packet_hashes.insert(packet_hash) {
            new_packets.push(packet);
        }
    }

    new_packets
}

/// Parse a single PAR2 file with progress output
pub fn parse_par2_file_with_progress(
    par2_file: &Path,
    seen_packet_hashes: &mut HashSet<Md5Hash>,
    show_progress: bool,
) -> (Vec<Packet>, usize) {
    let filename = par2_file.file_name().unwrap().to_string_lossy();

    if show_progress {
        println!("Loading \"{}\".", filename);
    }

    let new_packets = parse_par2_file(par2_file, seen_packet_hashes);
    let recovery_blocks = count_recovery_blocks(&new_packets);

    if show_progress {
        print_packet_load_result(&filename, new_packets.len(), recovery_blocks);
    }

    (new_packets, recovery_blocks)
}

/// Count the number of recovery slice packets in a collection of packets
pub fn count_recovery_blocks(packets: &[Packet]) -> usize {
    packets
        .iter()
        .filter(|p| matches!(p, Packet::RecoverySlice(_)))
        .count()
}

/// Print the result of loading packets from a file
fn print_packet_load_result(_filename: &str, packet_count: usize, recovery_blocks: usize) {
    if packet_count == 0 {
        println!("No new packets found");
    } else if recovery_blocks > 0 {
        println!(
            "Loaded {} new packets including {} recovery blocks",
            packet_count, recovery_blocks
        );
    } else {
        println!("Loaded {} new packets", packet_count);
    }
}

/// Load PAR2 packets EXCLUDING recovery slices (for memory-efficient operation)
/// Always use this with parse_recovery_slice_metadata() for lazy loading of recovery data
/// 
/// This prevents loading gigabytes of recovery data into memory.
pub fn load_par2_packets(par2_files: &[PathBuf], show_progress: bool) -> Vec<Packet> {
    let mut all_packets = Vec::new();
    let mut seen_packet_hashes = HashSet::default();

    for par2_file in par2_files {
        let (packets, _) = parse_par2_file_with_progress(par2_file, &mut seen_packet_hashes, show_progress);
        
        // Filter out RecoverySlice packets to save memory
        let non_recovery_packets: Vec<Packet> = packets
            .into_iter()
            .filter(|p| !matches!(p, Packet::RecoverySlice(_)))
            .collect();
        
        all_packets.extend(non_recovery_packets);
    }

    all_packets
}

/// Parse recovery slice metadata from PAR2 files without loading data into memory
/// This is the memory-efficient alternative to loading RecoverySlicePackets
/// Returns Vec<RecoverySliceMetadata> - one per recovery block found
pub fn parse_recovery_slice_metadata(
    par2_files: &[PathBuf],
    show_progress: bool,
) -> Vec<crate::RecoverySliceMetadata> {
    use std::fs::File;
    use std::io::{BufReader, Seek, SeekFrom};
    
    let mut all_metadata = Vec::new();
    let mut seen_recovery_slices: HashSet<(crate::repair::RecoverySetId, u32)> = HashSet::default();
    
    for par2_file in par2_files {
        let file = match File::open(par2_file) {
            Ok(f) => f,
            Err(_) => continue,
        };
        
        let mut reader = BufReader::with_capacity(1024 * 1024, file);
        let mut recovery_count = 0;
        
        // Parse packets and look for recovery slices
        loop {
            // Save position before reading header
            let start_pos = match reader.stream_position() {
                Ok(pos) => pos,
                Err(_) => break, // EOF or error
            };
            
            // Try to read packet header to determine type
            let mut header = [0u8; 64];
            if reader.read_exact(&mut header).is_err() {
                break; // EOF
            }
            
            // Check if this is a PAR2 packet
            if &header[0..8] != b"PAR2\0PKT" {
                break; // Not a valid packet
            }
            
            // Get packet type
            let type_bytes: [u8; 16] = match header[48..64].try_into() {
                Ok(bytes) => bytes,
                Err(_) => break,
            };
            
            // Check if this is a recovery slice packet
            if &type_bytes == crate::packets::recovery_slice_packet::TYPE_OF_PACKET {
                // Rewind to start of packet
                if reader.seek(SeekFrom::Start(start_pos)).is_err() {
                    break;
                }
                
                // Parse metadata without loading data
                match crate::RecoverySliceMetadata::parse_from_reader(&mut reader, par2_file.clone()) {
                    Ok(metadata) => {
                        // Deduplicate using (set_id, exponent) pair
                        let dedup_key = (metadata.set_id, metadata.exponent);
                        
                        if seen_recovery_slices.insert(dedup_key) {
                            all_metadata.push(metadata);
                            recovery_count += 1;
                        }
                    }
                    Err(_) => break,
                }
            } else {
                // Not a recovery slice - skip to next packet
                // Get packet length
                let length = u64::from_le_bytes(match header[8..16].try_into() {
                    Ok(bytes) => bytes,
                    Err(_) => break,
                });
                
                // Seek to next packet (length includes the entire packet)
                let next_pos = start_pos + length;
                if reader.seek(SeekFrom::Start(next_pos)).is_err() {
                    break;
                }
            }
        }
        
        if show_progress && recovery_count > 0 {
            let filename = par2_file.file_name().unwrap().to_string_lossy();
            println!("Loaded {} recovery block metadata from \"{}\"", recovery_count, filename);
        }
    }
    
    all_metadata
}
