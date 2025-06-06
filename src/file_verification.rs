//! File verification utilities
//!
//! This module provides functionality for verifying individual files
//! against their expected MD5 hashes.

use crate::Packet;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

/// Format a filename for display, truncating if necessary
pub fn format_display_name(file_name: &str) -> String {
    Path::new(file_name)
        .file_name()
        .and_then(|name| name.to_str())
        .map_or_else(
            || file_name.to_string(),
            |name| {
                if name.len() > 50 {
                    format!("{}...", &name[..47])
                } else {
                    name.to_string()
                }
            },
        )
}

/// Calculate MD5 hash of a file
pub fn calculate_file_md5(file_path: &Path) -> Result<[u8; 16], std::io::Error> {
    let mut file = fs::File::open(file_path)?;
    let mut hasher = md5::Context::new();
    let mut buffer = [0; 8192]; // 8KB buffer for reading

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.consume(&buffer[..bytes_read]);
    }

    Ok(hasher.compute().0)
}

/// Verify a single file by comparing its MD5 hash with the expected value
pub fn verify_single_file(file_name: &str, expected_md5: [u8; 16]) -> bool {
    verify_single_file_with_base_dir(file_name, expected_md5, None)
}

/// Verify a single file with optional base directory for path resolution
pub fn verify_single_file_with_base_dir(
    file_name: &str,
    expected_md5: [u8; 16],
    base_dir: Option<&Path>,
) -> bool {
    let file_path = if let Some(base) = base_dir {
        base.join(file_name)
    } else {
        Path::new(file_name).to_path_buf()
    };

    // Check if file exists
    if !file_path.exists() {
        return false;
    }

    // Calculate actual MD5 hash
    match calculate_file_md5(&file_path) {
        Ok(actual_md5) => actual_md5 == expected_md5,
        Err(_) => false,
    }
}

/// File verification result
#[derive(Debug, Clone)]
pub struct FileVerificationResult {
    pub file_name: String,
    pub file_id: [u8; 16],
    pub expected_md5: [u8; 16],
    pub is_valid: bool,
    pub exists: bool,
}

/// Verify files and collect results
pub fn verify_files_and_collect_results(
    file_info: &HashMap<String, ([u8; 16], [u8; 16], u64)>,
    show_progress: bool,
) -> Vec<FileVerificationResult> {
    verify_files_and_collect_results_with_base_dir(file_info, show_progress, None)
}

/// Verify files and collect results with optional base directory for path resolution
pub fn verify_files_and_collect_results_with_base_dir(
    file_info: &HashMap<String, ([u8; 16], [u8; 16], u64)>,
    show_progress: bool,
    base_dir: Option<&Path>,
) -> Vec<FileVerificationResult> {
    let mut results = Vec::new();

    for (file_name, &(file_id, expected_md5, _file_length)) in file_info {
        if show_progress {
            let truncated_name = format_display_name(file_name);
            println!("Opening: \"{}\"", truncated_name);
        }

        let file_path = if let Some(base) = base_dir {
            base.join(file_name)
        } else {
            Path::new(file_name).to_path_buf()
        };

        let exists = file_path.exists();
        let is_valid = if exists {
            verify_single_file_with_base_dir(file_name, expected_md5, base_dir)
        } else {
            false
        };

        if show_progress {
            if is_valid {
                println!("Target: \"{}\" - found.", file_name);
            } else if exists {
                println!("Target: \"{}\" - damaged.", file_name);
            } else {
                println!("Target: \"{}\" - missing.", file_name);
            }
        }

        results.push(FileVerificationResult {
            file_name: file_name.clone(),
            file_id,
            expected_md5,
            is_valid,
            exists,
        });
    }

    results
}

/// Find FileDescription packets for files that failed verification
pub fn find_broken_file_descriptors(
    packets: Vec<Packet>,
    broken_file_ids: &[[u8; 16]],
) -> Vec<Packet> {
    packets
        .into_iter()
        .filter(|packet| {
            if let Packet::FileDescription(fd) = packet {
                broken_file_ids.contains(&fd.file_id)
            } else {
                false
            }
        })
        .collect()
}
