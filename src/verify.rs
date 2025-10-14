
use crate::Packet;
use std::collections::HashMap;
use std::convert::TryInto;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// File verification status
#[derive(Debug, Clone)]
pub enum FileStatus {
    Complete, // File is perfect match
    Renamed,  // File exists but has wrong name
    Damaged,  // File exists but is damaged
    Missing,  // File is completely missing
}

/// Block verification result
#[derive(Debug, Clone)]
pub struct BlockVerificationResult {
    pub block_number: u32,
    pub file_id: [u8; 16],
    pub is_valid: bool,
    pub expected_hash: Option<[u8; 16]>,
    pub expected_crc: Option<u32>,
}

/// Comprehensive verification results
#[derive(Debug, Clone)]
pub struct VerificationResults {
    pub files: Vec<FileVerificationResult>,
    pub blocks: Vec<BlockVerificationResult>,
    pub complete_file_count: usize,
    pub renamed_file_count: usize,
    pub damaged_file_count: usize,
    pub missing_file_count: usize,
    pub available_block_count: usize,
    pub missing_block_count: usize,
    pub total_block_count: usize,
    pub recovery_blocks_available: usize,
    pub repair_possible: bool,
    pub blocks_needed_for_repair: usize,
}

/// Individual file verification result  
#[derive(Debug, Clone)]
pub struct FileVerificationResult {
    pub file_name: String,
    pub file_id: [u8; 16],
    pub status: FileStatus,
    pub blocks_available: usize,
    pub total_blocks: usize,
    pub damaged_blocks: Vec<u32>,
}

/// Verifies par2 packets.
/// This function reads the packets from the provided vector and verifies that they are usable
///
/// # Arguments
/// /// * `packets` - A vector of packets parsed from the PAR2 files.
///
/// # Returns
/// /// * `packets` - A vector of packets that are usable.
///
/// # Output
/// Prints failed verification messages to stderr if any packet fails verification.
// pub fn verify_par2_packets(packets: Vec<crate::Packet>) -> Vec<crate::Packet> {
//     packets.into_iter().filter_map(|packet| {
//         match packet {
//             Packet::PackedMainPacket(packed_main_packet) => {
//                 // TODO: Implement MD5 verification for PackedMainPacket if needed
//                 Some(packet)
//             }
//             _ => Some(packet), // Other packets are assumed valid for now
//         }
//     }).collect()
// }

/// Quickly verifies a set of files from the par2 md5sums
///
/// # Arguments
///
/// * `packets` - A list of packets parsed from the PAR2 files.
///
/// # Returns
///
/// A boolean indicating whether the verification was successful.
pub fn quick_check_files(packets: Vec<crate::Packet>) -> Vec<crate::Packet> {
    println!("Starting quick check of files...");

    // Collect file names from the packets
    let file_names: Vec<String> = packets
        .iter()
        .filter_map(|packet| {
            if let Packet::FileDescription(desc) = packet {
                Some(String::from_utf8_lossy(&desc.file_name).to_string())
            } else {
                None
            }
        })
        .collect();
    println!("Found file names: {:?}", file_names);

    // If no file names were found, return an empty list
    if file_names.is_empty() {
        println!("No file names found, nothing to verify.");
        return vec![];
    }

    // Quick Check all files
    // Return a list of FileDescription packets that failed the check
    packets
        .into_iter()
        .filter_map(|packet| {
            if let Packet::FileDescription(desc) = &packet {
                let file_name = String::from_utf8_lossy(&desc.file_name).to_string();
                match verify_file_md5(desc) {
                    Some(_) => None,
                    None => {
                        eprintln!("Failed to verify file: {}", file_name);
                        Some(packet)
                    }
                }
            } else {
                None
            }
        })
        .collect()
}

/// Helper function to compute MD5 checksum of a file
fn compute_md5(
    file_name: &str,
    directory: Option<&str>,
    length: Option<usize>,
) -> Result<[u8; 16], String> {
    let file_path = match directory {
        Some(dir) => Path::new(dir)
            .join(file_name.trim_end_matches(char::from(0)))
            .to_string_lossy()
            .to_string(),
        None => {
            let cwd = std::env::current_dir()
                .map_err(|_| "Failed to get current working directory".to_string())?;
            cwd.join(file_name.trim_end_matches(char::from(0)))
                .to_string_lossy()
                .to_string()
        }
    };

    use md5::{Digest, Md5};
    let file = File::open(&file_path).map_err(|_| format!("Failed to open file: {}", file_path))?;
    let mut reader = std::io::BufReader::new(file);
    let mut hasher = Md5::new();
    let mut buffer = vec![0u8; 256 * 1024 * 1024]; // 256MB buffer size

    let mut total_read = 0;
    loop {
        let bytes_to_read = match length {
            Some(len) if total_read + buffer.len() > len => len - total_read,
            _ => buffer.len(),
        };

        let bytes_read = reader
            .read(&mut buffer[..bytes_to_read])
            .map_err(|_| format!("Failed to read file: {}", file_path))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
        total_read += bytes_read;

        if let Some(len) = length {
            if total_read >= len {
                break;
            }
        }
    }

    let file_md5: [u8; 16] = hasher.finalize().into();
    file_md5
        .as_slice()
        .try_into()
        .map_err(|_| "MD5 hash should be 16 bytes".to_string())
}

