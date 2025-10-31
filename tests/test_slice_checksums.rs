/// Tests for PAR2 slice checksum computation
///
/// PAR2 spec requires that slices be zero-padded to the slice size when computing checksums.
/// This is important for files where the last slice (or only slice) is smaller than the slice size.
use std::fs;
use std::path::Path;

#[test]
fn test_slice_checksum_requires_padding() {
    // This test demonstrates that PAR2 slice checksums must be computed on padded data

    // Create a small file (32 bytes) that's less than typical slice size (512 bytes)
    let test_data = b"This is file 1 with some content";
    assert_eq!(test_data.len(), 32); // "This is file 1 with some content" is actually 32 bytes

    // Compute MD5 without padding (incorrect)
    use md_5::Digest;
    let unpadded_md5: [u8; 16] = md_5::Md5::digest(test_data).into();
    let unpadded_hex = hex::encode(unpadded_md5);
    println!("Unpadded MD5 (33 bytes): {}", unpadded_hex);

    // Compute MD5 with zero-padding to 512 bytes (correct for PAR2)
    let mut padded_data = vec![0u8; 512];
    padded_data[..32].copy_from_slice(test_data);
    let padded_md5: [u8; 16] = md_5::Md5::digest(&padded_data).into();
    let padded_hex = hex::encode(padded_md5);
    println!("Padded MD5 (512 bytes):  {}", padded_hex);

    // These should be different!
    assert_ne!(
        unpadded_hex, padded_hex,
        "Padded and unpadded checksums should differ"
    );

    // The PAR2 spec requires padding
    assert_eq!(
        padded_hex, "aa3124f070b41f3d511bcb2387876fb2",
        "Padded MD5 should be computed correctly"
    );
}

#[test]
fn test_actual_par2_slice_checksums() {
    // Verify that the test fixture files match their PAR2 checksums when properly padded

    let test_files = [
        (
            "tests/fixtures/textfiles_test/file1.txt",
            "c7d88ea92c6fca90f5d0c2619659312d",
        ),
        (
            "tests/fixtures/textfiles_test/file2.txt",
            "e863fb9b6d0066a3e8758f98656b47ff",
        ),
        (
            "tests/fixtures/textfiles_test/file3.txt",
            "c28b3007285e46926525769fed5c5c01",
        ),
    ];

    for (file_path, expected_md5) in &test_files {
        if !Path::new(file_path).exists() {
            println!("Skipping {} - file not found", file_path);
            continue;
        }

        let file_data = fs::read(file_path).unwrap();
        let file_size = file_data.len();

        println!("\nFile: {} ({} bytes)", file_path, file_size);

        // Compute unpadded MD5
        use md_5::Digest;
        let unpadded_md5: [u8; 16] = md_5::Md5::digest(&file_data).into();
        println!("  Unpadded MD5: {}", hex::encode(unpadded_md5));

        // Compute padded MD5 (PAR2 uses 512-byte slices for these files)
        let mut padded_data = vec![0u8; 512];
        padded_data[..file_size].copy_from_slice(&file_data);
        let padded_md5: [u8; 16] = md_5::Md5::digest(&padded_data).into();
        let padded_hex = hex::encode(padded_md5);
        println!("  Padded MD5:   {}", padded_hex);
        println!("  Expected:     {}", expected_md5);

        assert_eq!(
            padded_hex, *expected_md5,
            "Padded MD5 for {} should match PAR2 checksum",
            file_path
        );
    }
}

#[test]
fn test_load_all_slices_with_padding_during_verify() {
    // Test that verification works correctly with padding
    use par2rs::repair::repair_files;
    use std::path::PathBuf;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Copy test fixtures
    let fixture_dir = PathBuf::from("tests/fixtures/textfiles_test");
    if !fixture_dir.exists() {
        println!("Skipping test - fixtures not found");
        return;
    }

    for entry in fs::read_dir(&fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let file_name = entry.file_name();
        let source = entry.path();
        let dest = temp_path.join(&file_name);
        fs::copy(&source, &dest).unwrap();
    }

    // Verify files without any corruption
    let par2_file = temp_path.join("textfiles.par2");
    let (_context, result) = repair_files(par2_file.to_str().unwrap()).unwrap();

    // All files should verify successfully if we're computing slice checksums correctly
    match result {
        par2rs::repair::RepairResult::NoRepairNeeded { files_verified, .. } => {
            assert_eq!(
                files_verified, 3,
                "All 3 files should verify (this will fail if padding is wrong)"
            );
        }
        _ => panic!("Expected NoRepairNeeded result"),
    }
}

#[test]
fn test_load_all_slices_during_repair_needs_padding() {
    // This test will FAIL until we fix load_all_slices to pad slices properly
    // The issue is that load_all_slices is called during REPAIR to load existing slices,
    // and it needs to compute checksums to verify which slices are valid.
    use par2rs::repair::repair_files;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Copy test fixtures
    let fixture_dir = PathBuf::from("tests/fixtures/textfiles_test");
    if !fixture_dir.exists() {
        println!("Skipping test - fixtures not found");
        return;
    }

    for entry in fs::read_dir(&fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let file_name = entry.file_name();
        let source = entry.path();
        let dest = temp_path.join(&file_name);
        fs::copy(&source, &dest).unwrap();
    }

    // Corrupt ONE file (file2.txt) while leaving others intact
    let file2_path = temp_path.join("file2.txt");
    let mut file2 = fs::OpenOptions::new()
        .write(true)
        .open(&file2_path)
        .unwrap();
    file2.write_all(b"CORRUPTED DATA").unwrap();
    drop(file2);

    // Try to repair
    let par2_file = temp_path.join("textfiles.par2");
    let (_context, result) = repair_files(par2_file.to_str().unwrap()).unwrap();

    println!("\n=== Repair Result ===");
    println!("Files repaired: {:?}", result.repaired_files());
    println!("Files failed: {:?}", result.failed_files());

    // The repair should work if load_all_slices properly loads file1.txt and file3.txt
    // Currently this FAILS because load_all_slices doesn't pad slices for checksum computation
    assert!(
        result.repaired_files().contains(&"file2.txt".to_string())
            || result.failed_files().contains(&"file2.txt".to_string()),
        "file2.txt should be repaired or marked as failed"
    );

    if result.failed_files().contains(&"file2.txt".to_string()) {
        panic!(
            "EXPECTED FAILURE: load_all_slices doesn't pad slices for checksum verification, \
                so it can't load valid slices from file1.txt and file3.txt, \
                causing repair to fail"
        );
    }
}
