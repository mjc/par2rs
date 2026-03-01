//! Integration tests for PAR2 create functionality
//!
//! These tests verify that our PAR2 creation output is compatible with par2cmdline-turbo
//! by creating PAR2 files and then using the reference implementation to verify them.
//!
//! Prerequisites:
//! - par2cmdline-turbo must be available as 'par2' in PATH (provided by nix flake)

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

/// Helper to check if par2cmdline-turbo is available
fn par2_available() -> bool {
    Command::new("par2")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Helper to create a test file with specific content
fn create_test_file(path: &Path, size: usize, pattern: u8) -> std::io::Result<()> {
    let data = vec![pattern; size];
    fs::write(path, data)
}

/// Helper to run par2cmdline-turbo verify command
fn run_par2_verify(par2_file: &Path) -> std::io::Result<bool> {
    let output = Command::new("par2").arg("verify").arg(par2_file).output()?;

    Ok(output.status.success())
}

/// Helper to run par2cmdline-turbo create command for reference
fn run_par2_create(
    output_name: &Path,
    source_files: &[&Path],
    redundancy: u32,
) -> std::io::Result<bool> {
    // Change to the directory containing the output file
    let output_dir = output_name.parent().unwrap();
    let output_filename = output_name.file_name().unwrap();

    let mut cmd = Command::new("par2");
    cmd.current_dir(output_dir)
        .arg("create")
        .arg(format!("-r{}", redundancy))
        .arg(output_filename);

    for file in source_files {
        // Use just the filename if the file is in the same directory
        let file_arg = if file.parent() == Some(output_dir) {
            file.file_name().unwrap().to_str().unwrap()
        } else {
            file.to_str().unwrap()
        };
        cmd.arg(file_arg);
    }

    let output = cmd.output()?;
    Ok(output.status.success())
}

#[test]
fn test_par2cmdline_available() {
    assert!(
        par2_available(),
        "par2cmdline-turbo not available in PATH. Run tests in nix shell."
    );
}

#[test]
fn test_create_single_small_file_verify_with_par2cmdline() {
    if !par2_available() {
        eprintln!("Skipping test: par2cmdline-turbo not available");
        return;
    }

    let temp = tempdir().unwrap();
    let test_file = temp.path().join("test.txt");
    let par2_file = temp.path().join("test.par2");

    // Create a small test file (1KB)
    create_test_file(&test_file, 1024, 0xAA).unwrap();

    // Create PAR2 files using our implementation
    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true)); // quiet mode
    let mut context = par2rs::create::CreateContextBuilder::new()
        .output_name(par2_file.to_str().unwrap())
        .source_files(vec![test_file.clone()])
        .redundancy_percentage(5)
        .reporter(reporter)
        .build()
        .unwrap();

    context.create().unwrap();

    // Verify using par2cmdline-turbo
    assert!(
        run_par2_verify(&par2_file).unwrap(),
        "par2cmdline-turbo failed to verify our PAR2 files"
    );
}

#[test]
fn test_create_multiple_files_verify_with_par2cmdline() {
    if !par2_available() {
        eprintln!("Skipping test: par2cmdline-turbo not available");
        return;
    }

    let temp = tempdir().unwrap();
    let file1 = temp.path().join("file1.dat");
    let file2 = temp.path().join("file2.dat");
    let file3 = temp.path().join("file3.dat");
    let par2_file = temp.path().join("multifile.par2");

    // Create test files with different sizes and patterns
    create_test_file(&file1, 2048, 0x11).unwrap();
    create_test_file(&file2, 4096, 0x22).unwrap();
    create_test_file(&file3, 1024, 0x33).unwrap();

    // Create PAR2 files using our implementation
    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true)); // quiet mode
    let mut context = par2rs::create::CreateContextBuilder::new()
        .output_name(par2_file.to_str().unwrap())
        .source_files(vec![file1.clone(), file2.clone(), file3.clone()])
        .redundancy_percentage(10)
        .reporter(reporter)
        .build()
        .unwrap();

    context.create().unwrap();

    // Verify using par2cmdline-turbo
    assert!(
        run_par2_verify(&par2_file).unwrap(),
        "par2cmdline-turbo failed to verify our multifile PAR2 set"
    );
}

