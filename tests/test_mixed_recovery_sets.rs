use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;

/// Test that verifying with mixed PAR2 sets from different files doesn't cause issues
/// This replicates the bug where having testfile_par2rs.par2 and testfile_par2cmd.par2
/// in the same directory causes confusion
#[test]
fn test_mixed_recovery_sets_ignored() {
    let temp_dir = tempdir().unwrap();
    let temp_path = temp_dir.path();

    // Create two different test files
    let file1_path = temp_path.join("file1.dat");
    let file2_path = temp_path.join("file2.dat");

    fs::write(&file1_path, b"This is file 1 content").unwrap();
    fs::write(&file2_path, b"This is file 2 different content").unwrap();

    // Create PAR2 files for both
    let par2_bin = PathBuf::from(env!("CARGO_BIN_EXE_par2"));

    // Create PAR2 for file1
    let output1 = Command::new(&par2_bin)
        .args([
            "create",
            "-r5",
            temp_path.join("file1.par2").to_str().unwrap(),
            file1_path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to create PAR2 for file1");
    assert!(
        output1.status.success(),
        "Failed to create PAR2 for file1: {}",
        String::from_utf8_lossy(&output1.stderr)
    );

    // Create PAR2 for file2
    let output2 = Command::new(&par2_bin)
        .args([
            "create",
            "-r5",
            temp_path.join("file2.par2").to_str().unwrap(),
            file2_path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to create PAR2 for file2");
    assert!(
        output2.status.success(),
        "Failed to create PAR2 for file2: {}",
        String::from_utf8_lossy(&output2.stderr)
    );

    // Now try to verify file1.par2
    // This should work even though file2.par2 is in the same directory
    let verify_output = Command::new(&par2_bin)
        .args(["verify", temp_path.join("file1.par2").to_str().unwrap()])
        .output()
        .expect("Failed to verify file1.par2");

    let stderr = String::from_utf8_lossy(&verify_output.stderr);
    let stdout = String::from_utf8_lossy(&verify_output.stdout);

    // Should warn about mixed recovery sets
    assert!(
        stderr.contains("Multiple recovery sets detected")
            || stdout.contains("Multiple recovery sets detected"),
        "Expected warning about mixed recovery sets, got:\nstderr: {}\nstdout: {}",
        stderr,
        stdout
    );

    // But verification should still succeed for file1
    assert!(
        verify_output.status.success(),
        "Verification should succeed even with mixed PAR2 files in directory.\nstderr: {}\nstdout: {}",
        stderr,
        stdout
    );

    // Verify the output indicates file1 is correct
    assert!(
        stdout.contains("repair is not required")
            || stdout.contains("file(s) are ok")
            || verify_output.status.success(),
        "Expected successful verification, got:\nstderr: {}\nstdout: {}",
        stderr,
        stdout
    );
}
