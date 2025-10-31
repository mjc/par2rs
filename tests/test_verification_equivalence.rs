use par2rs::par2_files::load_all_par2_packets;
use par2rs::verify::{comprehensive_verify_files_with_config, VerificationConfig};

#[test]
fn test_parallel_sequential_equivalence() {
    // This test verifies that parallel and sequential verification modes
    // produce identical results on the same dataset

    // Skip if no test files available
    let test_files = find_test_par2_files();
    if test_files.is_empty() {
        println!("No PAR2 test files found, skipping parallel/sequential equivalence test");
        return;
    }

    for test_file in test_files.iter().take(1) {
        // Test with first file to keep test time reasonable
        println!(
            "Testing parallel/sequential equivalence with: {}",
            test_file.display()
        );

        // Load PAR2 packets - need to load twice since Packet doesn't implement Clone
        let packets_parallel = load_all_par2_packets(std::slice::from_ref(test_file));
        let packets_sequential = load_all_par2_packets(std::slice::from_ref(test_file));

        if packets_parallel.is_empty() {
            println!("No packets loaded from {}, skipping", test_file.display());
            continue;
        }

        // Test with parallel mode
        let parallel_config = VerificationConfig {
            threads: 2, // Use 2 threads for deterministic testing
            parallel: true,
        };
        let parallel_results =
            comprehensive_verify_files_with_config(packets_parallel, &parallel_config);

        // Test with sequential mode
        let sequential_config = VerificationConfig {
            threads: 0, // Threads don't matter in sequential mode
            parallel: false,
        };
        let sequential_results =
            comprehensive_verify_files_with_config(packets_sequential, &sequential_config);

        // Compare core verification results
        assert_eq!(
            parallel_results.present_file_count,
            sequential_results.present_file_count,
            "Complete file count mismatch for {}",
            test_file.display()
        );
        assert_eq!(
            parallel_results.corrupted_file_count,
            sequential_results.corrupted_file_count,
            "Damaged file count mismatch for {}",
            test_file.display()
        );
        assert_eq!(
            parallel_results.missing_file_count,
            sequential_results.missing_file_count,
            "Missing file count mismatch for {}",
            test_file.display()
        );
        assert_eq!(
            parallel_results.renamed_file_count,
            sequential_results.renamed_file_count,
            "Renamed file count mismatch for {}",
            test_file.display()
        );

        assert_eq!(
            parallel_results.total_block_count,
            sequential_results.total_block_count,
            "Total block count mismatch for {}",
            test_file.display()
        );
        assert_eq!(
            parallel_results.available_block_count,
            sequential_results.available_block_count,
            "Available block count mismatch for {}",
            test_file.display()
        );
        assert_eq!(
            parallel_results.missing_block_count,
            sequential_results.missing_block_count,
            "Missing block count mismatch for {}",
            test_file.display()
        );

        assert_eq!(
            parallel_results.recovery_blocks_available,
            sequential_results.recovery_blocks_available,
            "Recovery blocks available mismatch for {}",
            test_file.display()
        );
        assert_eq!(
            parallel_results.repair_possible,
            sequential_results.repair_possible,
            "Repair possible mismatch for {}",
            test_file.display()
        );
        assert_eq!(
            parallel_results.blocks_needed_for_repair,
            sequential_results.blocks_needed_for_repair,
            "Blocks needed for repair mismatch for {}",
            test_file.display()
        );

        // Verify same number of files and file results
        assert_eq!(
            parallel_results.files.len(),
            sequential_results.files.len(),
            "File results count mismatch for {}",
            test_file.display()
        );

        // Sort both file results by filename for comparison (parallel processing may change order)
        let mut parallel_files = parallel_results.files;
        let mut sequential_files = sequential_results.files;
        parallel_files.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        sequential_files.sort_by(|a, b| a.file_name.cmp(&b.file_name));

        // Compare individual file results
        for (parallel_file, sequential_file) in parallel_files.iter().zip(sequential_files.iter()) {
            assert_eq!(
                parallel_file.file_name,
                sequential_file.file_name,
                "Filename mismatch for {}",
                test_file.display()
            );
            assert_eq!(
                parallel_file.status,
                sequential_file.status,
                "Status mismatch for file {} in {}",
                parallel_file.file_name,
                test_file.display()
            );
            assert_eq!(
                parallel_file.total_blocks,
                sequential_file.total_blocks,
                "Total blocks mismatch for file {} in {}",
                parallel_file.file_name,
                test_file.display()
            );
            assert_eq!(
                parallel_file.blocks_available,
                sequential_file.blocks_available,
                "Available blocks mismatch for file {} in {}",
                parallel_file.file_name,
                test_file.display()
            );
        }

        println!(
            "✓ Parallel and sequential modes produce identical results for {}",
            test_file.display()
        );
    }
}

#[test]
fn test_thread_count_consistency() {
    // Test that different thread counts in parallel mode produce consistent results
    let test_files = find_test_par2_files();
    if test_files.is_empty() {
        println!("No PAR2 test files found, skipping thread count consistency test");
        return;
    }

    let test_file = &test_files[0];
    println!(
        "Testing thread count consistency with: {}",
        test_file.display()
    );

    // Test with different thread counts
    let thread_counts = [1, 2];
    let mut results = Vec::new();

    for threads in thread_counts.iter() {
        // Load packets fresh for each test since Packet doesn't implement Clone
        let packets = load_all_par2_packets(std::slice::from_ref(test_file));
        if packets.is_empty() {
            println!("No packets loaded from {}, skipping", test_file.display());
            return;
        }

        let config = VerificationConfig {
            threads: *threads,
            parallel: true,
        };
        let result = comprehensive_verify_files_with_config(packets, &config);
        results.push(result);
    }

    // Compare all results - they should be identical regardless of thread count
    for i in 1..results.len() {
        let baseline = &results[0];
        let current = &results[i];

        assert_eq!(
            baseline.present_file_count, current.present_file_count,
            "Thread count {} produces different complete file count",
            thread_counts[i]
        );
        assert_eq!(
            baseline.total_block_count, current.total_block_count,
            "Thread count {} produces different total block count",
            thread_counts[i]
        );
        assert_eq!(
            baseline.repair_possible, current.repair_possible,
            "Thread count {} produces different repair possible result",
            thread_counts[i]
        );
    }

    println!("✓ All thread counts produce consistent results");
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
    files.truncate(5);
    files
}
