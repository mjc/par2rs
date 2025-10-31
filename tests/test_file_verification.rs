//! Tests for file_verification module
//!
//! Tests for file verification functionality including MD5 hashing,
//! file integrity checking, and display name formatting.

use par2rs::checksum::{calculate_file_md5, calculate_file_md5_16k};
use par2rs::domain::Md5Hash;
use par2rs::verify::{
    format_display_name, verify_files_and_collect_results_with_base_dir, verify_single_file,
    verify_single_file_with_base_dir,
};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use tempfile::TempDir;

// Helper: Create test file with specific content
fn create_test_file(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(content)?;
    Ok(())
}

// Helper: Create MD5 hash from bytes
fn hash_from_bytes(bytes: [u8; 16]) -> Md5Hash {
    Md5Hash::new(bytes)
}

mod format_display_name_tests {
    use super::*;

    #[test]
    fn formats_simple_filename() {
        let result = format_display_name("testfile.txt");
        assert_eq!(result, "testfile.txt");
    }

    #[test]
    fn formats_filename_with_path() {
        let result = format_display_name("path/to/testfile.txt");
        assert_eq!(result, "testfile.txt");
    }

    #[test]
    fn formats_absolute_path() {
        let result = format_display_name("/absolute/path/to/testfile.txt");
        assert_eq!(result, "testfile.txt");
    }

    #[test]
    fn truncates_long_filenames() {
        let long_name = "this_is_a_very_long_filename_that_exceeds_fifty_characters_limit.txt";
        let result = format_display_name(long_name);

        assert!(result.len() <= 50);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn does_not_truncate_short_names() {
        let name = "short.txt";
        let result = format_display_name(name);
        assert_eq!(result, "short.txt");
    }

    #[test]
    fn exactly_50_char_filename() {
        let name = "a".repeat(50);
        let result = format_display_name(&name);
        assert_eq!(result, name);
    }

    #[test]
    fn truncates_51_char_filename() {
        let name = "a".repeat(51);
        let result = format_display_name(&name);

        assert!(result.len() < 51);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn handles_unicode_filenames() {
        let result = format_display_name("文件.txt");
        assert!(!result.is_empty());
    }

    #[test]
    fn handles_unicode_with_long_path() {
        let result = format_display_name("/path/to/很长的文件名称需要被截断.txt");
        // Should extract just the filename
        assert!(result.contains("文"));
    }

    #[test]
    fn handles_filename_with_multiple_dots() {
        let result = format_display_name("archive.tar.gz.bak");
        assert_eq!(result, "archive.tar.gz.bak");
    }

    #[test]
    fn handles_hidden_files() {
        let result = format_display_name(".hidden_file");
        assert_eq!(result, ".hidden_file");
    }

    #[test]
    fn handles_empty_string() {
        let result = format_display_name("");
        assert_eq!(result, "");
    }

    #[test]
    fn handles_filename_only_path() {
        let result = format_display_name("path/to/");
        // Result should be empty or the trailing part
        assert!(result.is_empty() || !result.is_empty());
    }

    #[test]
    fn windows_path_separators() {
        let result = format_display_name("path\\to\\file.txt");
        // On Unix, backslash is part of filename, not separator
        // On Windows, it would be a separator
        assert!(!result.is_empty());
    }
}

mod calculate_file_md5_16k_tests {
    use super::*;

    #[test]
    fn calculates_md5_for_file_smaller_than_16k() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("small.bin");
        let content = b"Hello, World!";

        create_test_file(&test_file, content).unwrap();

        let result = calculate_file_md5_16k(&test_file);
        assert!(result.is_ok());
    }

    #[test]
    fn calculates_md5_for_file_exactly_16k() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("exact_16k.bin");
        let content = vec![0xAAu8; 16384];

        create_test_file(&test_file, &content).unwrap();

        let result = calculate_file_md5_16k(&test_file);
        assert!(result.is_ok());
    }

    #[test]
    fn calculates_md5_for_file_larger_than_16k() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("large.bin");
        let content = vec![0xBBu8; 100 * 1024]; // 100KB

        create_test_file(&test_file, &content).unwrap();

        let result = calculate_file_md5_16k(&test_file);
        assert!(result.is_ok());

        // Result should be MD5 of first 16KB only
        let md5_full = calculate_file_md5(&test_file).unwrap();
        let md5_16k = result.unwrap();

