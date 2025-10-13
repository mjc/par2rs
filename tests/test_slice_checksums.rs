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
    let unpadded_md5 = md5::compute(test_data);
    let unpadded_hex = format!("{:x}", unpadded_md5);
    println!("Unpadded MD5 (33 bytes): {}", unpadded_hex);

    // Compute MD5 with zero-padding to 512 bytes (correct for PAR2)
    let mut padded_data = vec![0u8; 512];
    padded_data[..32].copy_from_slice(test_data);
    let padded_md5 = md5::compute(&padded_data);
    let padded_hex = format!("{:x}", padded_md5);
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
            "tests/fixtures/multifile_test/file1.txt",
            "c7d88ea92c6fca90f5d0c2619659312d",
        ),
        (
            "tests/fixtures/multifile_test/file2.txt",
            "9345911b386490ecf24df9ae1e35f4cf",
        ),
        (
            "tests/fixtures/multifile_test/file3.txt",
            "a4d007ed3a0a7a4d2dfe951d1b29e8f6",
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
        let unpadded_md5 = md5::compute(&file_data);
        println!("  Unpadded MD5: {:x}", unpadded_md5);

        // Compute padded MD5 (PAR2 uses 512-byte slices for these files)
        let mut padded_data = vec![0u8; 512];
        padded_data[..file_size].copy_from_slice(&file_data);
        let padded_md5 = md5::compute(&padded_data);
        let padded_hex = format!("{:x}", padded_md5);
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
    let fixture_dir = PathBuf::from("tests/fixtures/multifile_test");
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
    let par2_file = temp_path.join("multifile.par2");
    let result = repair_files(par2_file.to_str().unwrap(), &[]).unwrap();

    // All files should verify successfully if we're computing slice checksums correctly
    assert_eq!(
        result.files_verified, 3,
        "All 3 files should verify (this will fail if padding is wrong)"
    );
    assert_eq!(result.files_repaired, 0, "No repairs needed");
    assert!(
        result.files_failed.is_empty(),
        "No files should fail verification: {:?}",
        result.files_failed
    );
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
    let fixture_dir = PathBuf::from("tests/fixtures/multifile_test");
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
    let par2_file = temp_path.join("multifile.par2");
    let result = repair_files(par2_file.to_str().unwrap(), &[]).unwrap();

    println!("\n=== Repair Result ===");
    println!("Files repaired: {:?}", result.repaired_files);
    println!("Files verified: {}", result.files_verified);
    println!("Files failed: {:?}", result.files_failed);

    // The repair should work if load_all_slices properly loads file1.txt and file3.txt
    // Currently this FAILS because load_all_slices doesn't pad slices for checksum computation
    assert!(
        result.repaired_files.contains(&"file2.txt".to_string())
            || result.files_failed.contains(&"file2.txt".to_string()),
        "file2.txt should be repaired or marked as failed"
    );

    if result.files_failed.contains(&"file2.txt".to_string()) {
        panic!(
            "EXPECTED FAILURE: load_all_slices doesn't pad slices for checksum verification, \
                so it can't load valid slices from file1.txt and file3.txt, \
                causing repair to fail"
        );
    }
}
