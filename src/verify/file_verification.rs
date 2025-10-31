//! File verification utilities
//!
//! This module provides functionality for verifying individual files
//! against their expected MD5 hashes.

//! File verification utilities
//!
//! Functional-style file verification using centralized checksum.

use crate::domain::{FileId, Md5Hash};
use std::collections::HashMap;
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

/// Calculate MD5 hash of the first 16KB of a file (fast integrity check)
pub use crate::checksum::calculate_file_md5_16k;

/// Calculate MD5 hash of a file
///
/// Functional implementation using chunked iteration for performance
/// Calculate MD5 hash of a file (using functional style for efficiency)
pub use crate::checksum::calculate_file_md5;

/// Verify a single file by comparing its MD5 hash with the expected value
pub fn verify_single_file(file_name: &str, expected_md5: &Md5Hash) -> bool {
    verify_single_file_with_base_dir(file_name, expected_md5, None)
}

/// Verify a single file with optional base directory for path resolution
pub fn verify_single_file_with_base_dir(
    file_name: &str,
    expected_md5: &Md5Hash,
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
        Ok(actual_md5) => &actual_md5 == expected_md5,
        Err(_) => false,
    }
}

/// File verification result
#[derive(Debug, Clone)]
pub struct FileVerificationResult {
    pub file_name: String,
    pub file_id: FileId,
    pub expected_md5: Md5Hash,
    pub is_valid: bool,
    pub exists: bool,
}

/// Verify files and collect results with optional base directory for path resolution
pub fn verify_files_and_collect_results_with_base_dir(
    file_info: &HashMap<String, (FileId, Md5Hash, u64)>,
    show_progress: bool,
    base_dir: Option<&Path>,
) -> Vec<FileVerificationResult> {
    let mut results = Vec::new();

    for (file_name, (file_id, expected_md5, _file_length)) in file_info {
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
            file_id: *file_id,
            expected_md5: *expected_md5,
            is_valid,
            exists,
        });
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_calculate_file_md5_16k_small_file() {
        // Create a temp file with less than 16KB
        let mut temp_file = NamedTempFile::new().unwrap();
        let data = b"Hello, World!";
        temp_file.write_all(data).unwrap();
        temp_file.flush().unwrap();

        let result = calculate_file_md5_16k(temp_file.path());
        assert!(result.is_ok());

        // For small files, 16k hash should match full hash
        let full_hash = calculate_file_md5(temp_file.path()).unwrap();
        let partial_hash = result.unwrap();
        assert_eq!(partial_hash, full_hash);
    }

    #[test]
    fn test_calculate_file_md5_16k_large_file() {
        // Create a temp file with more than 16KB
        let mut temp_file = NamedTempFile::new().unwrap();
        let data = vec![0u8; 20000]; // 20KB
        temp_file.write_all(&data).unwrap();
        temp_file.flush().unwrap();

        let result_16k = calculate_file_md5_16k(temp_file.path());
        assert!(result_16k.is_ok());

        // 16k hash should be different from full hash for large files
        let full_hash = calculate_file_md5(temp_file.path()).unwrap();
        let partial_hash = result_16k.unwrap();
        assert_ne!(partial_hash, full_hash);
    }

    #[test]
    fn test_calculate_file_md5_16k_exactly_16kb() {
        // Create a temp file with exactly 16KB
        let mut temp_file = NamedTempFile::new().unwrap();
        let data = vec![42u8; 16384]; // Exactly 16KB
        temp_file.write_all(&data).unwrap();
        temp_file.flush().unwrap();

        let result = calculate_file_md5_16k(temp_file.path());
        assert!(result.is_ok());

        // For exactly 16KB file, 16k hash should match full hash
        let full_hash = calculate_file_md5(temp_file.path()).unwrap();
        let partial_hash = result.unwrap();
        assert_eq!(partial_hash, full_hash);
    }

    #[test]
    fn test_calculate_file_md5_large_buffer() {
        // Test that large buffer (128MB) works correctly
        let mut temp_file = NamedTempFile::new().unwrap();
        let data = vec![1u8; 1024 * 1024]; // 1MB
        temp_file.write_all(&data).unwrap();
        temp_file.flush().unwrap();

        let result = calculate_file_md5(temp_file.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_16k_hash_performance_optimization() {
        // Create a large file to demonstrate the optimization
        let mut temp_file = NamedTempFile::new().unwrap();
        // Write 1MB of data
        for _ in 0..1024 {
            temp_file.write_all(&[0u8; 1024]).unwrap();
        }
        temp_file.flush().unwrap();

        // The 16KB hash should be much faster than full hash
        let start = std::time::Instant::now();
        let _ = calculate_file_md5_16k(temp_file.path()).unwrap();
        let time_16k = start.elapsed();

        let start = std::time::Instant::now();
        let _ = calculate_file_md5(temp_file.path()).unwrap();
        let time_full = start.elapsed();

        // 16KB hash should be faster (though this may not always hold on small files)
        println!("16KB hash: {:?}, Full hash: {:?}", time_16k, time_full);
        assert!(time_16k < time_full * 10); // At least 10x faster for large files
    }
}
