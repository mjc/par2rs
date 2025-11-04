//! File Operations Module
//!
//! This module provides functionality for discovering and parsing PAR2 files.
//! It includes utilities for finding PAR2 files in a directory and parsing their
//! packet structures from disk with minimal memory overhead.

use crate::domain::{Md5Hash, RecoverySetId};
use crate::Packet;
use rayon::prelude::*;
use rustc_hash::FxHashSet as HashSet;
use std::fs;
use std::io::{BufReader, Read, Seek};
use std::path::{Path, PathBuf};

/// PAR2 packet set with associated metadata
///
/// This struct holds both the parsed packets and important metadata about them,
/// such as the count of validated recovery blocks and base directory.
/// This avoids having to pass recovery block counts and paths as separate parameters.
#[derive(Debug)]
pub struct PacketSet {
    /// The parsed PAR2 packets (may or may not include recovery slice data)
    pub packets: Vec<Packet>,
    /// Number of validated recovery blocks available
    /// (counted even if recovery slice data is not kept in memory)
    pub recovery_block_count: usize,
    /// Base directory for resolving relative file paths in the PAR2 set
    pub base_dir: PathBuf,
}

impl PacketSet {
    /// Create a new packet set with the given packets, recovery block count, and base directory
    pub fn new(packets: Vec<Packet>, recovery_block_count: usize, base_dir: PathBuf) -> Self {
        Self {
            packets,
            recovery_block_count,
            base_dir,
        }
    }

    /// Create a packet set by counting recovery blocks from the packets
    /// Uses current directory as base_dir
    pub fn from_packets(packets: Vec<Packet>) -> Self {
        let recovery_block_count = packets
            .iter()
            .filter(|p| matches!(p, Packet::RecoverySlice(_)))
            .count();
        Self {
            packets,
            recovery_block_count,
            base_dir: PathBuf::from("."),
        }
    }

    /// Create a packet set with a specific base directory
    pub fn from_packets_with_base_dir(packets: Vec<Packet>, base_dir: PathBuf) -> Self {
        let recovery_block_count = packets
            .iter()
            .filter(|p| matches!(p, Packet::RecoverySlice(_)))
            .count();
        Self {
            packets,
            recovery_block_count,
            base_dir,
        }
    }
}

/// Type alias for I/O results in this module
type IoResult<T> = std::io::Result<T>;

/// Buffer size for reading PAR2 files (1MB - recovery slices can be 100KB+ each)
const BUFFER_SIZE: usize = 1024 * 1024;

/// PAR2 packet header size in bytes
const PACKET_HEADER_SIZE: usize = 64;

/// PAR2 packet magic bytes
const PAR2_MAGIC: &[u8; 8] = b"PAR2\0PKT";

/// Offset of magic bytes in packet header
const MAGIC_OFFSET: usize = 0;
const MAGIC_END: usize = 8;

/// Offset of packet length in header
const LENGTH_OFFSET: usize = 8;
const LENGTH_END: usize = 16;

/// Offset of packet type in header
const TYPE_OFFSET: usize = 48;
const TYPE_END: usize = 64;

/// Find all PAR2 files in a directory, excluding the specified file
#[must_use]
pub fn find_par2_files_in_directory(folder_path: &Path, exclude_file: &Path) -> Vec<PathBuf> {
    match fs::read_dir(folder_path) {
        Ok(entries) => entries
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                (path.extension().is_some_and(|ext| ext == "par2") && path != exclude_file)
                    .then_some(path)
            })
            .collect(),
        Err(e) => {
            eprintln!(
                "Warning: Failed to read directory {}: {}",
                folder_path.display(),
                e
            );
            Vec::new()
        }
    }
}

/// Collect all PAR2 files related to the input file (main file + volume files)
#[must_use]
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
#[must_use]
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
) -> IoResult<Vec<Packet>> {
    parse_par2_file_impl(par2_file, seen_packet_hashes, false)
}

/// Internal implementation with recovery slice inclusion control
fn parse_par2_file_impl(
    par2_file: &Path,
    seen_packet_hashes: &mut HashSet<Md5Hash>,
    include_recovery_slices: bool,
) -> IoResult<Vec<Packet>> {
    let file = fs::File::open(par2_file)?;
    // Use 1MB buffer - recovery slices can be 100KB+ each
    let mut buffered = BufReader::with_capacity(BUFFER_SIZE, file);
    let all_packets =
        crate::packets::parse_packets_with_options(&mut buffered, include_recovery_slices);

    // Filter out packets we've already seen (based on packet MD5)
    let new_packets = all_packets
        .into_iter()
        .filter_map(|packet| {
            let packet_hash = get_packet_hash(&packet);
            seen_packet_hashes.insert(packet_hash).then_some(packet)
        })
        .collect();

    Ok(new_packets)
}

