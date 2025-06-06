//! Tests for file_ops module
//!
//! This module tests file discovery, PAR2 file collection,
//! packet loading, and deduplication functionality.

use par2rs::file_ops::*;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[test]
fn test_find_par2_files_in_directory() {
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
fn test_collect_par2_files() {
    let main_file = Path::new("tests/fixtures/testfile.par2");

    let par2_files = collect_par2_files(main_file);

    // Should include the main file as the first entry
    assert_eq!(par2_files[0], main_file);

    // Should find multiple files
    assert!(par2_files.len() > 1);

    // Files should be sorted
    let filenames: Vec<_> = par2_files
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap())
        .collect();
    let mut sorted_filenames = filenames.clone();
    sorted_filenames.sort();
    assert_eq!(filenames, sorted_filenames);
}

#[test]
fn test_get_packet_hash() {
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
fn test_parse_par2_file_deduplication() {
    let main_file = Path::new("tests/fixtures/testfile.par2");
    let mut seen_hashes = HashSet::new();

    // Parse the same file twice
    let packets1 = parse_par2_file(main_file, &mut seen_hashes);
    let packets2 = parse_par2_file(main_file, &mut seen_hashes);

    // First parse should return packets
    assert!(!packets1.is_empty());

    // Second parse should return no packets (all duplicates)
    assert!(packets2.is_empty());
}

#[test]
fn test_parse_par2_file_with_progress() {
    let main_file = Path::new("tests/fixtures/testfile.par2");
    let mut seen_hashes = HashSet::new();

    // Test with progress enabled
    let (packets_with_progress, recovery_count) =
        parse_par2_file_with_progress(main_file, &mut seen_hashes, true);

    assert!(!packets_with_progress.is_empty());
    assert_eq!(recovery_count, 0); // Main file should have no recovery blocks

    // Test with progress disabled
    seen_hashes.clear();
    let (packets_no_progress, _) =
        parse_par2_file_with_progress(main_file, &mut seen_hashes, false);

    assert_eq!(packets_with_progress.len(), packets_no_progress.len());
}

#[test]
fn test_count_recovery_blocks() {
    // Test with main file (should have 0 recovery blocks)
    let main_file = Path::new("tests/fixtures/testfile.par2");
    let mut file = fs::File::open(main_file).unwrap();
    let packets = par2rs::parse_packets(&mut file);

    let recovery_count = count_recovery_blocks(&packets);
    assert_eq!(recovery_count, 0);

    // Test with volume file (should have recovery blocks)
    let volume_file = Path::new("tests/fixtures/testfile.vol00+01.par2");
    let mut file = fs::File::open(volume_file).unwrap();
    let packets = par2rs::parse_packets(&mut file);

    let recovery_count = count_recovery_blocks(&packets);
    assert_eq!(recovery_count, 1); // This volume should have 1 recovery block
}

#[test]
fn test_load_all_par2_packets() {
    let main_file = Path::new("tests/fixtures/testfile.par2");
    let par2_files = collect_par2_files(main_file);

    let (all_packets, total_recovery_blocks) = load_all_par2_packets(&par2_files, false);

    // Should load packets from all files
    assert!(!all_packets.is_empty());

    // Should have some recovery blocks from volume files
    assert!(total_recovery_blocks > 0);

    // Should have exactly one main packet
    let main_packets: Vec<_> = all_packets
        .iter()
        .filter(|p| matches!(p, par2rs::Packet::Main(_)))
        .collect();
    assert_eq!(main_packets.len(), 1);

    // Should have file description packets
    let file_desc_packets: Vec<_> = all_packets
        .iter()
        .filter(|p| matches!(p, par2rs::Packet::FileDescription(_)))
        .collect();
    assert!(!file_desc_packets.is_empty());
}

#[test]
fn test_load_all_par2_packets_deduplication() {
    let main_file = Path::new("tests/fixtures/testfile.par2");

    // Load packets twice with the same file list
    let par2_files = vec![main_file.to_path_buf(), main_file.to_path_buf()];
    let (all_packets, _) = load_all_par2_packets(&par2_files, false);

    // Should not have duplicates despite loading the same file twice
    let mut packet_hashes = HashSet::new();
    for packet in &all_packets {
        let hash = get_packet_hash(packet);
        assert!(packet_hashes.insert(hash), "Found duplicate packet");
    }
}

#[test]
fn test_nonexistent_file() {
    let nonexistent = Path::new("tests/fixtures/nonexistent.par2");
    let mut seen_hashes = HashSet::new();

    // Should panic when trying to parse nonexistent file
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        parse_par2_file(nonexistent, &mut seen_hashes)
    }));
    assert!(result.is_err());
}

#[test]
fn test_empty_directory() {
    // Create a temporary empty directory
    let temp_dir = std::env::temp_dir().join("empty_par2_test");
    let _ = fs::create_dir(&temp_dir);

    let main_file = temp_dir.join("nonexistent.par2");
    let found_files = find_par2_files_in_directory(&temp_dir, &main_file);

    assert!(found_files.is_empty());

    // Clean up
    let _ = fs::remove_dir(&temp_dir);
}
