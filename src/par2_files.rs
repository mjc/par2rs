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
use std::sync::Mutex;

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

/// Recovery file format identified from a user-supplied recovery path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryFormat {
    /// PAR1 recovery set (`.par`, `.PAR`, `.pNN`, `.PNN`)
    Par1,
    /// PAR2 recovery set (`.par2`, `.PAR2`)
    Par2,
}

/// Extract the base stem of a PAR2 filename, stripping `.par2` and any `.volN+M` suffix.
///
/// Examples:
/// - `test.par2`         → `test`
/// - `test.vol0+1.par2`  → `test`
/// - `test.vol000+02.par2` → `test`
fn par2_base_stem(path: &Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let without_ext = if name
        .get(name.len().saturating_sub(5)..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".par2"))
    {
        &name[..name.len() - 5]
    } else {
        name
    };
    // Strip `.vol<digits>+<digits>` suffix if present
    if let Some(dot_pos) = without_ext.rfind('.') {
        let after_dot = &without_ext[dot_pos + 1..];
        if let Some(rest) = after_dot
            .get(..3)
            .filter(|prefix| prefix.eq_ignore_ascii_case("vol"))
            .and_then(|_| after_dot.get(3..))
        {
            if let Some(plus_pos) = rest.find('+') {
                let before = &rest[..plus_pos];
                let after = &rest[plus_pos + 1..];
                if !before.is_empty()
                    && before.bytes().all(|b| b.is_ascii_digit())
                    && !after.is_empty()
                    && after.bytes().all(|b| b.is_ascii_digit())
                {
                    return without_ext[..dot_pos].to_string();
                }
            }
        }
    }
    without_ext.to_string()
}

fn has_par2_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("par2"))
}

fn has_par1_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ext.eq_ignore_ascii_case("par")
                || (ext.len() == 3
                    && ext.as_bytes()[0].eq_ignore_ascii_case(&b'p')
                    && ext.as_bytes()[1].is_ascii_digit()
                    && ext.as_bytes()[2].is_ascii_digit())
        })
}

/// Detect whether a recovery path names a PAR1 or PAR2 set.
#[must_use]
pub fn detect_recovery_format(path: &Path) -> Option<RecoveryFormat> {
    if has_par2_extension(path) {
        Some(RecoveryFormat::Par2)
    } else if has_par1_extension(path) {
        Some(RecoveryFormat::Par1)
    } else {
        None
    }
}

fn append_path_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut candidate = path.as_os_str().to_os_string();
    candidate.push(suffix);
    PathBuf::from(candidate)
}

fn par1_base_stem(path: &Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if let Some(dot_pos) = name.rfind('.') {
        let ext = &name[dot_pos + 1..];
        if ext.eq_ignore_ascii_case("par")
            || (ext.len() == 3
                && ext.as_bytes()[0].eq_ignore_ascii_case(&b'p')
                && ext.as_bytes()[1].is_ascii_digit()
                && ext.as_bytes()[2].is_ascii_digit())
        {
            return name[..dot_pos].to_string();
        }
    }
    name.to_string()
}

/// Resolve a command-line PAR2 argument to an existing `.par2` file.
///
/// par2cmdline accepts a protected data filename or basename when the matching
/// `<name>.par2` file exists. Preserve that behavior for verify/repair
/// frontends while still accepting explicit `.par2` and `.PAR2` paths.
#[must_use]
pub fn resolve_par2_file_argument(input: &Path) -> Option<PathBuf> {
    if has_par2_extension(input) && input.exists() {
        return Some(input.to_path_buf());
    }

    [".par2", ".PAR2"]
        .into_iter()
        .map(|suffix| append_path_suffix(input, suffix))
        .find(|candidate| candidate.exists())
}

/// Find all PAR2 files in a directory, excluding the specified file
#[must_use]
pub fn find_par2_files_in_directory(folder_path: &Path, exclude_file: &Path) -> Vec<PathBuf> {
    let exclude_path = if exclude_file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .is_some()
    {
        exclude_file.to_path_buf()
    } else {
        folder_path.join(exclude_file)
    };

    fs::read_dir(folder_path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| has_par2_extension(path) && path != &exclude_path)
                .collect()
        })
        .unwrap_or_else(|e| {
            eprintln!(
                "Warning: Failed to read directory {}: {}",
                folder_path.display(),
                e
            );
            Vec::new()
        })
}

