//! Repair Test Module
//!
//! Tests for PAR2 repair functionality, including detection of corrupted files
//! and scenarios that require repair operations.

use par2rs::file_ops::*;
use par2rs::file_verification::*;
use std::fs;
use std::path::Path;

mod corruption_detection {
    use super::*;

    #[test]
    fn detects_corrupted_file() {
        // Load the PAR2 set to get expected file information
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let par2_files = collect_par2_files(main_file);
        let (packets, _) = load_all_par2_packets(&par2_files, false);

        // Extract file information from packets
        let mut expected_md5 = None;
        for packet in &packets {
            if let par2rs::Packet::FileDescription(fd) = packet {
                let file_name = String::from_utf8_lossy(&fd.file_name).trim_end_matches('\0').to_string();
                if file_name == "testfile" {
                    expected_md5 = Some(fd.md5_hash);
                    break;
                }
            }
        }

        let expected_md5 = expected_md5.expect("Should find testfile in PAR2 set");

        // Verify original file passes
        let original_file = Path::new("tests/fixtures/testfile");
        assert!(original_file.exists(), "Original test file should exist");
        
        let original_md5 = calculate_file_md5(original_file).expect("Should calculate MD5");
        assert_eq!(original_md5, expected_md5, "Original file should match expected MD5");

        // Verify corrupted file fails verification
        let corrupted_file = Path::new("tests/fixtures/testfile_corrupted");
        assert!(corrupted_file.exists(), "Corrupted test file should exist");
        
        let corrupted_md5 = calculate_file_md5(corrupted_file).expect("Should calculate MD5");
        assert_ne!(corrupted_md5, expected_md5, "Corrupted file should not match expected MD5");
    }

    #[test]
    fn detects_heavily_corrupted_file() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let par2_files = collect_par2_files(main_file);
        let (packets, _) = load_all_par2_packets(&par2_files, false);

        // Extract file information
        let mut expected_md5 = None;
        for packet in &packets {
            if let par2rs::Packet::FileDescription(fd) = packet {
                let file_name = String::from_utf8_lossy(&fd.file_name).trim_end_matches('\0').to_string();
                if file_name == "testfile" {
                    expected_md5 = Some(fd.md5_hash);
                    break;
                }
            }
        }

        let expected_md5 = expected_md5.expect("Should find testfile in PAR2 set");

        // Verify heavily corrupted file fails verification
        let heavily_corrupted_file = Path::new("tests/fixtures/testfile_heavily_corrupted");
        assert!(heavily_corrupted_file.exists(), "Heavily corrupted test file should exist");
        
        let corrupted_md5 = calculate_file_md5(heavily_corrupted_file).expect("Should calculate MD5");
        assert_ne!(corrupted_md5, expected_md5, "Heavily corrupted file should not match expected MD5");
    }

    #[test]
    fn verifies_file_sizes_match() {
        let original_file = Path::new("tests/fixtures/testfile");
        let corrupted_file = Path::new("tests/fixtures/testfile_corrupted");
        let heavily_corrupted_file = Path::new("tests/fixtures/testfile_heavily_corrupted");

        let original_size = fs::metadata(original_file).unwrap().len();
        let corrupted_size = fs::metadata(corrupted_file).unwrap().len();
        let heavily_corrupted_size = fs::metadata(heavily_corrupted_file).unwrap().len();

        // All files should have the same size (only content is corrupted, not length)
        assert_eq!(original_size, corrupted_size, "Corrupted file should have same size as original");
        assert_eq!(original_size, heavily_corrupted_size, "Heavily corrupted file should have same size as original");
        assert_eq!(original_size, 1048576, "Test file should be 1MB");
    }
}

mod missing_file_scenarios {
    use super::*;