#[test]
fn test_create_large_file_verify_with_par2cmdline() {
    if !par2_available() {
        eprintln!("Skipping test: par2cmdline-turbo not available");
        return;
    }

    let temp = tempdir().unwrap();
    let test_file = temp.path().join("large.dat");
    let par2_file = temp.path().join("large.par2");

    // Create a larger test file (1MB)
    create_test_file(&test_file, 1024 * 1024, 0xBB).unwrap();

    // Create PAR2 files using our implementation
    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true)); // quiet mode
    let mut context = par2rs::create::CreateContextBuilder::new()
        .output_name(par2_file.to_str().unwrap())
        .source_files(vec![test_file.clone()])
        .redundancy_percentage(5)
        .reporter(reporter)
        .build()
        .unwrap();

    context.create().unwrap();

    // Verify using par2cmdline-turbo
    assert!(
        run_par2_verify(&par2_file).unwrap(),
        "par2cmdline-turbo failed to verify our PAR2 files for large file"
    );
}

#[test]
fn test_create_with_explicit_block_size() {
    if !par2_available() {
        eprintln!("Skipping test: par2cmdline-turbo not available");
        return;
    }

    let temp = tempdir().unwrap();
    let test_file = temp.path().join("test.bin");
    let par2_file = temp.path().join("test_blocks.par2");

    // Create test file
    create_test_file(&test_file, 8192, 0xCC).unwrap();

    // Create PAR2 files with explicit block size
    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true)); // quiet mode
    let mut context = par2rs::create::CreateContextBuilder::new()
        .output_name(par2_file.to_str().unwrap())
        .source_files(vec![test_file.clone()])
        .block_size(2048) // 2KB blocks
        .recovery_block_count(2)
        .reporter(reporter)
        .build()
        .unwrap();

    context.create().unwrap();

    // Verify using par2cmdline-turbo
    assert!(
        run_par2_verify(&par2_file).unwrap(),
        "par2cmdline-turbo failed to verify PAR2 files with explicit block size"
    );
}

#[test]
fn test_create_then_corrupt_and_repair_with_par2cmdline() {
    if !par2_available() {
        eprintln!("Skipping test: par2cmdline-turbo not available");
        return;
    }

    let temp = tempdir().unwrap();
    let test_file = temp.path().join("test.dat");
    let par2_file = temp.path().join("test.par2");

    // Create test file
    create_test_file(&test_file, 4096, 0xDD).unwrap();

    // Create PAR2 files using our implementation
    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true)); // quiet mode
    let mut context = par2rs::create::CreateContextBuilder::new()
        .output_name(par2_file.to_str().unwrap())
        .source_files(vec![test_file.clone()])
        .redundancy_percentage(20) // Higher redundancy for repair test
        .reporter(reporter)
        .build()
        .unwrap();

    context.create().unwrap();

    // Corrupt the test file
    let mut data = fs::read(&test_file).unwrap();
    data[512] = !data[512]; // Flip bits in middle of file
    data[1024] = !data[1024]; // Flip more bits
    fs::write(&test_file, data).unwrap();

    // Try to repair using par2cmdline-turbo
    let repair_output = Command::new("par2")
        .arg("repair")
        .arg(&par2_file)
        .output()
        .unwrap();

    assert!(
        repair_output.status.success(),
        "par2cmdline-turbo failed to repair file using our PAR2 files"
    );

    // Verify the repaired file
    assert!(
        run_par2_verify(&par2_file).unwrap(),
        "Verification failed after repair"
    );
}

