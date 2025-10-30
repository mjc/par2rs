use par2rs::par2_files::load_all_par2_packets;
use par2rs::verify::VerificationConfig;

#[test]
fn test_file_deduplication_with_real_data() {
    // Test deduplication using actual PAR2 files to ensure it works correctly
    // This test uses the real file loading mechanism to test deduplication

    let test_files = find_test_par2_files();
    if test_files.is_empty() {
        println!("No PAR2 test files found, skipping deduplication test");
        return;
    }

    // Load a PAR2 file that potentially has multiple volumes (and thus duplicate FileDescription packets)
    let test_file = &test_files[0];
    println!("Testing file deduplication with: {}", test_file.display());

    let packets = load_all_par2_packets(std::slice::from_ref(test_file));
    if packets.is_empty() {
        println!("No packets loaded from {}, skipping", test_file.display());
        return;
    }

    // Count FileDescription packets in the loaded data
    let file_description_count = packets
        .iter()
        .filter(|p| matches!(p, par2rs::Packet::FileDescription(_)))
        .count();

    if file_description_count == 0 {
        println!("No FileDescription packets found, skipping");
        return;
    }

    println!(
        "Found {} FileDescription packets in PAR2 data",
        file_description_count
    );

    // Run verification which should deduplicate files
    let config = VerificationConfig::new(2, true);
    let results = par2rs::verify::comprehensive_verify_files_with_config(packets, &config);

    // In most cases, there should be fewer unique files than FileDescription packets
    // (unless it's a single-volume PAR2 set with no duplicates)
    println!("Verification found {} unique files", results.files.len());

    // Basic validation - should have at least some files
    assert!(
        !results.files.is_empty(),
        "Should find at least one file after deduplication"
    );

    // Verify no duplicate file names in results (indicating successful deduplication)
    let mut file_names: Vec<_> = results.files.iter().map(|f| &f.file_name).collect();
    let original_count = file_names.len();
    file_names.sort();
    file_names.dedup();
    assert_eq!(
        file_names.len(),
        original_count,
        "Found duplicate file names in results - deduplication failed"
    );

    println!(
        "✓ File deduplication working correctly - {} unique files processed",
        results.files.len()
    );
}

// Helper function to find available PAR2 test files
fn find_test_par2_files() -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();

    // Look in common test locations
    let search_paths = [
        "tests/fixtures",
        ".",
        "100gb",
        "Being.Human.US.S02.1080p.AMZN.WEB-DL.DD+2.0.H.264-playWEB",
    ];

    for search_path in search_paths.iter() {
        if let Ok(entries) = std::fs::read_dir(search_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(extension) = path.extension() {
                    if extension == "par2" {
                        files.push(path);
                    }
                }
            }
        }
    }

    // Limit to first few files to keep test time reasonable
    files.truncate(3);
    files
}