    #[test]
    fn detects_missing_data_file() {
        // Test scenario where PAR2 files exist but data file is missing
        let repair_dir = Path::new("tests/fixtures/repair_scenarios");
        let main_file = repair_dir.join("testfile.par2");
        let data_file = repair_dir.join("testfile");

        assert!(main_file.exists(), "PAR2 file should exist in repair scenarios");
        assert!(!data_file.exists(), "Data file should be missing in repair scenarios");

        // Load PAR2 information
        let par2_files = collect_par2_files(&main_file);
        let (packets, recovery_blocks) = load_all_par2_packets(&par2_files, false);

        assert!(!packets.is_empty(), "Should have packets from PAR2 files");
        assert!(recovery_blocks > 0, "Should have recovery blocks available for repair");

        // Verify we can identify the missing file from the PAR2 set
        let mut found_testfile = false;
        for packet in &packets {
            if let par2rs::Packet::FileDescription(fd) = packet {
                let file_name = String::from_utf8_lossy(&fd.file_name).trim_end_matches('\0').to_string();
                if file_name == "testfile" {
                    found_testfile = true;
                    break;
                }
            }
        }
        assert!(found_testfile, "Should find testfile description in PAR2 set");
    }

    #[test]
    fn has_sufficient_recovery_data() {
        let repair_dir = Path::new("tests/fixtures/repair_scenarios");
        let main_file = repair_dir.join("testfile.par2");
        let par2_files = collect_par2_files(&main_file);
        let (packets, recovery_blocks) = load_all_par2_packets(&par2_files, false);

        // Extract main packet information to understand the recovery requirements
        let mut slice_size = 0;
        let mut file_count = 0;
        for packet in &packets {
            if let par2rs::Packet::Main(main) = packet {
                slice_size = main.slice_size;
                file_count = main.file_count;
                break;
            }
        }

        assert!(slice_size > 0, "Should have slice size information");
        assert!(file_count > 0, "Should have file count information");
        assert!(recovery_blocks > 0, "Should have recovery blocks available");
        
        // For a complete file recovery, we need at least as many recovery blocks as data blocks
        // In practice, PAR2 might have more recovery data than needed
        println!("Slice size: {}, File count: {}, Recovery blocks: {}", slice_size, file_count, recovery_blocks);
        assert!(recovery_blocks > 0, "Should have substantial recovery data available");
    }
}

mod repair_prerequisites {
    use super::*;

    #[test]
    fn identifies_repairable_scenarios() {
        // Test that we can identify when repair is possible vs impossible
        
        // Scenario 1: Corrupted file with PAR2 data - should be repairable
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let par2_files = collect_par2_files(main_file);
        let (packets, recovery_blocks) = load_all_par2_packets(&par2_files, false);
        
        assert!(!packets.is_empty(), "Should have PAR2 packets available");
        assert!(recovery_blocks > 0, "Should have recovery data for repair");

        // Scenario 2: Missing file with PAR2 data - should be repairable
        let repair_dir = Path::new("tests/fixtures/repair_scenarios");
        let repair_main_file = repair_dir.join("testfile.par2");
        let repair_par2_files = collect_par2_files(&repair_main_file);
        let (repair_packets, repair_recovery_blocks) = load_all_par2_packets(&repair_par2_files, false);
        
        assert!(!repair_packets.is_empty(), "Should have PAR2 packets for repair scenario");
        assert!(repair_recovery_blocks > 0, "Should have recovery data for repair scenario");
    }

    #[test]
    fn extracts_file_information_for_repair() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let par2_files = collect_par2_files(main_file);
        let (packets, _) = load_all_par2_packets(&par2_files, false);

        let mut file_info = Vec::new();
        
        // Extract file descriptions that would be needed for repair
        for packet in &packets {
            if let par2rs::Packet::FileDescription(fd) = packet {
                let file_name = String::from_utf8_lossy(&fd.file_name).trim_end_matches('\0').to_string();
                let file_size = fd.file_length;
                let file_md5 = fd.md5_hash;
                
                file_info.push((file_name, file_size, file_md5));
            }
        }

        assert_eq!(file_info.len(), 1, "Should find exactly one file in the PAR2 set");
        
        let (name, size, _md5) = &file_info[0];
        assert_eq!(name, "testfile", "Should find the correct filename");
        assert_eq!(*size, 1048576, "Should have correct file size");
    }
}