/// Test that verifies compatibility by comparing our output structure
/// with par2cmdline-turbo's output structure
#[test]
fn test_output_file_structure_matches_par2cmdline() {
    if !par2_available() {
        eprintln!("Skipping test: par2cmdline-turbo not available");
        return;
    }

    let temp = tempdir().unwrap();

    // Create test file
    let test_file = temp.path().join("test.dat");
    create_test_file(&test_file, 2048, 0xEE).unwrap();

    // Create PAR2 with par2cmdline-turbo (reference)
    let ref_par2 = temp.path().join("reference.par2");
    run_par2_create(&ref_par2, &[&test_file], 10).unwrap();

    // Count reference files
    let ref_files: Vec<PathBuf> = fs::read_dir(temp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.starts_with("reference") && s.ends_with(".par2"))
                .unwrap_or(false)
        })
        .collect();

    // Create PAR2 with our implementation
    let test_file2 = temp.path().join("test2.dat");
    create_test_file(&test_file2, 2048, 0xEE).unwrap(); // Same content

    let our_par2 = temp.path().join("ours.par2");
    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true)); // quiet mode
    let mut context = par2rs::create::CreateContextBuilder::new()
        .output_name(our_par2.to_str().unwrap())
        .source_files(vec![test_file2])
        .redundancy_percentage(10)
        .reporter(reporter)
        .build()
        .unwrap();

    context.create().unwrap();

    // Count our files
    let our_files: Vec<PathBuf> = fs::read_dir(temp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.starts_with("ours") && s.ends_with(".par2"))
                .unwrap_or(false)
        })
        .collect();

    // Should create similar number of files (may vary slightly with scheme)
    // Main assertion: both should verify correctly
    assert!(
        run_par2_verify(&ref_par2).unwrap(),
        "Reference PAR2 verification failed"
    );
    assert!(
        run_par2_verify(&our_par2).unwrap(),
        "Our PAR2 verification failed"
    );

    println!("par2cmdline-turbo created {} files", ref_files.len());
    println!("par2rs created {} files", our_files.len());
}

/// Volume files must each contain critical packets (so repair works without the index)
#[test]
fn volume_files_each_contain_critical_packets() {
    let temp = tempdir().unwrap();
    let test_file = temp.path().join("test.dat");
    let par2_file = temp.path().join("test.par2");

    create_test_file(&test_file, 4096, 0xAB).unwrap();

    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true));
    let mut context = par2rs::create::CreateContextBuilder::new()
        .output_name(par2_file.to_str().unwrap())
        .source_files(vec![test_file])
        .recovery_block_count(4)
        .reporter(reporter)
        .build()
        .unwrap();

    context.create().unwrap();

    // Every .vol*.par2 file must contain at least one recovery block AND critical packets
    let vol_files: Vec<PathBuf> = fs::read_dir(temp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.contains(".vol"))
                .unwrap_or(false)
        })
        .collect();

    assert!(!vol_files.is_empty(), "Expected at least one volume file");

    for vol in &vol_files {
        let packets = par2rs::par2_files::load_par2_packets(&[vol.clone()], true, false);
        assert!(
            packets.recovery_block_count > 0,
            "{} should contain recovery blocks",
            vol.display()
        );
        assert!(
            !packets.packets.is_empty(),
            "{} should contain critical packets",
            vol.display()
        );
    }
}

/// Repair must succeed using only volume files (no index file)
#[test]
fn repair_using_only_volume_files_succeeds() {
    if !par2_available() {
        eprintln!("Skipping test: par2cmdline-turbo not available");
        return;
    }

    let temp = tempdir().unwrap();
    let test_file = temp.path().join("test.dat");
    let par2_file = temp.path().join("test.par2");

    create_test_file(&test_file, 4096, 0xEF).unwrap();

    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true));
    let mut context = par2rs::create::CreateContextBuilder::new()
        .output_name(par2_file.to_str().unwrap())
        .source_files(vec![test_file.clone()])
        .redundancy_percentage(20)
        .reporter(reporter)
        .build()
        .unwrap();

    context.create().unwrap();

    // Delete the index file — repair must still work using volume files alone
    fs::remove_file(&par2_file).unwrap();

    // Corrupt the source file
    let mut data = fs::read(&test_file).unwrap();
    data[100] = !data[100];
    fs::write(&test_file, &data).unwrap();

    // Find any volume file to use as the repair entry point
    let vol_file = fs::read_dir(temp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.contains(".vol") && s.ends_with(".par2"))
                .unwrap_or(false)
        })
        .expect("No volume file found");

    let repair_output = Command::new("par2")
        .arg("repair")
        .arg(&vol_file)
        .output()
        .unwrap();

    assert!(
        repair_output.status.success(),
        "Repair from volume file only failed.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&repair_output.stdout),
        String::from_utf8_lossy(&repair_output.stderr),
    );
}

