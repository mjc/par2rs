//! Tests for recovery block counting in verification
//!
//! This test verifies that the verification engine correctly counts available
//! recovery blocks from PAR2 recovery files.

use par2rs::Packet;
use std::path::Path;

#[test]
fn test_recovery_blocks_counted_in_verification() {
    // This fixture has:
    // - 1986 data blocks (testfile, 1MB with 528 byte blocks)
    // - Recovery files with 1+2+4+8+16+32+36 = 99 recovery blocks
    // - The testfile is damaged (block 1985 is corrupted)
    let par2_file = Path::new("tests/fixtures/repair_scenarios/testfile.par2");

    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    // Load all PAR2 files and packets
    let par2_files = par2rs::par2_files::collect_par2_files(par2_file);
    let packets = par2rs::par2_files::load_all_par2_packets(&par2_files);

    // Run verification
    let config = par2rs::verify::VerificationConfig::default();
    let reporter = par2rs::reporters::ConsoleVerificationReporter::new();
    let results = par2rs::verify::comprehensive_verify_files_with_config_and_reporter_in_dir(
        packets,
        &config,
        &reporter,
        "tests/fixtures/repair_scenarios",
    );

    // ASSERTIONS:
    // 1. Should detect the damaged file
    assert_eq!(
        results.corrupted_file_count, 1,
        "Should detect 1 corrupted file"
    );

    // 2. Should find 1985 out of 1986 blocks
    assert_eq!(
        results.available_block_count, 1985,
        "Should find 1985 available blocks"
    );
    assert_eq!(
        results.missing_block_count, 1,
        "Should find 1 missing block"
    );

    // 3. BUG: Should count recovery blocks from recovery files
    // Currently returns 0, but should return 99
    eprintln!(
        "recovery_blocks_available = {}",
        results.recovery_blocks_available
    );
    eprintln!("missing_block_count = {}", results.missing_block_count);
    eprintln!("repair_possible = {}", results.repair_possible);

    assert_eq!(
        results.recovery_blocks_available, 99,
        "Should count 99 recovery blocks from vol files. \
         Current bug: verification doesn't count recovery blocks, always returns 0."
    );

    // 4. Should determine repair is possible (99 recovery blocks > 1 missing block)
    assert!(
        results.repair_possible,
        "Repair should be possible with 99 recovery blocks and only 1 missing block. \
         Current bug: shows repair_possible=false because recovery_blocks_available=0"
    );
}

#[test]
fn test_recovery_blocks_from_multiple_vol_files() {
    // Test that we correctly count recovery blocks across multiple vol files
    let par2_file = Path::new("tests/fixtures/repair_scenarios/testfile.par2");

    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    let par2_files = par2rs::par2_files::collect_par2_files(par2_file);
    let packets = par2rs::par2_files::load_all_par2_packets(&par2_files);

    // Count recovery packets manually
    let recovery_count = packets
        .iter()
        .filter(|p| matches!(p, Packet::RecoverySlice(_)))
        .count();

    eprintln!("Found {} recovery packets in loaded files", recovery_count);

    // We should find recovery packets from all the vol files
    // testfile.vol00+01.par2 = 1 recovery block
    // testfile.vol01+02.par2 = 2 recovery blocks
    // testfile.vol03+04.par2 = 4 recovery blocks
    // testfile.vol07+08.par2 = 8 recovery blocks
    // testfile.vol15+16.par2 = 16 recovery blocks
    // testfile.vol31+32.par2 = 32 recovery blocks
    // testfile.vol63+36.par2 = 36 recovery blocks
    // Total = 99 recovery blocks
    assert_eq!(
        recovery_count, 99,
        "Should load 99 recovery packets from vol files"
    );
}

#[test]
fn test_repair_possible_calculation() {
    // Test that repair_possible is correctly calculated based on:
    // missing_block_count <= recovery_blocks_available
    let par2_file = Path::new("tests/fixtures/repair_scenarios/testfile.par2");

    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    let par2_files = par2rs::par2_files::collect_par2_files(par2_file);
    let packets = par2rs::par2_files::load_all_par2_packets(&par2_files);

    let config = par2rs::verify::VerificationConfig::default();
    let reporter = par2rs::reporters::SilentVerificationReporter;
    let results = par2rs::verify::comprehensive_verify_files_with_config_and_reporter_in_dir(
        packets,
        &config,
        &reporter,
        "tests/fixtures/repair_scenarios",
    );

    // With 1 missing block and 99 recovery blocks, repair should be possible
    assert!(
        results.missing_block_count <= results.recovery_blocks_available,
        "Repair should be mathematically possible: {} missing <= {} recovery",
        results.missing_block_count,
        results.recovery_blocks_available
    );

    assert!(
        results.repair_possible,
        "repair_possible should be true when we have enough recovery blocks"
    );

    assert_eq!(
        results.blocks_needed_for_repair, results.missing_block_count,
        "blocks_needed_for_repair should equal missing_block_count"
    );
}
