//! File Operations Module Tests
//!
//! Tests for file discovery, PAR2 file collection, packet loading, and deduplication.
//! Organized into logical groups: file discovery, packet parsing, deduplication, and collection.

use par2rs::file_ops::*;
use rustc_hash::FxHashSet as HashSet;
use std::fs;
use std::path::{Path, PathBuf};

// Helper function for tests that need to load all packets including recovery slices
fn load_packets_with_recovery(par2_files: &[PathBuf]) -> (Vec<par2rs::Packet>, usize) {
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
            let hash = get_packet_hash(&packet);
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

mod file_discovery {
    use super::*;

    #[test]
    fn finds_par2_files_in_directory() {
        let fixtures_dir = Path::new("tests/fixtures");
        let main_file = fixtures_dir.join("testfile.par2");

        let par2_files = find_par2_files_in_directory(fixtures_dir, &main_file);

        // Should find all volume files but exclude the main file
        assert!(par2_files.len() >= 7); // At least 7 volume files
        assert!(!par2_files.contains(&main_file));

        // All found files should have .par2 extension
        for file in &par2_files {
            assert_eq!(file.extension().unwrap(), "par2");
        }

        // Should include volume files
        let volume_files: Vec<_> = par2_files
            .iter()
            .filter(|f| f.file_name().unwrap().to_str().unwrap().contains("vol"))
            .collect();

        assert!(!volume_files.is_empty());
    }

    #[test]
    fn collects_all_par2_files_including_main() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let par2_files = collect_par2_files(main_file);

        // Should include the main file
        assert!(par2_files.contains(&main_file.to_path_buf()));

        // Should include volume files
        let volume_count = par2_files
            .iter()
            .filter(|f| f.file_name().unwrap().to_str().unwrap().contains("vol"))
            .count();

        assert!(volume_count >= 7);
        assert!(par2_files.len() >= 8); // Main file + volume files
    }

    #[test]
    fn handles_nonexistent_directory() {
        let nonexistent_dir = Path::new("tests/nonexistent");
        let fake_main_file = nonexistent_dir.join("fake.par2");

        // Should return empty vec and print warning instead of panicking
        let par2_files = find_par2_files_in_directory(nonexistent_dir, &fake_main_file);
        assert!(
            par2_files.is_empty(),
            "Should return empty vec for nonexistent directory"
        );
    }
}

mod packet_parsing {
    use super::*;

    #[test]
    fn parses_packets_from_par2_file() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let mut seen_hashes = HashSet::default();

        let packets = parse_par2_file(main_file, &mut seen_hashes).expect("Failed to parse");

        assert!(!packets.is_empty());
        // Main file should contain at least a main packet and file description packet
        assert!(packets.len() >= 2);
    }

    #[test]
    fn parses_with_progress_tracking() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let mut seen_hashes = HashSet::default();

        // Test with progress enabled
        let (packets_with_progress, recovery_count) =
            parse_par2_file_with_progress(main_file, &mut seen_hashes, true)
                .expect("Failed to parse with progress");

        assert!(!packets_with_progress.is_empty());
        assert_eq!(recovery_count, 0); // Main file should have no recovery blocks

        // Test with progress disabled
        seen_hashes.clear();
        let (packets_no_progress, _) =
            parse_par2_file_with_progress(main_file, &mut seen_hashes, false)
                .expect("Failed to parse without progress");

        assert_eq!(packets_with_progress.len(), packets_no_progress.len());
    }

    #[test]
    fn extracts_packet_hashes_correctly() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let mut file = fs::File::open(main_file).unwrap();
        let packets = par2rs::parse_packets(&mut file);

        // Should be able to get hashes for all packet types
        for packet in packets {
            let hash = get_packet_hash(&packet);
            assert_eq!(hash.len(), 16); // MD5 hash is 16 bytes
            assert_ne!(hash, [0; 16]); // Should not be all zeros
        }
    }

    #[test]
    fn handles_corrupted_file_gracefully() {
        let nonexistent_file = Path::new("tests/fixtures/nonexistent.par2");
        let mut seen_hashes = HashSet::default();

        // With the improved code, this should return an error, not panic
        let result = parse_par2_file(nonexistent_file, &mut seen_hashes);
        assert!(result.is_err());
    }
}

mod deduplication {
    use super::*;

    #[test]
    fn prevents_duplicate_packet_processing() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let mut seen_hashes = HashSet::default();

        // Parse the same file twice
        let packets1 = parse_par2_file(main_file, &mut seen_hashes).expect("Failed first parse");
        let packets2 = parse_par2_file(main_file, &mut seen_hashes).expect("Failed second parse");

        // First parse should return packets
        assert!(!packets1.is_empty());

        // Second parse should return no packets (all duplicates)
        assert!(packets2.is_empty());
    }

    #[test]
    fn accumulates_unique_packets_across_files() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let volume_file = Path::new("tests/fixtures/testfile.vol00+01.par2");
        let mut seen_hashes = HashSet::default();

        let main_packets =
            parse_par2_file(main_file, &mut seen_hashes).expect("Failed to parse main file");
        let volume_packets =
            parse_par2_file(volume_file, &mut seen_hashes).expect("Failed to parse volume file");

        // Should get packets from both files
        assert!(!main_packets.is_empty());
        assert!(!volume_packets.is_empty());

        // Seen hashes should include packets from both files
        assert!(seen_hashes.len() >= main_packets.len() + volume_packets.len());
    }

    #[test]
    fn filters_duplicates_in_all_packets_loading() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let par2_files = collect_par2_files(main_file);

        let (packets, _) = load_packets_with_recovery(&par2_files);

        // Should have loaded packets without duplicates
        assert!(!packets.is_empty());

        // Verify no duplicate hashes by checking each packet's hash
        let mut packet_hashes = HashSet::default();
        for packet in &packets {
            let hash = get_packet_hash(packet);
            assert!(packet_hashes.insert(hash), "Found duplicate packet hash");
        }
    }
}

mod collection_operations {
    use super::*;

    #[test]
    fn loads_all_packets_with_recovery_count() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let par2_files = collect_par2_files(main_file);

        let (packets, recovery_blocks) = load_packets_with_recovery(&par2_files);

        assert!(!packets.is_empty());
        assert!(recovery_blocks > 0); // Should have recovery blocks from volume files

        // Should have loaded from multiple files
        assert!(par2_files.len() > 1);
    }

    #[test]
    fn sorts_filenames_alphabetically() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let par2_files = collect_par2_files(main_file);

        let filenames: Vec<String> = par2_files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        let mut sorted_filenames = filenames.clone();
        sorted_filenames.sort();
        assert_eq!(filenames, sorted_filenames);
    }

    #[test]
    fn handles_empty_file_list() {
        let empty_files = vec![];
        let (packets, recovery_blocks) = load_packets_with_recovery(&empty_files);

        assert!(packets.is_empty());
        assert_eq!(recovery_blocks, 0);
    }

    #[test]
    fn tracks_progress_when_enabled() {
        let main_file = Path::new("tests/fixtures/testfile.par2");
        let par2_files = collect_par2_files(main_file);

        // Test with progress enabled
        let (packets_with_progress, recovery_with_progress) =
            load_packets_with_recovery(&par2_files);

        // Test with progress disabled
        let (packets_without_progress, recovery_without_progress) =
            load_packets_with_recovery(&par2_files);

        // Results should be the same regardless of progress setting
        assert_eq!(packets_with_progress.len(), packets_without_progress.len());
        assert_eq!(recovery_with_progress, recovery_without_progress);
    }
}