/// Collect all PAR2 files related to the input file (main file + volume files)
///
/// Only returns files that share the same base stem as `file_path`, preventing
/// accidental mixing of different PAR2 sets in the same directory.
#[must_use]
pub fn collect_par2_files(file_path: &Path) -> Vec<PathBuf> {
    let folder_path = file_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));

    let base_stem = par2_base_stem(file_path);

    let mut related_files: Vec<PathBuf> = find_par2_files_in_directory(folder_path, file_path)
        .into_iter()
        .filter(|p| par2_base_stem(p) == base_stem)
        .collect();
    related_files.sort();

    let mut par2_files = vec![file_path.to_path_buf()];
    par2_files.extend(related_files);
    sort_dedup_preserving_first(&mut par2_files);
    par2_files
}

/// Collect all PAR1 files related to the input file (`.par` and `.pNN`).
#[must_use]
pub fn collect_par1_files(file_path: &Path) -> Vec<PathBuf> {
    let folder_path = file_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));

    let base_stem = par1_base_stem(file_path);
    let mut par1_files = Vec::new();

    if file_path.exists() {
        par1_files.push(file_path.to_path_buf());
    }

    if let Ok(entries) = fs::read_dir(folder_path) {
        par1_files.extend(
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| has_par1_extension(path) && par1_base_stem(path) == base_stem),
        );
    }

    par1_files.sort();
    par1_files.dedup();
    par1_files
}

/// Sort and deduplicate paths while keeping the first path in front.
pub fn sort_dedup_preserving_first(paths: &mut Vec<PathBuf>) {
    let Some(first) = paths.first().cloned() else {
        return;
    };

    let mut rest = paths[1..].to_vec();
    rest.sort();
    rest.dedup();

    paths.clear();
    paths.push(first.clone());
    paths.extend(rest.into_iter().filter(|path| path != &first));
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
    let (all_packets, _recovery_count) =
        crate::packets::parse_packets_with_options(&mut buffered, include_recovery_slices);

    // Filter out packets we've already seen (based on packet MD5)
    Ok(all_packets
        .into_iter()
        .filter(|packet| {
            let packet_hash = get_packet_hash(packet);
            seen_packet_hashes.insert(packet_hash)
        })
        .collect())
}

/// Parse result containing packets and metadata
#[derive(Debug)]
struct ParseResult {
    packets: Vec<Packet>,
    recovery_block_count: usize,
}

/// Parse a single PAR2 file with optional progress output
fn parse_single_file(
    par2_file: &Path,
    include_recovery_slices: bool,
    show_progress: bool,
    output_lock: &Mutex<()>,
) -> IoResult<ParseResult> {
    let filename = par2_file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    if show_progress {
        let _guard = output_lock.lock().unwrap();
        println!("Loading \"{}\".", filename);
    }

    // Parse without deduplication - that happens at the global level
    let file = fs::File::open(par2_file)?;
    let mut buffered = BufReader::with_capacity(BUFFER_SIZE, file);
    let (packets, recovery_block_count) =
        crate::packets::parse_packets_with_options(&mut buffered, include_recovery_slices);

    let result = ParseResult {
        packets,
        recovery_block_count,
    };

    if show_progress {
        print_packet_load_result(
            result.packets.len(),
            result.recovery_block_count,
            output_lock,
        );
    }

    Ok(result)
}

/// Legacy wrapper for compatibility - prefer parse_single_file
pub fn parse_par2_file_with_progress(
    par2_file: &Path,
    seen_packet_hashes: &mut HashSet<Md5Hash>,
    include_recovery_slices: bool,
    show_progress: bool,
) -> IoResult<(Vec<Packet>, usize)> {
    let output_lock = Mutex::new(());
    let result = parse_single_file(
        par2_file,
        include_recovery_slices,
        show_progress,
        &output_lock,
    )?;

    // Apply deduplication using the provided set
    let new_packets: Vec<Packet> = result
        .packets
        .into_iter()
        .filter(|p| {
            let packet_hash = get_packet_hash(p);
            seen_packet_hashes.insert(packet_hash)
        })
        .collect();

    let recovery_blocks = crate::packets::processing::count_recovery_blocks(&new_packets);
    Ok((new_packets, recovery_blocks))
}

