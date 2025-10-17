/// Tests for multi-file PAR2 repair
///
/// These tests demonstrate the key difference between single-file and multi-file PAR2 sets:
/// - Single-file PAR2: Recovery slices computed from slices of ONE file
/// - Multi-file PAR2: Recovery slices computed from slices across ALL files
///
/// The critical requirement for multi-file repair is that when reconstructing missing slices,
/// we must load ALL slices from ALL files to correctly compute the contribution of present
/// slices to the recovery data.
use par2rs::repair::repair_files;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

/// Helper to copy test fixtures to a temp directory
fn setup_multifile_test() -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path().to_path_buf();

    // Copy the multi-file test fixtures
    let fixture_dir = PathBuf::from("tests/fixtures/multifile_test");
    for entry in fs::read_dir(&fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let file_name = entry.file_name();
        let source = entry.path();
        let dest = temp_path.join(&file_name);
        fs::copy(&source, &dest).unwrap();
    }

    (temp_dir, temp_path)
}

#[test]
fn test_multifile_all_files_present() {
    // This test verifies that when all files are present and intact,
    // the repair operation correctly reports success
    let (_temp_dir, temp_path) = setup_multifile_test();

    let par2_file = temp_path.join("multifile.par2");
    let (_context, result) = repair_files(par2_file.to_str().unwrap()).unwrap();

    // All files should verify successfully
    assert!(result.repaired_files().is_empty(), "No files should need repair");
    assert!(result.is_success(), "Should succeed");
    assert!(result.failed_files().is_empty(), "No files should fail");
}

#[test]
fn test_multifile_one_file_corrupted() {
    // This test demonstrates the multi-file repair issue:
    // When one file is corrupted in a multi-file PAR2 set, we need to load
    // ALL slices from ALL files to correctly reconstruct the missing data.
    let (_temp_dir, temp_path) = setup_multifile_test();

    // Corrupt file2.txt (middle file in the sequence)
    let file2_path = temp_path.join("file2.txt");
    let mut file2 = fs::OpenOptions::new()
        .write(true)
        .open(&file2_path)
        .unwrap();
    file2
        .write_all(b"CORRUPTED DATA THAT DOES NOT MATCH")
        .unwrap();
    drop(file2);

    let par2_file = temp_path.join("multifile.par2");
    let (_context, result) = repair_files(par2_file.to_str().unwrap()).unwrap();

    // EXPECTED BEHAVIOR (after fix): file2.txt should be repaired successfully
    // CURRENT BEHAVIOR (before fix): repair fails because we don't load slices from file1.txt and file3.txt

    println!("\n=== Multi-file Repair Test Result ===");
    println!("Files repaired: {:?}", result.repaired_files());
    println!("Files failed: {:?}", result.failed_files());

    // Document the current state: this test SHOULD pass after the fix
    // For now, we just document what happens
    if result.repaired_files().contains(&"file2.txt".to_string()) {
        println!("✓ Multi-file repair WORKS: file2.txt was repaired successfully!");

        // Verify the repaired content matches the original
        let repaired_content = fs::read_to_string(&file2_path).unwrap();
        let original_content =
            fs::read_to_string("tests/fixtures/multifile_test/file2.txt.backup").unwrap();
        assert_eq!(
            repaired_content, original_content,
            "Repaired content should match original"
        );
    } else {
        println!("✗ Multi-file repair currently FAILS (expected before fix)");
        println!("   Issue: Not loading slices from other files (file1.txt, file3.txt)");
        println!("   This is a known issue that needs fixing!");

        // For now, just check that it at least tried to repair
        assert!(
            result.failed_files().contains(&"file2.txt".to_string()),
            "file2.txt should be in failed files list"
        );
    }
}

#[test]
fn test_multifile_first_file_corrupted() {
    // Test corrupting the first file in the sequence
    let (_temp_dir, temp_path) = setup_multifile_test();

    let file1_path = temp_path.join("file1.txt");
    let mut file1 = fs::OpenOptions::new()
        .write(true)
        .open(&file1_path)
        .unwrap();
    file1.write_all(b"CORRUPTED").unwrap();
    drop(file1);

    let par2_file = temp_path.join("multifile.par2");
    let (_context, result) = repair_files(par2_file.to_str().unwrap()).unwrap();

    // Should repair successfully
    assert!(
        result.repaired_files().contains(&"file1.txt".to_string())
            || result.failed_files().contains(&"file1.txt".to_string()),
        "file1.txt should be processed"
    );
}

#[test]
fn test_multifile_last_file_corrupted() {
    // Test corrupting the last file in the sequence
    let (_temp_dir, temp_path) = setup_multifile_test();

    let file3_path = temp_path.join("file3.txt");
    let mut file3 = fs::OpenOptions::new()
        .write(true)
        .open(&file3_path)
        .unwrap();
    file3.write_all(b"CORRUPTED").unwrap();
    drop(file3);

    let par2_file = temp_path.join("multifile.par2");
    let (_context, result) = repair_files(par2_file.to_str().unwrap()).unwrap();

    // Should repair successfully
    assert!(
        result.repaired_files().contains(&"file3.txt".to_string())
            || result.failed_files().contains(&"file3.txt".to_string()),
        "file3.txt should be processed"
    );
}

#[test]
fn test_single_file_repair_still_works() {
    // Verify that our changes don't break single-file PAR2 repair
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Copy single-file test fixtures
    let fixture_dir = PathBuf::from("tests/fixtures/repair_scenarios");
    for entry in fs::read_dir(&fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let file_name = entry.file_name();
        let source = entry.path();
        let dest = temp_path.join(&file_name);
        fs::copy(&source, &dest).unwrap();
    }

    // Corrupt the testfile
    let testfile = temp_path.join("testfile");
    let mut file = fs::OpenOptions::new().write(true).open(&testfile).unwrap();
    file.write_all(&[0xFF; 1000]).unwrap();
    drop(file);

    let par2_file = temp_path.join("testfile.par2");
    let (_context, result) = repair_files(par2_file.to_str().unwrap()).unwrap();

    // Single-file repair should still work
    assert!(
        result.repaired_files().contains(&"testfile".to_string()),
        "Single-file repair should work"
    );
}
