/// Test to verify recovery slice metadata parsing works correctly
use par2rs::par2_files;
use std::path::PathBuf;

#[test]
fn test_metadata_parsing_counts_all_recovery_slices() {
    // This test uses the Being Human PAR2 set which has 375 recovery blocks total
    // The bug was that parse_recovery_slice_metadata() only found 9 blocks

    let fixtures = PathBuf::from(
        "Monster.2022.S02E06.Dont.Dream.Its.Over.2160p.NF.WEB-DL.DDP5.1.Atmos.H.265-FLUX",
    );
    let par2_file = fixtures.join("L5hlLqa8Lud5wLjC4I9j9hmr.vol63+32.par2");

    if !par2_file.exists() {
        eprintln!("Skipping test - fixture file not found");
        return;
    }

    let par2_files = par2_files::collect_par2_files(&par2_file);

    // Parse metadata
    let metadata = par2_files::parse_recovery_slice_metadata(&par2_files, false);

    // Should find 32 recovery blocks in this one file
    assert_eq!(
        metadata.len(),
        32,
        "Should find all 32 recovery blocks in vol63+32.par2, but found {}",
        metadata.len()
    );

    // Verify exponents are correct (should be 63-94)
    let mut exponents: Vec<u32> = metadata.iter().map(|m| m.exponent).collect();
    exponents.sort();
    assert_eq!(exponents[0], 63, "First exponent should be 63");
    assert_eq!(exponents[31], 94, "Last exponent should be 94");
}

#[test]
fn test_metadata_parsing_vs_packet_parsing() {
    // Compare metadata parsing with direct packet parsing to ensure they find the same count

    let fixtures = PathBuf::from("tests/fixtures");
    let par2_file = fixtures.join("testfile.par2");

    if !par2_file.exists() {
        eprintln!("Skipping test - fixture file not found");
        return;
    }

    let par2_files = par2_files::collect_par2_files(&par2_file);

    // Parse with direct method (loads all packets including recovery slices)
    use std::fs::File;
    use std::io::BufReader;
    let mut packet_recovery_count = 0;
    for par2_file in &par2_files {
        let file = File::open(par2_file).unwrap();
        let mut reader = BufReader::new(file);
        let packets = par2rs::parse_packets(&mut reader);
        packet_recovery_count += packets
            .iter()
            .filter(|p| matches!(p, par2rs::Packet::RecoverySlice(_)))
            .count();
    }

    // Parse with new method (metadata only)
    let metadata = par2_files::parse_recovery_slice_metadata(&par2_files, false);

    assert_eq!(
        metadata.len(),
        packet_recovery_count,
        "Metadata parsing should find same number of recovery blocks as packet parsing"
    );
}