/// Print the result of loading packets from a file (thread-safe)
fn print_packet_load_result(packet_count: usize, recovery_blocks: usize, lock: &Mutex<()>) {
    let _guard = lock.lock().unwrap();
    match (packet_count, recovery_blocks) {
        (0, _) => println!("No new packets found"),
        (count, 0) => println!("Loaded {count} new packets"),
        (count, blocks) => {
            println!("Loaded {count} new packets including {blocks} recovery blocks")
        }
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
pub fn load_par2_packets(
    par2_files: &[PathBuf],
    include_recovery_slices: bool,
    show_progress: bool,
) -> PacketSet {
    if show_progress {
        return load_par2_packets_with_progress(par2_files, include_recovery_slices);
    }

    // Parse files in parallel and collect results
    // Use mutex for thread-safe output (like par2cmdline-turbo's output_lock)
    let output_lock = Mutex::new(());

    let all_packets: Vec<Vec<Packet>> = par2_files
        .par_iter()
        .filter_map(|par2_file| {
            parse_single_file(
                par2_file,
                include_recovery_slices,
                show_progress,
                &output_lock,
            )
            .map(|result| result.packets)
            .map_err(|e| {
                let _guard = output_lock.lock().unwrap();
                eprintln!(
                    "Warning: Failed to parse PAR2 file {}: {}",
                    par2_file.display(),
                    e
                );
                e
            })
            .ok()
        })
        .collect();

    let primary_set_id = all_packets
        .iter()
        .flatten()
        .next()
        .map(packet_recovery_set_id);

    // Deduplicate packets in a single pass, keeping only the primary recovery
    // set. par2cmdline-turbo treats packets from explicit foreign PAR2 files as
    // "no new packets" rather than merging recovery sets.
    let mut seen_hashes = HashSet::default();

    let packets: Vec<Packet> = all_packets
        .into_iter()
        .flatten()
        .filter(|packet| {
            // Skip recovery slices if not including them (already counted above)
            if !include_recovery_slices && matches!(packet, Packet::RecoverySlice(_)) {
                return false;
            }

            if primary_set_id.is_some_and(|set_id| packet_recovery_set_id(packet) != set_id) {
                return false;
            }

            // Deduplicate based on packet hash
            let packet_hash = get_packet_hash(packet);
            seen_hashes.insert(packet_hash)
        })
        .collect();

    let recovery_block_count = if let Some(set_id) = primary_set_id {
        if include_recovery_slices {
            packets
                .iter()
                .filter(|packet| matches!(packet, Packet::RecoverySlice(_)))
                .count()
        } else {
            parse_recovery_slice_metadata(par2_files, false)
                .into_iter()
                .filter(|metadata| metadata.set_id == set_id)
                .count()
        }
    } else {
        0
    };

    // Determine base directory from the first PAR2 file
    let base_dir = par2_files
        .first()
        .and_then(|p| p.parent())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| PathBuf::from("."));

    PacketSet::new(packets, recovery_block_count, base_dir)
}

fn load_par2_packets_with_progress(
    par2_files: &[PathBuf],
    include_recovery_slices: bool,
) -> PacketSet {
    let output_lock = Mutex::new(());
    let mut primary_set_id = None;
    let mut seen_hashes = HashSet::default();
    let mut packets = Vec::new();

    for par2_file in par2_files {
        let filename = par2_file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown");
        {
            let _guard = output_lock.lock().unwrap();
            println!("Loading \"{}\".", filename);
        }

        let result =
            match parse_single_file(par2_file, include_recovery_slices, false, &output_lock) {
                Ok(result) => result,
                Err(e) => {
                    let _guard = output_lock.lock().unwrap();
                    eprintln!(
                        "Warning: Failed to parse PAR2 file {}: {}",
                        par2_file.display(),
                        e
                    );
                    continue;
                }
            };

        let file_set_id = result.packets.first().map(packet_recovery_set_id);
        if primary_set_id.is_none() {
            primary_set_id = file_set_id;
        }

        let mut new_packets = Vec::new();
        for packet in result.packets {
            if !include_recovery_slices && matches!(packet, Packet::RecoverySlice(_)) {
                continue;
            }

            if primary_set_id.is_some_and(|set_id| packet_recovery_set_id(&packet) != set_id) {
                continue;
            }

            let packet_hash = get_packet_hash(&packet);
            if seen_hashes.insert(packet_hash) {
                new_packets.push(packet);
            }
        }

        let recovery_blocks = if include_recovery_slices {
            new_packets
                .iter()
                .filter(|packet| matches!(packet, Packet::RecoverySlice(_)))
                .count()
        } else if file_set_id == primary_set_id {
            result.recovery_block_count
        } else {
            0
        };

        let loaded_packet_count = new_packets.len()
            + if include_recovery_slices {
                0
            } else {
                recovery_blocks
            };
        print_packet_load_result(loaded_packet_count, recovery_blocks, &output_lock);

        packets.extend(new_packets.into_iter().filter(|packet| {
            include_recovery_slices || !matches!(packet, Packet::RecoverySlice(_))
        }));
    }

    let recovery_block_count = if let Some(set_id) = primary_set_id {
        if include_recovery_slices {
            packets
                .iter()
                .filter(|packet| matches!(packet, Packet::RecoverySlice(_)))
                .count()
        } else {
            parse_recovery_slice_metadata(par2_files, false)
                .into_iter()
                .filter(|metadata| metadata.set_id == set_id)
                .count()
        }
    } else {
        0
    };

    let base_dir = par2_files
        .first()
        .and_then(|p| p.parent())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| PathBuf::from("."));

    PacketSet::new(packets, recovery_block_count, base_dir)
}