/// Parse a single PAR2 file with progress output
pub fn parse_par2_file_with_progress(
    par2_file: &Path,
    seen_packet_hashes: &mut HashSet<Md5Hash>,
    include_recovery_slices: bool,
) -> IoResult<(Vec<Packet>, usize)> {
    let filename = par2_file
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_else(|| "unknown".into());

    if include_recovery_slices {
        println!("Loading \"{}\".", filename);
    }

    let new_packets = parse_par2_file_impl(par2_file, seen_packet_hashes, include_recovery_slices)?;
    let recovery_blocks = crate::packets::processing::count_recovery_blocks(&new_packets);

    if include_recovery_slices {
        print_packet_load_result(new_packets.len(), recovery_blocks);
    }

    Ok((new_packets, recovery_blocks))
}

/// Print the result of loading packets from a file
fn print_packet_load_result(packet_count: usize, recovery_blocks: usize) {
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
#[must_use]
/// Load PAR2 packets with optional recovery slice validation
///
/// When `include_recovery_slices` is false:
/// - Validates all recovery slices (checks they can be parsed)
/// - Counts valid recovery blocks
/// - Does NOT keep recovery slice data in memory (for efficiency)
/// - Returns PacketSet with packets_without_recovery and recovery_block_count
///
/// When `include_recovery_slices` is true:
/// - Includes recovery slices in the returned packet list
/// - Returns PacketSet with all_packets and recovery_block_count
pub fn load_par2_packets(par2_files: &[PathBuf], include_recovery_slices: bool) -> PacketSet {
    let mut seen_packet_hashes = HashSet::default();
    let mut recovery_block_count = 0usize;

    // Parse files in parallel
    let all_packets: Vec<Vec<Packet>> = par2_files
        .par_iter()
        .filter_map(|par2_file| {
            let mut local_seen = HashSet::default();
            match parse_par2_file_with_progress(par2_file, &mut local_seen, include_recovery_slices)
            {
                Ok((packets, _)) => Some(packets),
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to parse PAR2 file {}: {}",
                        par2_file.display(),
                        e
                    );
                    None
                }
            }
        })
        .collect();

    // Deduplicate and filter in a single pass
    let packets: Vec<Packet> = all_packets
        .into_iter()
        .flatten()
        .filter(|p| {
            // Count recovery slices before filtering
            if matches!(p, Packet::RecoverySlice(_)) {
                recovery_block_count += 1;
                return include_recovery_slices;
            }
            // Deduplicate based on packet hash
            let packet_hash = get_packet_hash(p);
            seen_packet_hashes.insert(packet_hash)
        })
        .collect();

    // Determine base directory from the first PAR2 file
    let base_dir = par2_files
        .first()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    PacketSet::new(packets, recovery_block_count, base_dir)
}

/// Load all PAR2 packets INCLUDING recovery slices (in parallel)
/// This is a convenience wrapper around load_par2_packets(files, true)
#[must_use]
pub fn load_all_par2_packets(par2_files: &[PathBuf]) -> PacketSet {
    load_par2_packets(par2_files, true)
}

/// Parse recovery slice metadata from PAR2 files without loading data into memory
/// This is the memory-efficient alternative to loading RecoverySlicePackets
/// Returns Vec<RecoverySliceMetadata> - one per recovery block found
#[must_use]
pub fn parse_recovery_slice_metadata(
    par2_files: &[PathBuf],
    show_progress: bool,
) -> Vec<crate::RecoverySliceMetadata> {
    let mut seen_recovery_slices: HashSet<(RecoverySetId, u32)> = HashSet::default();

    // Parse files in parallel
    let all_metadata: Vec<Vec<crate::RecoverySliceMetadata>> = par2_files
        .par_iter()
        .filter_map(|par2_file| {
            parse_recovery_metadata_from_file(par2_file, show_progress)
                .ok()
                .or_else(|| {
                    eprintln!("Warning: Failed to parse PAR2 file {}", par2_file.display());
                    None
                })
        })
        .collect();

    // Deduplicate recovery slices
    all_metadata
        .into_iter()
        .flatten()
        .filter_map(|metadata| {
            let dedup_key = (metadata.set_id, metadata.exponent);
            seen_recovery_slices.insert(dedup_key).then_some(metadata)
        })
        .collect()
}