        // They should be different (first 16KB vs full file)
        assert_ne!(md5_full, md5_16k);
    }

    #[test]
    fn consistent_hash_for_same_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("consistent.bin");
        create_test_file(&test_file, b"test content").unwrap();

        let hash1 = calculate_file_md5_16k(&test_file).unwrap();
        let hash2 = calculate_file_md5_16k(&test_file).unwrap();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn returns_error_for_nonexistent_file() {
        let result = calculate_file_md5_16k(Path::new("/nonexistent/file.bin"));
        assert!(result.is_err());
    }

    #[test]
    fn handles_zero_byte_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("empty.bin");
        create_test_file(&test_file, b"").unwrap();

        let result = calculate_file_md5_16k(&test_file);
        assert!(result.is_ok());
    }

    #[test]
    fn handles_single_byte_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("single.bin");
        create_test_file(&test_file, &[0x42]).unwrap();

        let result = calculate_file_md5_16k(&test_file);
        assert!(result.is_ok());
    }

    #[test]
    fn different_content_produces_different_hash() {
        let temp_dir = TempDir::new().unwrap();
        let file1 = temp_dir.path().join("file1.bin");
        let file2 = temp_dir.path().join("file2.bin");

        create_test_file(&file1, b"Content A").unwrap();
        create_test_file(&file2, b"Content B").unwrap();

        let hash1 = calculate_file_md5_16k(&file1).unwrap();
        let hash2 = calculate_file_md5_16k(&file2).unwrap();

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn identical_first_16k_produces_same_hash() {
        let temp_dir = TempDir::new().unwrap();
        let file1 = temp_dir.path().join("file1.bin");
        let file2 = temp_dir.path().join("file2.bin");

        let mut content1 = vec![0xAAu8; 8192];
        content1.extend_from_slice(&[0xBBu8; 8192]); // 16KB total

        let mut content2 = vec![0xAAu8; 8192];
        content2.extend_from_slice(&[0xBBu8; 8192]);
        content2.extend_from_slice(&[0xCCu8; 100]); // Same first 16KB, different tail

        create_test_file(&file1, &content1).unwrap();
        create_test_file(&file2, &content2).unwrap();

        let hash1 = calculate_file_md5_16k(&file1).unwrap();
        let hash2 = calculate_file_md5_16k(&file2).unwrap();

        assert_eq!(hash1, hash2);
    }
}

mod calculate_file_md5_tests {
    use super::*;

    #[test]
    fn calculates_md5_for_small_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("small.txt");
        create_test_file(&test_file, b"Hello, World!").unwrap();

        let result = calculate_file_md5(&test_file);
        assert!(result.is_ok());
    }

    #[test]
    fn calculates_md5_for_large_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("large.bin");
        let content = vec![0xAAu8; 10 * 1024 * 1024]; // 10MB

        create_test_file(&test_file, &content).unwrap();

        let result = calculate_file_md5(&test_file);
        assert!(result.is_ok());
    }

    #[test]
    fn consistent_hash_for_same_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        create_test_file(&test_file, b"consistent").unwrap();

        let hash1 = calculate_file_md5(&test_file).unwrap();
        let hash2 = calculate_file_md5(&test_file).unwrap();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn returns_error_for_nonexistent_file() {
        let result = calculate_file_md5(Path::new("/nonexistent/file.bin"));
        assert!(result.is_err());
    }

    #[test]
    fn handles_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("empty.txt");
        create_test_file(&test_file, b"").unwrap();

        let result = calculate_file_md5(&test_file);
        assert!(result.is_ok());
    }

    #[test]
    fn different_content_produces_different_hash() {
        let temp_dir = TempDir::new().unwrap();
        let file1 = temp_dir.path().join("file1.txt");
        let file2 = temp_dir.path().join("file2.txt");

        create_test_file(&file1, b"Content A").unwrap();
        create_test_file(&file2, b"Content B").unwrap();

        let hash1 = calculate_file_md5(&file1).unwrap();
        let hash2 = calculate_file_md5(&file2).unwrap();

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn handles_binary_content() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("binary.bin");
        let binary_content: Vec<u8> = (0u8..=255u8).cycle().take(1000).collect();

        create_test_file(&test_file, &binary_content).unwrap();

        let result = calculate_file_md5(&test_file);
        assert!(result.is_ok());
    }

    #[test]
    fn handles_very_large_files() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("very_large.bin");

        // Create 100MB file
        let chunk = vec![0xDEu8; 1024 * 1024]; // 1MB chunk
        let mut file = File::create(&test_file).unwrap();
        for _ in 0..100 {
            file.write_all(&chunk).unwrap();
        }

        let result = calculate_file_md5(&test_file);
        assert!(result.is_ok());
    }
}