fn packet_recovery_set_id(packet: &Packet) -> RecoverySetId {
    match packet {
        Packet::Main(p) => p.set_id,
        Packet::PackedMain(p) => p.set_id,
        Packet::FileDescription(p) => p.set_id,
        Packet::InputFileSliceChecksum(p) => p.set_id,
        Packet::RecoverySlice(p) => p.set_id,
        Packet::Creator(p) => p.set_id,
    }
}

/// Load all PAR2 packets INCLUDING recovery slices (in parallel)
/// This is a convenience wrapper around load_par2_packets(files, true, true)
#[must_use]
pub fn load_all_par2_packets(par2_files: &[PathBuf]) -> PacketSet {
    load_par2_packets(par2_files, true, true)
}

/// Parse recovery slice metadata from PAR2 files without loading data into memory
/// This is the memory-efficient alternative to loading RecoverySlicePackets
/// Returns Vec<RecoverySliceMetadata> - one per recovery block found
#[must_use]
pub fn parse_recovery_slice_metadata(
    par2_files: &[PathBuf],
    show_progress: bool,
) -> Vec<crate::RecoverySliceMetadata> {
    // Parse files in parallel
    let all_metadata: Vec<Vec<crate::RecoverySliceMetadata>> = par2_files
        .par_iter()
        .filter_map(|par2_file| {
            parse_recovery_metadata_from_file(par2_file, show_progress)
                .map_err(|e| {
                    eprintln!(
                        "Warning: Failed to parse PAR2 file {}: {}",
                        par2_file.display(),
                        e
                    );
                    e
                })
                .ok()
        })
        .collect();

    // Deduplicate recovery slices based on (set_id, exponent)
    let mut seen_recovery_slices = HashSet::default();
    all_metadata
        .into_iter()
        .flatten()
        .filter(|metadata| {
            let dedup_key = (metadata.set_id, metadata.exponent);
            seen_recovery_slices.insert(dedup_key)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn par2_base_stem_strips_par2_extension() {
        assert_eq!(par2_base_stem(Path::new("test.par2")), "test");
        assert_eq!(par2_base_stem(Path::new("test.PAR2")), "test");
    }

    #[test]
    fn par2_base_stem_strips_vol_suffix() {
        assert_eq!(par2_base_stem(Path::new("test.vol0+1.par2")), "test");
        assert_eq!(par2_base_stem(Path::new("test.vol000+02.par2")), "test");
        assert_eq!(par2_base_stem(Path::new("test.vol06+4.par2")), "test");
        assert_eq!(par2_base_stem(Path::new("test.VOL06+4.PAR2")), "test");
    }

    #[test]
    fn par2_base_stem_handles_dots_in_base() {
        assert_eq!(par2_base_stem(Path::new("my.file.par2")), "my.file");
        assert_eq!(par2_base_stem(Path::new("my.file.vol0+1.par2")), "my.file");
    }

    #[test]
    fn par1_base_stem_strips_par_and_volume_extensions() {
        assert_eq!(par1_base_stem(Path::new("test.par")), "test");
        assert_eq!(par1_base_stem(Path::new("test.PAR")), "test");
        assert_eq!(par1_base_stem(Path::new("test.p01")), "test");
        assert_eq!(par1_base_stem(Path::new("test.P01")), "test");
        assert_eq!(par1_base_stem(Path::new("my.file.p99")), "my.file");
    }

    #[test]
    fn detect_recovery_format_accepts_par2_extensions() {
        assert_eq!(
            detect_recovery_format(Path::new("archive.par2")),
            Some(RecoveryFormat::Par2)
        );
        assert_eq!(
            detect_recovery_format(Path::new("archive.PAR2")),
            Some(RecoveryFormat::Par2)
        );
    }

    #[test]
    fn detect_recovery_format_accepts_par1_extensions() {
        assert_eq!(
            detect_recovery_format(Path::new("archive.par")),
            Some(RecoveryFormat::Par1)
        );
        assert_eq!(
            detect_recovery_format(Path::new("archive.PAR")),
            Some(RecoveryFormat::Par1)
        );
        assert_eq!(
            detect_recovery_format(Path::new("archive.p01")),
            Some(RecoveryFormat::Par1)
        );
        assert_eq!(
            detect_recovery_format(Path::new("archive.P01")),
            Some(RecoveryFormat::Par1)
        );
    }

    #[test]
    fn detect_recovery_format_rejects_non_recovery_extensions() {
        assert_eq!(detect_recovery_format(Path::new("archive.par3")), None);
        assert_eq!(detect_recovery_format(Path::new("archive.p1")), None);
        assert_eq!(detect_recovery_format(Path::new("archive.p001")), None);
        assert_eq!(detect_recovery_format(Path::new("archive.dat")), None);
    }

    #[test]
    fn collect_par2_files_excludes_different_base_stem() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();

        // Create dummy .par2 files
        std::fs::write(dir.join("file1.par2"), b"").unwrap();
        std::fs::write(dir.join("file1.vol0+1.par2"), b"").unwrap();
        std::fs::write(dir.join("file1.vol1+1.PAR2"), b"").unwrap();
        std::fs::write(dir.join("file2.par2"), b"").unwrap();
        std::fs::write(dir.join("file2.vol0+1.par2"), b"").unwrap();

        let collected = collect_par2_files(&dir.join("file1.par2"));

        // Should only contain file1's files
        assert!(
            collected.iter().all(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.starts_with("file1"))
                    .unwrap_or(false)
            }),
            "Should not include file2's PAR2 files: {:?}",
            collected
        );
        assert_eq!(collected.len(), 3, "Expected file1 PAR2 set files");
    }

    #[test]
    fn collect_par1_files_finds_main_and_volumes_for_volume_input() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();

        std::fs::write(dir.join("file1.par"), b"").unwrap();
        std::fs::write(dir.join("file1.p01"), b"").unwrap();
        std::fs::write(dir.join("file1.P02"), b"").unwrap();
        std::fs::write(dir.join("file2.par"), b"").unwrap();
        std::fs::write(dir.join("file2.p01"), b"").unwrap();

        let collected = collect_par1_files(&dir.join("file1.P02"));

        assert_eq!(
            collected,
            vec![
                dir.join("file1.P02"),
                dir.join("file1.p01"),
                dir.join("file1.par"),
            ]
        );
    }

    #[test]
    fn resolve_par2_file_argument_accepts_explicit_par2_path() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("file.PAR2");
        std::fs::write(&path, b"").unwrap();

        assert_eq!(resolve_par2_file_argument(&path), Some(path));
    }

    #[test]
    fn resolve_par2_file_argument_accepts_data_filename_companion() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("file.dat");
        let par2 = temp.path().join("file.dat.par2");
        std::fs::write(&source, b"").unwrap();
        std::fs::write(&par2, b"").unwrap();

        assert_eq!(resolve_par2_file_argument(&source), Some(par2));
    }

    #[test]
    fn resolve_par2_file_argument_accepts_uppercase_companion() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("file.dat");
        let par2 = temp.path().join("file.dat.PAR2");
        std::fs::write(&par2, b"").unwrap();

        assert_eq!(resolve_par2_file_argument(&source), Some(par2));
    }

    #[test]
    fn find_par2_files_excludes_bare_filename_in_folder() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        std::fs::write(dir.join("file1.par2"), b"").unwrap();
        std::fs::write(dir.join("file1.vol0+1.par2"), b"").unwrap();

        let found = find_par2_files_in_directory(dir, Path::new("file1.par2"));

        assert_eq!(found, vec![dir.join("file1.vol0+1.par2")]);
    }
}