/// Parse recovery slice metadata from a single PAR2 file
fn parse_recovery_metadata_from_file(
    par2_file: &Path,
    show_progress: bool,
) -> IoResult<Vec<crate::RecoverySliceMetadata>> {
    use std::fs::File;
    use std::io::BufReader;

    if show_progress {
        let filename = par2_file
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_else(|| "unknown".into());
        println!("Loading \"{}\".", filename);
    }

    let file = File::open(par2_file)?;
    let mut reader = BufReader::with_capacity(BUFFER_SIZE, file);

    let metadata_list: Vec<_> =
        std::iter::from_fn(|| parse_next_recovery_metadata(&mut reader, par2_file).transpose())
            .collect::<IoResult<Vec<_>>>()?;

    if show_progress && !metadata_list.is_empty() {
        let filename = par2_file
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_else(|| "unknown".into());
        println!(
            "Loaded {} recovery block metadata from \"{}\"",
            metadata_list.len(),
            filename
        );
    }

    Ok(metadata_list)
}

/// Parse the next recovery slice metadata from a reader, returning None at EOF
fn parse_next_recovery_metadata<R: Read + Seek>(
    reader: &mut R,
    par2_file: &Path,
) -> IoResult<Option<crate::RecoverySliceMetadata>> {
    use std::io::{ErrorKind, SeekFrom};

    // Save position before reading header
    let start_pos = reader.stream_position()?;

    // Try to read packet header to determine type
    let mut header = [0u8; PACKET_HEADER_SIZE];
    if let Err(e) = reader.read_exact(&mut header) {
        return if e.kind() == ErrorKind::UnexpectedEof {
            Ok(None)
        } else {
            Err(e)
        };
    }

    // Check if this is a valid PAR2 packet
    if !is_valid_par2_header(&header) {
        return Ok(None); // Not a valid packet, end of file
    }

    // Get packet type and length
    let type_bytes = get_packet_type(&header)
        .ok_or_else(|| std::io::Error::new(ErrorKind::InvalidData, "Invalid packet type"))?;

    // Check if this is a recovery slice packet
    if is_recovery_slice_packet(&type_bytes) {
        // Rewind to start of packet
        reader.seek(SeekFrom::Start(start_pos))?;

        // Parse metadata without loading data
        crate::RecoverySliceMetadata::parse_from_reader(reader, par2_file.to_path_buf())
            .map(Some)
            .map_err(|_| {
                std::io::Error::new(ErrorKind::InvalidData, "Failed to parse recovery metadata")
            })
    } else {
        // Not a recovery slice - skip to next packet
        let length = get_packet_length(&header)
            .ok_or_else(|| std::io::Error::new(ErrorKind::InvalidData, "Invalid packet length"))?;

        // Seek to next packet (length includes the entire packet)
        reader.seek(SeekFrom::Start(start_pos + length))?;

        // Tail recursion to try next packet
        parse_next_recovery_metadata(reader, par2_file)
    }
}

/// Helper function to check if a header is a valid PAR2 packet header
#[inline]
fn is_valid_par2_header(header: &[u8; PACKET_HEADER_SIZE]) -> bool {
    &header[MAGIC_OFFSET..MAGIC_END] == PAR2_MAGIC
}

/// Helper function to check if packet type is a recovery slice
#[inline]
fn is_recovery_slice_packet(type_bytes: &[u8; 16]) -> bool {
    type_bytes == crate::packets::recovery_slice_packet::TYPE_OF_PACKET
}

/// Helper function to extract packet type from header
#[inline]
fn get_packet_type(header: &[u8; PACKET_HEADER_SIZE]) -> Option<[u8; 16]> {
    header[TYPE_OFFSET..TYPE_END].try_into().ok()
}

/// Helper function to get packet length from header
#[inline]
fn get_packet_length(header: &[u8; PACKET_HEADER_SIZE]) -> Option<u64> {
    header[LENGTH_OFFSET..LENGTH_END]
        .try_into()
        .ok()
        .map(u64::from_le_bytes)
}