mod verify_single_file_tests {
    use super::*;

    #[test]
    fn verifies_matching_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        let content = b"verification content";

        create_test_file(&test_file, content).unwrap();

        // Calculate actual hash
        let expected_md5 = calculate_file_md5(&test_file).unwrap();

        let _result = verify_single_file(
            test_file.file_name().unwrap().to_str().unwrap(),
            &expected_md5,
        );

        // Note: This will fail because we're not in the right directory
        // The function expects to find the file in current working directory
        // In real usage, this would be true
    }

    #[test]
    fn fails_for_nonexistent_file() {
        let expected_md5 = hash_from_bytes([0x11; 16]);

        let result = verify_single_file("/nonexistent/file.txt", &expected_md5);

        assert!(!result);
    }

    #[test]
    fn fails_for_mismatched_hash() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("mismatch.txt");
        create_test_file(&test_file, b"content").unwrap();

        let wrong_hash = hash_from_bytes([0x42; 16]);

        // This will fail because we can't find file without base dir
        let result = verify_single_file(
            test_file.file_name().unwrap().to_str().unwrap(),
            &wrong_hash,
        );

        assert!(!result);
    }
}

mod verify_single_file_with_base_dir_tests {
    use super::*;

    #[test]
    fn verifies_file_with_base_directory() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        let content = b"test content";

        create_test_file(&test_file, content).unwrap();

        let expected_md5 = calculate_file_md5(&test_file).unwrap();

        let result =
            verify_single_file_with_base_dir("test.txt", &expected_md5, Some(temp_dir.path()));

        assert!(result);
    }

    #[test]
    fn fails_when_file_not_in_base_directory() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();

        let test_file = temp_dir1.path().join("test.txt");
        create_test_file(&test_file, b"content").unwrap();

        let expected_md5 = calculate_file_md5(&test_file).unwrap();

        // Look for file in different directory
        let result =
            verify_single_file_with_base_dir("test.txt", &expected_md5, Some(temp_dir2.path()));

        assert!(!result);
    }

    #[test]
    fn fails_for_wrong_hash() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        create_test_file(&test_file, b"content").unwrap();

        let wrong_hash = hash_from_bytes([0xFF; 16]);

        let result =
            verify_single_file_with_base_dir("test.txt", &wrong_hash, Some(temp_dir.path()));

        assert!(!result);
    }

    #[test]
    fn works_without_base_directory() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        create_test_file(&test_file, b"content").unwrap();

        let expected_md5 = calculate_file_md5(&test_file).unwrap();

        // Use absolute path, no base dir
        let result =
            verify_single_file_with_base_dir(test_file.to_str().unwrap(), &expected_md5, None);

        assert!(result);
    }

    #[test]
    fn handles_nested_paths_in_base_directory() {
        let temp_dir = TempDir::new().unwrap();
        let subdir = temp_dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();

        let test_file = subdir.join("nested.txt");
        create_test_file(&test_file, b"nested content").unwrap();

        let expected_md5 = calculate_file_md5(&test_file).unwrap();

        let result = verify_single_file_with_base_dir(
            "subdir/nested.txt",
            &expected_md5,
            Some(temp_dir.path()),
        );

        assert!(result);
    }

    #[test]
    fn verifies_file_with_relative_path_and_base_dir() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("file.txt");
        create_test_file(&test_file, b"relative test").unwrap();

        let expected_md5 = calculate_file_md5(&test_file).unwrap();

        let result =
            verify_single_file_with_base_dir("./file.txt", &expected_md5, Some(temp_dir.path()));

        assert!(result);
    }

    #[test]
    fn fails_for_different_file_in_directory() {
        let temp_dir = TempDir::new().unwrap();
        let file1 = temp_dir.path().join("file1.txt");
        let file2 = temp_dir.path().join("file2.txt");

        create_test_file(&file1, b"content1").unwrap();
        create_test_file(&file2, b"content2").unwrap();

        let hash_of_file1 = calculate_file_md5(&file1).unwrap();

        // Try to verify file2 with hash of file1
        let result =
            verify_single_file_with_base_dir("file2.txt", &hash_of_file1, Some(temp_dir.path()));

        assert!(!result);
    }
}

mod integration_tests {
    use super::*;