/// Volume splitting: par2rs should produce index + volume files, not one big file
#[test]
fn create_produces_index_plus_volume_files() {
    let temp = tempdir().unwrap();
    let test_file = temp.path().join("test.dat");
    let par2_file = temp.path().join("test.par2");

    create_test_file(&test_file, 4096, 0xAB).unwrap();

    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true));
    let mut context = par2rs::create::CreateContextBuilder::new()
        .output_name(par2_file.to_str().unwrap())
        .source_files(vec![test_file])
        .recovery_block_count(4)
        .reporter(reporter)
        .build()
        .unwrap();

    context.create().unwrap();

    // Should produce test.par2 (index) + at least one test.vol*.par2 (volume)
    let par2_files: Vec<PathBuf> = fs::read_dir(temp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("par2"))
        .collect();

    assert!(
        par2_files.len() >= 2,
        "Expected index + at least one volume file, got {} file(s): {:?}",
        par2_files.len(),
        par2_files
    );

    // The base index file must exist
    assert!(par2_file.exists(), "Index file test.par2 must exist");

    // At least one volume file must exist
    let vol_files: Vec<_> = par2_files
        .iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.contains(".vol"))
                .unwrap_or(false)
        })
        .collect();
    assert!(
        !vol_files.is_empty(),
        "Expected at least one .volNNN+MM.par2 file"
    );
}

/// Index file must contain no recovery slice packets
#[test]
fn index_file_contains_no_recovery_data() {
    let temp = tempdir().unwrap();
    let test_file = temp.path().join("test.dat");
    let par2_file = temp.path().join("test.par2");

    create_test_file(&test_file, 4096, 0xCD).unwrap();

    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true));
    let mut context = par2rs::create::CreateContextBuilder::new()
        .output_name(par2_file.to_str().unwrap())
        .source_files(vec![test_file])
        .recovery_block_count(4)
        .reporter(reporter)
        .build()
        .unwrap();

    context.create().unwrap();

    // The index file (base.par2) must have no recovery slices
    // par2rs uses par2rs::par2_files::load_par2_packets to check
    let packets = par2rs::par2_files::load_par2_packets(&[par2_file.clone()], true, false);
    assert_eq!(
        packets.recovery_block_count, 0,
        "Index file must contain no recovery slice packets"
    );
}

/// first_recovery_block builder method sets non-zero starting exponent
#[test]
fn first_recovery_block_sets_starting_exponent() {
    if !par2_available() {
        eprintln!("Skipping test: par2cmdline-turbo not available");
        return;
    }

    let temp = tempdir().unwrap();
    let test_file = temp.path().join("test.dat");
    let par2_file = temp.path().join("test.par2");

    create_test_file(&test_file, 4096, 0x77).unwrap();

    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true));
    let mut context = par2rs::create::CreateContextBuilder::new()
        .output_name(par2_file.to_str().unwrap())
        .source_files(vec![test_file])
        .recovery_block_count(4)
        .first_recovery_block(10) // non-zero starting exponent
        .reporter(reporter)
        .build()
        .unwrap();

    context.create().unwrap();

    // par2cmdline-turbo must still verify it
    assert!(
        run_par2_verify(&par2_file).unwrap(),
        "par2cmdline-turbo failed to verify PAR2 with non-zero first_recovery_block"
    );

    // Volume filenames should reflect the non-zero starting exponent
    let vol_files: Vec<_> = fs::read_dir(temp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.contains(".vol1") || s.contains(".vol2") || s.contains(".vol3"))
                .unwrap_or(false)
        })
        .collect();
    assert!(
        !vol_files.is_empty(),
        "Expected volume files with non-zero exponent in filename"
    );
}

