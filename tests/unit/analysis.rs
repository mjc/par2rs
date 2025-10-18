//! Analysis Module Tests
//!
//! Tests for packet analysis, statistics calculation, and metadata extraction.
//! Organized into logical groups: filename extraction, statistics, file info, and edge cases.

use par2rs::analysis::*;
use std::fs;

// Helper function for tests that need to load all packets including recovery slices
fn load_packets_with_recovery(par2_files: &[std::path::PathBuf]) -> (Vec<par2rs::Packet>, usize) {
    use rustc_hash::FxHashSet as HashSet;
    use std::io::BufReader;
    let mut all_packets = Vec::new();
    let mut recovery_count = 0;
    let mut seen_hashes = HashSet::default();

    for par2_file in par2_files {
        let file = fs::File::open(par2_file).expect("Failed to open PAR2 file");
        let mut reader = BufReader::new(file);
        let packets = par2rs::parse_packets(&mut reader);

        // Deduplicate packets
        for packet in packets {
            let hash = par2rs::file_ops::get_packet_hash(&packet);
            if seen_hashes.insert(hash) {
                if matches!(packet, par2rs::Packet::RecoverySlice(_)) {
                    recovery_count += 1;
                }
                all_packets.push(packet);
            }
        }
    }

    (all_packets, recovery_count)
}

use std::path::Path;

mod filename_extraction {
    use super::*;

    #[test]
    fn extracts_unique_filenames_from_packets() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let mut file = fs::File::open(main_file).unwrap();
        let packets = par2rs::parse_packets(&mut file);

        let filenames = extract_unique_filenames(&packets);

        // Should find the test file
        assert_eq!(filenames.len(), 1);
        assert_eq!(filenames[0], "testfile");
    }

    #[test]
    fn returns_empty_list_when_no_packets() {
        let packets = vec![];
        let filenames = extract_unique_filenames(&packets);
        assert!(filenames.is_empty());
    }
}

mod statistics_calculation {
    use super::*;

    #[test]
    fn extracts_main_packet_stats_correctly() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let mut file = fs::File::open(main_file).unwrap();
        let packets = par2rs::parse_packets(&mut file);

        let (block_size, total_blocks) = extract_main_packet_stats(&packets);

        // Test file should have specific block size
        assert_eq!(block_size, 528);
        // Should have calculated correct number of blocks
        assert!(total_blocks > 0);
        assert_eq!(total_blocks, 1986); // Expected value for test file
    }

    #[test]
    fn calculates_total_size_correctly() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let mut file = fs::File::open(main_file).unwrap();
        let packets = par2rs::parse_packets(&mut file);

        let total_size = calculate_total_size(&packets);

        // Test file is 1MB
        assert_eq!(total_size, 1048576);
    }

    #[test]
    fn calculates_comprehensive_par2_stats() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let par2_files = par2rs::file_ops::collect_par2_files(main_file);
        let (packets, recovery_blocks) = load_packets_with_recovery(&par2_files);

        let stats = calculate_par2_stats(&packets, recovery_blocks);

        // Verify all statistics
        assert_eq!(stats.file_count, 1);
        assert_eq!(stats.block_size, 528);
        assert_eq!(stats.total_blocks, 1986);
        assert_eq!(stats.total_size, 1048576);
        assert!(stats.recovery_blocks > 0); // Should have recovery blocks from volume files
    }

    #[test]
    fn returns_default_stats_when_no_main_packet() {
        // Create empty packet vector
        let packets = vec![];

        let (block_size, total_blocks) = extract_main_packet_stats(&packets);

        // Should return defaults when no main packet present
        assert_eq!(block_size, 0);
        assert_eq!(total_blocks, 0);
    }

    #[test]
    fn returns_zero_size_for_empty_packets() {
        let packets = vec![];
        let total_size = calculate_total_size(&packets);
        assert_eq!(total_size, 0);
    }

    #[test]
    fn maintains_consistency_across_multiple_calculations() {
        // Load packets multiple times and ensure stats are consistent
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let par2_files = par2rs::file_ops::collect_par2_files(main_file);

        let (packets1, recovery_blocks1) = load_packets_with_recovery(&par2_files);
        let stats1 = calculate_par2_stats(&packets1, recovery_blocks1);

        let (packets2, recovery_blocks2) = load_packets_with_recovery(&par2_files);
        let stats2 = calculate_par2_stats(&packets2, recovery_blocks2);

        // Stats should be identical
        assert_eq!(stats1.file_count, stats2.file_count);
        assert_eq!(stats1.block_size, stats2.block_size);
        assert_eq!(stats1.total_blocks, stats2.total_blocks);
        assert_eq!(stats1.total_size, stats2.total_size);
        assert_eq!(stats1.recovery_blocks, stats2.recovery_blocks);
    }
}

mod file_information {
    use super::*;

    #[test]
    fn collects_file_info_from_packets() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let mut file = fs::File::open(main_file).unwrap();
        let packets = par2rs::parse_packets(&mut file);

        let file_info = collect_file_info_from_packets(&packets);

        // Should have one file
        assert_eq!(file_info.len(), 1);
        // Should contain the test file
        assert!(file_info.contains_key("testfile"));

        let (file_id, md5_hash, file_length) = file_info["testfile"];

        // File ID should not be all zeros
        assert_ne!(file_id, [0; 16]);
        // MD5 hash should not be all zeros
        assert_ne!(md5_hash, [0; 16]);
        // File length should match expected size
        assert_eq!(file_length, 1048576);
    }

    #[test]
    fn handles_multiple_volume_files() {
        // Load all packets from the par2 set
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let par2_files = par2rs::file_ops::collect_par2_files(main_file);
        let (packets, _) = load_packets_with_recovery(&par2_files);

        let file_info = collect_file_info_from_packets(&packets);

        // Even though we load from multiple volume files,
        // there should still be only one unique file described
        assert_eq!(file_info.len(), 1);
        assert!(file_info.contains_key("testfile"));
    }

    #[test]
    fn returns_empty_info_for_empty_packets() {
        let packets = vec![];
        let file_info = collect_file_info_from_packets(&packets);
        assert!(file_info.is_empty());
    }
}

mod par2_stats_struct {
    use super::*;

    #[test]
    fn supports_clone_and_debug() {
        let stats = Par2Stats {
            file_count: 5,
            block_size: 1024,
            total_blocks: 100,
            total_size: 102400,
            recovery_blocks: 20,
        };

        // Test that struct can be cloned and debugged
        let cloned_stats = stats.clone();
        assert_eq!(stats.file_count, cloned_stats.file_count);

        let debug_output = format!("{:?}", stats);
        assert!(debug_output.contains("file_count: 5"));
    }

    #[test]
    fn print_summary_does_not_panic() {
        let stats = Par2Stats {
            file_count: 1,
            block_size: 528,
            total_blocks: 1986,
            total_size: 1048576,
            recovery_blocks: 99,
        };

        // This test just ensures the function doesn't panic
        // In a real application, you might want to capture stdout
        print_summary_stats(&stats);
    }
}