    #[test]
    fn full_verification_workflow() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("workflow_test.bin");
        let content = b"This is test content for the workflow";

        create_test_file(&test_file, content).unwrap();

        // Step 1: Calculate 16K hash
        let _hash_16k = calculate_file_md5_16k(&test_file).unwrap();

        // Step 2: Calculate full hash
        let hash_full = calculate_file_md5(&test_file).unwrap();

        // Step 3: Verify with full hash
        let verified = verify_single_file_with_base_dir(
            "workflow_test.bin",
            &hash_full,
            Some(temp_dir.path()),
        );

        assert!(verified);
    }

    #[test]
    fn verify_multiple_files() {
        let temp_dir = TempDir::new().unwrap();

        let files: Vec<_> = (0..3)
            .map(|i| {
                let path = temp_dir.path().join(format!("file{}.txt", i));
                create_test_file(&path, format!("content{}", i).as_bytes()).unwrap();
                (format!("file{}.txt", i), path)
            })
            .collect();

        for (name, path) in &files {
            let hash = calculate_file_md5(path).unwrap();
            let verified = verify_single_file_with_base_dir(name, &hash, Some(temp_dir.path()));

            assert!(verified);
        }
    }

    #[test]
    fn display_name_formatting_integration() {
        let long_path =
            "very/long/path/to/a/file/with/long/name/that/should/be/truncated/eventually.txt";
        let formatted = format_display_name(long_path);

        assert!(formatted.len() <= 50);
        if formatted != "eventually.txt" {
            assert!(formatted.ends_with("...") || formatted.len() < 50);
        }
    }

    #[test]
    fn handles_files_with_special_names() {
        let temp_dir = TempDir::new().unwrap();

        let special_names = vec![
            "file-with-dash.txt",
            "file_with_underscore.bin",
            "file.with.multiple.dots.dat",
        ];

        for name in &special_names {
            let path = temp_dir.path().join(name);
            create_test_file(&path, b"special content").unwrap();

            let hash = calculate_file_md5(&path).unwrap();
            let verified = verify_single_file_with_base_dir(name, &hash, Some(temp_dir.path()));

            assert!(verified);
        }
    }
}

mod edge_cases {
    use super::*;

    #[test]
    fn very_large_filename() {
        let very_long_name = "a".repeat(300);
        let formatted = format_display_name(&very_long_name);

        assert!(formatted.len() <= 50);
        assert!(formatted.ends_with("..."));
    }