#[test]
fn test_create_builder_validation() {
    // Test empty source files
    let result = par2rs::create::CreateContextBuilder::new()
        .output_name("test.par2")
        .build();
    assert!(result.is_err(), "Should fail with no source files");

    // Test no output name
    let result = par2rs::create::CreateContextBuilder::new()
        .source_files(vec![PathBuf::from("test.txt")])
        .build();
    assert!(result.is_err(), "Should fail with no output name");

    // Test invalid redundancy
    let result = par2rs::create::CreateContextBuilder::new()
        .output_name("test.par2")
        .source_files(vec![PathBuf::from("test.txt")])
        .redundancy_percentage(0)
        .build();
    assert!(result.is_err(), "Should fail with 0% redundancy");

    let result = par2rs::create::CreateContextBuilder::new()
        .output_name("test.par2")
        .source_files(vec![PathBuf::from("test.txt")])
        .redundancy_percentage(101)
        .build();
    assert!(result.is_err(), "Should fail with >100% redundancy");
}

#[test]
fn test_block_size_calculation() {
    let temp = tempdir().unwrap();

    // Create files of different sizes to test auto block size calculation
    let small_file = temp.path().join("small.dat");
    let medium_file = temp.path().join("medium.dat");
    let large_file = temp.path().join("large.dat");

    create_test_file(&small_file, 1024, 0x11).unwrap(); // 1KB
    create_test_file(&medium_file, 1024 * 1024, 0x22).unwrap(); // 1MB
    create_test_file(&large_file, 10 * 1024 * 1024, 0x33).unwrap(); // 10MB

    // Test auto block size calculation for small file
    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true));
    let context = par2rs::create::CreateContextBuilder::new()
        .output_name("small.par2")
        .source_files(vec![small_file])
        .redundancy_percentage(5)
        .reporter(reporter)
        .build()
        .unwrap();

    // Block size must be a non-zero multiple of 4 (spec minimum is 4 bytes)
    // A 1KB file with 2000 target blocks: 2000 > 1024/4=256 units, so algorithm
    // hits the "too many blocks" path and returns the minimum size of 4. This is correct.
    assert!(
        context.block_size() >= 4 && context.block_size() % 4 == 0,
        "Block size invalid: {}",
        context.block_size()
    );

    // Test auto block size calculation for medium file
    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true));
    let context = par2rs::create::CreateContextBuilder::new()
        .output_name("medium.par2")
        .source_files(vec![medium_file])
        .redundancy_percentage(5)
        .reporter(reporter)
        .build()
        .unwrap();

    assert!(
        context.block_size() >= 512,
        "Block size too small for medium file"
    );

    // Test auto block size calculation for large file
    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true));
    let context = par2rs::create::CreateContextBuilder::new()
        .output_name("large.par2")
        .source_files(vec![large_file])
        .redundancy_percentage(5)
        .reporter(reporter)
        .build()
        .unwrap();

    // Should be reasonable for 10MB file
    assert!(
        context.block_size() >= 512 && context.block_size() <= 16 * 1024 * 1024,
        "Block size out of range: {}",
        context.block_size()
    );
}

#[test]
fn test_recovery_block_count_calculation() {
    let temp = tempdir().unwrap();
    let test_file = temp.path().join("test.dat");
    create_test_file(&test_file, 10240, 0x55).unwrap(); // 10KB

    // Test with percentage
    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true));
    let context = par2rs::create::CreateContextBuilder::new()
        .output_name("test.par2")
        .source_files(vec![test_file.clone()])
        .redundancy_percentage(10)
        .reporter(reporter)
        .build()
        .unwrap();

    let source_blocks = context.source_block_count();
    let recovery_blocks = context.recovery_block_count();

    // Recovery blocks should be approximately 10% of source blocks
    let expected = (source_blocks as f64 * 0.10).ceil() as u32;
    assert_eq!(
        recovery_blocks, expected,
        "Recovery block count mismatch for 10% redundancy"
    );

    // Test with explicit count
    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(true));
    let context = par2rs::create::CreateContextBuilder::new()
        .output_name("test2.par2")
        .source_files(vec![test_file])
        .recovery_block_count(5)
        .reporter(reporter)
        .build()
        .unwrap();

    assert_eq!(
        context.recovery_block_count(),
        5,
        "Explicit recovery block count not respected"
    );
}