/// Helper function to verify MD5 checksum
fn verify_md5(
    file_name: &str,
    directory: Option<&str>,
    length: Option<usize>,
    expected_md5: &[u8; 16],
    description: &str,
) -> Result<(), String> {
    let computed_md5 = compute_md5(file_name, directory, length)?;
    if &computed_md5 != expected_md5 {
        return Err(format!(
            "MD5 mismatch for {} {}: expected {:?}, got {:?}",
            description, file_name, expected_md5, computed_md5
        ));
    }
    Ok(())
}

pub fn verify_file_md5(desc: &crate::packets::FileDescriptionPacket) -> Option<String> {
    let file_name = String::from_utf8_lossy(&desc.file_name).to_string();
    let file_path = file_name.trim_end_matches(char::from(0)).to_string();

    // Verify the MD5 of the first 16 KB of the file
    if let Err(err) = verify_md5(
        &file_path,
        None,
        Some(16 * 1024),
        &desc.md5_16k,
        "first 16 KB of file",
    ) {
        eprintln!("{}", err);
        return None;
    }
    println!(
        "Verified first 16 KB of file: {}",
        file_name.trim_end_matches(char::from(0))
    );

    // Verify the MD5 of the entire file
    if let Err(err) = verify_md5(&file_path, None, None, &desc.md5_hash, "entire file") {
        eprintln!("{}", err);
        return None;
    }
    println!(
        "Verified entire file: {}",
        file_name.trim_end_matches(char::from(0))
    );

    Some(file_name)
}