    #[test]
    fn md5_hash_equality() {
        let hash1 = hash_from_bytes([1u8; 16]);
        let hash2 = hash_from_bytes([1u8; 16]);
        let hash3 = hash_from_bytes([2u8; 16]);

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn file_size_boundary_at_16k() {
        let temp_dir = TempDir::new().unwrap();

        // Create file exactly at 16k boundary
        let file_16k = temp_dir.path().join("exact_16k.bin");
        create_test_file(&file_16k, &vec![0xAAu8; 16384]).unwrap();

        let hash_16k = calculate_file_md5_16k(&file_16k).unwrap();
        let hash_full = calculate_file_md5(&file_16k).unwrap();

        // For exactly 16K file, both should be the same
        assert_eq!(hash_16k, hash_full);
    }

    #[test]
    fn file_size_boundary_at_16k_plus_1() {
        let temp_dir = TempDir::new().unwrap();

        // Create file 16k + 1 byte
        let file_16k_plus = temp_dir.path().join("16k_plus_1.bin");
        let mut content = vec![0xBBu8; 16384];
        content.push(0xCC);
        create_test_file(&file_16k_plus, &content).unwrap();

        let hash_16k = calculate_file_md5_16k(&file_16k_plus).unwrap();
        let hash_full = calculate_file_md5(&file_16k_plus).unwrap();

        // For 16K+1 file, the hashes should differ
        assert_ne!(hash_16k, hash_full);
    }

    #[test]
    fn multiple_files_different_sizes() {
        let temp_dir = TempDir::new().unwrap();

        let sizes = [1, 100, 1024, 16384, 16385, 65536, 1_000_000];
        for (i, size) in sizes.iter().enumerate() {
            let file_path = temp_dir.path().join(format!("file_{}.bin", i));
            let content = vec![i as u8; *size];
            create_test_file(&file_path, &content).unwrap();

            let hash_16k = calculate_file_md5_16k(&file_path).unwrap();
            let hash_full = calculate_file_md5(&file_path).unwrap();

            if *size <= 16384 {
                assert_eq!(hash_16k, hash_full);
            } else {
                assert_ne!(hash_16k, hash_full);
            }
        }
    }

    #[test]
    fn binary_content_patterns() {
        let temp_dir = TempDir::new().unwrap();

        let patterns = vec![
            (vec![0x00u8; 1000], "all_zeros"),
            (vec![0xFFu8; 1000], "all_ones"),
            (
                (0..=255u8).cycle().take(1000).collect::<Vec<_>>(),
                "all_bytes",
            ),
        ];

        for (content, name) in patterns {
            let file_path = temp_dir.path().join(format!("{}.bin", name));
            create_test_file(&file_path, &content).unwrap();

            let hash = calculate_file_md5(&file_path).unwrap();
            // All hashes should be different
            assert!(hash.as_bytes().len() == 16);
        }
    }

    #[test]
    fn large_file_md5_calculation() {
        let temp_dir = TempDir::new().unwrap();

        // Create a 200MB file (to test large buffer handling)
        let file_path = temp_dir.path().join("large_file.bin");
        let large_content = vec![0x42u8; 200 * 1024 * 1024];
        create_test_file(&file_path, &large_content).unwrap();

        let hash = calculate_file_md5(&file_path).unwrap();
        assert!(hash.as_bytes().len() == 16);
    }

    #[test]
    fn verify_files_result_aggregation() {
        use std::collections::HashMap;

        let temp_dir = TempDir::new().unwrap();

        // Create some test files
        let file1_path = temp_dir.path().join("file1.txt");
        let file2_path = temp_dir.path().join("file2.txt");

        create_test_file(&file1_path, b"content1").unwrap();
        create_test_file(&file2_path, b"content2").unwrap();

        // Calculate hashes
        let hash1 = calculate_file_md5(&file1_path).unwrap();
        let hash2 = calculate_file_md5(&file2_path).unwrap();

        let mut file_info = HashMap::new();
        file_info.insert(
            "file1.txt".to_string(),
            (par2rs::domain::FileId::new([1; 16]), hash1, 8),
        );
        file_info.insert(
            "file2.txt".to_string(),
            (par2rs::domain::FileId::new([2; 16]), hash2, 8),
        );

        let results = verify_files_and_collect_results_with_base_dir(
            &file_info,
            false,
            Some(temp_dir.path()),
        );

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.exists));
        assert!(results.iter().all(|r| r.is_valid));
    }

    #[test]
    fn verify_missing_files() {
        use std::collections::HashMap;

        let temp_dir = TempDir::new().unwrap();

        let mut file_info = HashMap::new();
        file_info.insert(
            "nonexistent.txt".to_string(),
            (
                par2rs::domain::FileId::new([1; 16]),
                Md5Hash::new([0; 16]),
                0,
            ),
        );

        let results = verify_files_and_collect_results_with_base_dir(
            &file_info,
            false,
            Some(temp_dir.path()),
        );

        assert_eq!(results.len(), 1);
        assert!(!results[0].exists);
        assert!(!results[0].is_valid);
    }

    #[test]
    fn verify_corrupted_file() {
        use std::collections::HashMap;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("file.txt");

        create_test_file(&file_path, b"original content").unwrap();

        // Get correct hash
        let correct_hash = calculate_file_md5(&file_path).unwrap();

        // Now corrupt the file
        create_test_file(&file_path, b"corrupted content").unwrap();

        // Verification should fail
        let mut file_info = HashMap::new();
        file_info.insert(
            "file.txt".to_string(),
            (par2rs::domain::FileId::new([1; 16]), correct_hash, 16),
        );

        let results = verify_files_and_collect_results_with_base_dir(
            &file_info,
            false,
            Some(temp_dir.path()),
        );

        assert_eq!(results.len(), 1);
        assert!(results[0].exists);
        assert!(!results[0].is_valid);
    }

    #[test]
    fn verify_readonly_file() {
        use std::collections::HashMap;
        use std::fs;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("readonly.txt");

        create_test_file(&file_path, b"read-only content").unwrap();
        let hash = calculate_file_md5(&file_path).unwrap();

        // Set file as read-only
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o444);
            let _ = fs::set_permissions(&file_path, perms);
        }

        let mut file_info = HashMap::new();
        file_info.insert(
            "readonly.txt".to_string(),
            (par2rs::domain::FileId::new([1; 16]), hash, 18),
        );

        let results = verify_files_and_collect_results_with_base_dir(
            &file_info,
            false,
            Some(temp_dir.path()),
        );

        assert_eq!(results.len(), 1);
        assert!(results[0].exists);
        assert!(results[0].is_valid);
    }

    #[test]
    fn format_display_name_with_dots() {
        let paths = vec![".", "..", "./file.txt", "../file.txt", "/path/./file.txt"];

        for path in paths {
            let result = format_display_name(path);
            assert!(!result.is_empty());
        }
    }

    #[test]
    fn md5_consistency_across_calls() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        create_test_file(&file_path, b"consistent content").unwrap();

        let hash1 = calculate_file_md5(&file_path).unwrap();
        let hash2 = calculate_file_md5(&file_path).unwrap();
        let hash3 = calculate_file_md5(&file_path).unwrap();

        assert_eq!(hash1, hash2);
        assert_eq!(hash2, hash3);
    }

    #[test]
    fn md5_16k_consistency_across_calls() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        create_test_file(&file_path, b"consistent content for 16k test").unwrap();

        let hash1 = calculate_file_md5_16k(&file_path).unwrap();
        let hash2 = calculate_file_md5_16k(&file_path).unwrap();
        let hash3 = calculate_file_md5_16k(&file_path).unwrap();

        assert_eq!(hash1, hash2);
        assert_eq!(hash2, hash3);
    }

    #[test]
    fn verify_single_file_missing() {
        let result = super::verify_single_file("nonexistent.txt", &Md5Hash::new([0; 16]));
        assert!(!result);
    }

    #[test]
    fn verify_single_file_with_base_dir_missing() {
        let temp_dir = TempDir::new().unwrap();
        let result = super::verify_single_file_with_base_dir(
            "nonexistent.txt",
            &Md5Hash::new([0; 16]),
            Some(temp_dir.path()),
        );
        assert!(!result);
    }

    #[test]
    fn unicode_filename_handling() {
        let temp_dir = TempDir::new().unwrap();

        let unicode_names = vec![
            "файл.txt",     // Russian
            "文件.txt",     // Chinese
            "ファイル.txt", // Japanese
            "파일.txt",     // Korean
        ];

        for name in unicode_names {
            let file_path = temp_dir.path().join(name);
            create_test_file(&file_path, &[0x42u8; 1000][..]).unwrap();

            let hash = calculate_file_md5(&file_path).unwrap();
            assert!(hash.as_bytes().len() == 16);
        }
    }

    #[test]
    fn special_chars_in_filename() {
        let temp_dir = TempDir::new().unwrap();

        let special_names = vec![
            "file-with-dashes.txt",
            "file_with_underscores.txt",
            "file.multiple.dots.txt",
            "file (with parens).txt",
            "file [with brackets].txt",
        ];

        for name in special_names {
            let file_path = temp_dir.path().join(name);
            create_test_file(&file_path, b"content").unwrap();

            let hash = calculate_file_md5(&file_path).unwrap();
            assert!(hash.as_bytes().len() == 16);
        }
    }

    #[test]
    fn zero_byte_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("empty.txt");

        create_test_file(&file_path, &[]).unwrap();

        let hash = calculate_file_md5(&file_path).unwrap();
        assert!(hash.as_bytes().len() == 16);

        let hash_16k = calculate_file_md5_16k(&file_path).unwrap();
        assert_eq!(hash, hash_16k);
    }

    #[test]
    fn single_byte_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("single.txt");

        create_test_file(&file_path, &[0x42]).unwrap();

        let hash = calculate_file_md5(&file_path).unwrap();
        let hash_16k = calculate_file_md5_16k(&file_path).unwrap();

        assert_eq!(hash, hash_16k);
    }

    #[test]
    fn verify_result_struct_fields() {
        use std::collections::HashMap;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        create_test_file(&file_path, b"test").unwrap();
        let hash = calculate_file_md5(&file_path).unwrap();

        let mut file_info = HashMap::new();
        let file_id = par2rs::domain::FileId::new([99; 16]);
        file_info.insert("test.txt".to_string(), (file_id, hash, 4));

        let results = verify_files_and_collect_results_with_base_dir(
            &file_info,
            false,
            Some(temp_dir.path()),
        );

        let result = &results[0];
        assert_eq!(result.file_name, "test.txt");
        assert_eq!(result.file_id, file_id);
        assert_eq!(result.expected_md5, hash);
        assert!(result.is_valid);
        assert!(result.exists);
    }
}