/// Comprehensive verification function based on par2cmdline approach
///
/// This function performs detailed verification similar to par2cmdline:
/// 1. Verifies files at the whole-file level using MD5 hashes
/// 2. For damaged files, performs block-level verification using slice checksums
/// 3. Reports which blocks are broken and calculates repair requirements
/// 4. Determines if repair is possible with available recovery blocks
pub fn comprehensive_verify_files(packets: Vec<crate::Packet>) -> VerificationResults {
    println!("Starting comprehensive verification...");

    let mut results = VerificationResults {
        files: Vec::new(),
        blocks: Vec::new(),
        complete_file_count: 0,
        renamed_file_count: 0,
        damaged_file_count: 0,
        missing_file_count: 0,
        available_block_count: 0,
        missing_block_count: 0,
        total_block_count: 0,
        recovery_blocks_available: 0,
        repair_possible: false,
        blocks_needed_for_repair: 0,
    };

    // Extract main packet information
    let main_packet = packets.iter().find_map(|p| {
        if let Packet::Main(main) = p {
            Some(main)
        } else {
            None
        }
    });

    let block_size = main_packet.map(|m| m.slice_size).unwrap_or(0);

    // Count recovery blocks available
    results.recovery_blocks_available = packets
        .iter()
        .filter_map(|p| {
            if let Packet::RecoverySlice(_) = p {
                Some(1)
            } else {
                None
            }
        })
        .sum();

    // Collect file descriptions
    let file_descriptions: Vec<_> = packets
        .iter()
        .filter_map(|p| {
            if let Packet::FileDescription(fd) = p {
                Some(fd)
            } else {
                None
            }
        })
        .collect();

    // Collect slice checksum packets indexed by file ID
    let slice_checksums: HashMap<[u8; 16], Vec<([u8; 16], u32)>> = packets
        .iter()
        .filter_map(|p| {
            if let Packet::InputFileSliceChecksum(ifsc) = p {
                Some((ifsc.file_id, ifsc.slice_checksums.clone()))
            } else {
                None
            }
        })
        .collect();

    println!("Found {} files to verify", file_descriptions.len());

    // Verify each file
    for file_desc in file_descriptions {
        let file_name = String::from_utf8_lossy(&file_desc.file_name)
            .trim_end_matches('\0')
            .to_string();

        println!("Verifying: \"{}\"", file_name);

        let mut file_result = FileVerificationResult {
            file_name: file_name.clone(),
            file_id: file_desc.file_id,
            status: FileStatus::Missing,
            blocks_available: 0,
            total_blocks: 0,
            damaged_blocks: Vec::new(),
        };

        // Calculate total blocks for this file
        if block_size > 0 {
            file_result.total_blocks = file_desc.file_length.div_ceil(block_size) as usize;
            results.total_block_count += file_result.total_blocks;
        }

        // Check if file exists
        let file_path = Path::new(&file_name);
        if !file_path.exists() {
            println!("Target: \"{}\" - missing.", file_name);
            file_result.status = FileStatus::Missing;
            results.missing_file_count += 1;

            // All blocks are missing for this file
            for block_num in 0..file_result.total_blocks {
                results.blocks.push(BlockVerificationResult {
                    block_number: block_num as u32,
                    file_id: file_desc.file_id,
                    is_valid: false,
                    expected_hash: None,
                    expected_crc: None,
                });
            }
            results.missing_block_count += file_result.total_blocks;
        } else {
            // File exists, verify its integrity
            match verify_file_integrity(file_desc, &file_name) {
                Ok(true) => {
                    println!("Target: \"{}\" - found.", file_name);
                    file_result.status = FileStatus::Complete;
                    file_result.blocks_available = file_result.total_blocks;
                    results.complete_file_count += 1;
                    results.available_block_count += file_result.total_blocks;

                    // Mark all blocks as valid
                    for block_num in 0..file_result.total_blocks {
                        results.blocks.push(BlockVerificationResult {
                            block_number: block_num as u32,
                            file_id: file_desc.file_id,
                            is_valid: true,
                            expected_hash: None,
                            expected_crc: None,
                        });
                    }
                }
                Ok(false) | Err(_) => {
                    println!("Target: \"{}\" - damaged.", file_name);
                    file_result.status = FileStatus::Damaged;
                    results.damaged_file_count += 1;

                    // Perform block-level verification if we have slice checksums
                    if let Some(checksums) = slice_checksums.get(&file_desc.file_id) {
                        let (available_blocks, damaged_block_numbers) =
                            verify_blocks_in_file(&file_name, checksums, block_size as usize);

                        file_result.blocks_available = available_blocks;
                        file_result.damaged_blocks = damaged_block_numbers.clone();
                        results.available_block_count += available_blocks;

                        // Create block verification results
                        for (block_num, (expected_hash, expected_crc)) in
                            checksums.iter().enumerate()
                        {
                            let is_valid = !damaged_block_numbers.contains(&(block_num as u32));

                            results.blocks.push(BlockVerificationResult {
                                block_number: block_num as u32,
                                file_id: file_desc.file_id,
                                is_valid,
                                expected_hash: Some(*expected_hash),
                                expected_crc: Some(*expected_crc),
                            });
                        }

                        results.missing_block_count += damaged_block_numbers.len();

                        if !damaged_block_numbers.is_empty() {
                            println!(
                                "  {} of {} blocks are damaged",
                                damaged_block_numbers.len(),
                                checksums.len()
                            );
                        }
                    } else {
                        // No block-level checksums available, assume all blocks are damaged
                        results.missing_block_count += file_result.total_blocks;

                        for block_num in 0..file_result.total_blocks {
                            file_result.damaged_blocks.push(block_num as u32);
                            results.blocks.push(BlockVerificationResult {
                                block_number: block_num as u32,
                                file_id: file_desc.file_id,
                                is_valid: false,
                                expected_hash: None,
                                expected_crc: None,
                            });
                        }
                    }
                }
            }
        }

        results.files.push(file_result);
    }

    // Calculate repair requirements
    results.blocks_needed_for_repair = results.missing_block_count;
    results.repair_possible = results.recovery_blocks_available >= results.missing_block_count;

    results
}

/// Verify integrity of a single file using MD5 hashes
fn verify_file_integrity(
    desc: &crate::packets::FileDescriptionPacket,
    file_path: &str,
) -> Result<bool, String> {
    // Verify the MD5 of the first 16 KB of the file
    if verify_md5(
        file_path,
        None,
        Some(16 * 1024),
        &desc.md5_16k,
        "first 16 KB of file",
    )
    .is_err()
    {
        return Ok(false);
    }

    // Verify the MD5 of the entire file
    if verify_md5(file_path, None, None, &desc.md5_hash, "entire file").is_err() {
        return Ok(false);
    }

    Ok(true)
}

/// Verify individual blocks within a file using slice checksums
fn verify_blocks_in_file(
    file_path: &str,
    slice_checksums: &[([u8; 16], u32)],
    block_size: usize,
) -> (usize, Vec<u32>) {
    let mut available_blocks = 0;
    let mut damaged_blocks = Vec::new();

    let mut file = match File::open(file_path) {
        Ok(f) => f,
        Err(_) => return (0, (0..slice_checksums.len() as u32).collect()),
    };

    // Get file size to handle the last block correctly
    let file_size = match file.metadata() {
        Ok(metadata) => metadata.len() as usize,
        Err(_) => return (0, (0..slice_checksums.len() as u32).collect()),
    };

    let mut buffer = vec![0u8; block_size];

    for (block_index, (expected_md5, expected_crc)) in slice_checksums.iter().enumerate() {
        let block_offset = block_index * block_size;

        // Calculate how many bytes we should read for this block
        let bytes_to_read = if block_offset + block_size <= file_size {
            block_size
        } else if block_offset < file_size {
            file_size - block_offset
        } else {
            // Block is beyond file size
            damaged_blocks.push(block_index as u32);
            continue;
        };

        // Seek to the correct position for this block
        if file.seek(SeekFrom::Start(block_offset as u64)).is_err() {
            damaged_blocks.push(block_index as u32);
            continue;
        }

        // Read exactly the amount we need for this block
        buffer.resize(bytes_to_read, 0);
        let mut total_read = 0;
        while total_read < bytes_to_read {
            match file.read(&mut buffer[total_read..bytes_to_read]) {
                Ok(0) => break, // EOF
                Ok(n) => total_read += n,
                Err(_) => {
                    damaged_blocks.push(block_index as u32);
                    continue;
                }
            }
        }

        if total_read != bytes_to_read {
            damaged_blocks.push(block_index as u32);
            continue;
        }

        // Compute MD5 of the block
        use md5::Digest;
        let block_md5: [u8; 16] = md5::Md5::digest(&buffer[..bytes_to_read]).into();

        // Compute CRC32 of the block
        let block_crc = crc32fast::hash(&buffer[..bytes_to_read]);

        // Check if block is valid
        if block_md5 == *expected_md5 && block_crc == *expected_crc {
            available_blocks += 1;
        } else {
            damaged_blocks.push(block_index as u32);
        }
    }

    (available_blocks, damaged_blocks)
}

/// Print verification results in par2cmdline style
pub fn print_verification_results(results: &VerificationResults) {
    println!("\nVerification Results:");
    println!("====================");

    // Print file summary
    if results.complete_file_count > 0 {
        println!("{} file(s) are ok.", results.complete_file_count);
    }
    if results.renamed_file_count > 0 {
        println!(
            "{} file(s) have the wrong name.",
            results.renamed_file_count
        );
    }
    if results.damaged_file_count > 0 {
        println!(
            "{} file(s) exist but are damaged.",
            results.damaged_file_count
        );
    }
    if results.missing_file_count > 0 {
        println!("{} file(s) are missing.", results.missing_file_count);
    }

    // Print block summary
    println!(
        "You have {} out of {} data blocks available.",
        results.available_block_count, results.total_block_count
    );

    if results.recovery_blocks_available > 0 {
        println!(
            "You have {} recovery blocks available.",
            results.recovery_blocks_available
        );
    }

    // Print repair status
    if results.missing_block_count == 0 {
        println!("All files are correct, repair is not required.");
    } else if results.repair_possible {
        println!("Repair is possible.");

        if results.recovery_blocks_available > results.missing_block_count {
            println!(
                "You have an excess of {} recovery blocks.",
                results.recovery_blocks_available - results.missing_block_count
            );
        }

        println!(
            "{} recovery blocks will be used to repair.",
            results.missing_block_count
        );
    } else {
        println!("Repair is not possible.");
        println!(
            "You need {} more recovery blocks to be able to repair.",
            results.missing_block_count - results.recovery_blocks_available
        );
    }

    // Print detailed block information for damaged files
    for file_result in &results.files {
        if !file_result.damaged_blocks.is_empty() {
            println!("\nDamaged blocks in \"{}\":", file_result.file_name);
            if file_result.damaged_blocks.len() <= 20 {
                // Show all blocks if there are 20 or fewer
                for &block_num in &file_result.damaged_blocks {
                    println!("  Block {}: damaged", block_num);
                }
            } else {
                // Show first 10 and last 10 blocks if there are many
                for &block_num in &file_result.damaged_blocks[..10] {
                    println!("  Block {}: damaged", block_num);
                }
                println!(
                    "  ... {} more damaged blocks ...",
                    file_result.damaged_blocks.len() - 20
                );
                for &block_num in
                    &file_result.damaged_blocks[file_result.damaged_blocks.len() - 10..]
                {
                    println!("  Block {}: damaged", block_num);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packets::main_packet::MainPacket;
    use crate::Packet;

    #[test]
    fn test_quick_check_files() {
        // Create mock packets for testing
        let mock_packets = vec![Packet::Main(MainPacket {
            length: 0,
            md5: [0; 16],
            set_id: [0; 16],
            slice_size: 0,
            file_count: 0,
            file_ids: vec![],
            non_recovery_file_ids: vec![],
        })];

        let result = quick_check_files(mock_packets);
        assert!(
            result.is_empty(),
            "Verification should succeed for mock packets"
        );
    }
}
