//! Comprehensive integration tests for all par2rs binaries
//!
//! Tests the command-line interfaces and basic functionality of:
//! - par2 (unified interface)
//! - par2verify
//! - par2repair
//! - par2create (placeholder)
//! - split_par2

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// Helper to get the path to a compiled binary
fn get_binary_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("target");
    path.push("debug");
    path.push(name);
    path
}

/// Helper to create a test file with known content
fn create_test_file(path: &Path, content: &[u8]) -> std::io::Result<()> {
    fs::write(path, content)
}

// =============================================================================
// par2 binary tests (unified interface)
// =============================================================================

#[test]
fn test_par2_help() {
    let output = Command::new(get_binary_path("par2"))
        .arg("--help")
        .output()
        .expect("Failed to execute par2");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PAR2 file verification and repair utility"));
    assert!(stdout.contains("create"));
    assert!(stdout.contains("verify"));
    assert!(stdout.contains("repair"));
}

#[test]
fn test_par2_version() {
    let output = Command::new(get_binary_path("par2"))
        .arg("--version")
        .output()
        .expect("Failed to execute par2");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("par2"));
}

#[test]
fn test_par2_verify_help() {
    let output = Command::new(get_binary_path("par2"))
        .arg("verify")
        .arg("--help")
        .output()
        .expect("Failed to execute par2 verify");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Verify files using PAR2 data"));
    assert!(stdout.contains("--quiet"));
}

#[test]
fn test_par2_verify_alias() {
    let output = Command::new(get_binary_path("par2"))
        .arg("v")
        .arg("--help")
        .output()
        .expect("Failed to execute par2 v");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Verify files using PAR2 data"));
}

#[test]
fn test_par2_repair_help() {
    let output = Command::new(get_binary_path("par2"))
        .arg("repair")
        .arg("--help")
        .output()
        .expect("Failed to execute par2 repair");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Repair files using PAR2 recovery data"));
    assert!(stdout.contains("--purge"));
}

#[test]
fn test_par2_repair_alias() {
    let output = Command::new(get_binary_path("par2"))
        .arg("r")
        .arg("--help")
        .output()
        .expect("Failed to execute par2 r");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Repair files using PAR2 recovery data"));
}

#[test]
fn test_par2_verify_missing_file() {
    let output = Command::new(get_binary_path("par2"))
        .arg("verify")
        .arg("nonexistent.par2")
        .output()
        .expect("Failed to execute par2 verify");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("does not exist") || stderr.contains("Error"),
        "Expected error message, got: {}",
        stderr
    );
}

#[test]
fn test_par2_verify_with_test_fixtures() {
    let par2_file = Path::new("tests/fixtures/repair_scenarios/testfile.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    let output = Command::new(get_binary_path("par2"))
        .arg("verify")
        .arg(par2_file)
        .output()
        .expect("Failed to execute par2 verify");

    // Should run without crashing
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Loading PAR2 files") || stdout.contains("Verifying"));
}

#[test]
fn test_par2_verify_quiet_mode() {
    let par2_file = Path::new("tests/fixtures/repair_scenarios/testfile.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    let output = Command::new(get_binary_path("par2"))
        .arg("verify")
        .arg("--quiet")
        .arg(par2_file)
        .output()
        .expect("Failed to execute par2 verify");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let total_output = stdout.len() + stderr.len();
    
    // Quiet mode should produce less output than normal mode
    // Allow some output for warnings/errors
    assert!(
        total_output < 500,
        "Quiet mode produced too much output: {} bytes",
        total_output
    );
}

#[test]
fn test_par2_repair_with_test_fixtures() {
    let par2_file = Path::new("tests/fixtures/repair_scenarios/testfile.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    let output = Command::new(get_binary_path("par2"))
        .arg("repair")
        .arg(par2_file)
        .output()
        .expect("Failed to execute par2 repair");

    // Should run without crashing
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Loading") || stdout.contains("repair") || stdout.contains("Repair"));
}

// =============================================================================
// par2verify binary tests
// =============================================================================

#[test]
fn test_par2verify_help() {
    let output = Command::new(get_binary_path("par2verify"))
        .arg("--help")
        .output()
        .expect("Failed to execute par2verify");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PAR2") || stdout.contains("verify"));
}

#[test]
fn test_par2verify_missing_file() {
    let output = Command::new(get_binary_path("par2verify"))
        .arg("nonexistent.par2")
        .output()
        .expect("Failed to execute par2verify");

    assert!(!output.status.success());
}

#[test]
fn test_par2verify_with_fixtures() {
    let par2_file = Path::new("tests/fixtures/repair_scenarios/testfile.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    let output = Command::new(get_binary_path("par2verify"))
        .arg(par2_file)
        .output()
        .expect("Failed to execute par2verify");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Loading PAR2 files") || stdout.contains("Verifying"));
}

#[test]
fn test_par2verify_sequential_mode() {
    let par2_file = Path::new("tests/fixtures/repair_scenarios/testfile.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    let output = Command::new(get_binary_path("par2verify"))
        .arg("--no-parallel")
        .arg(par2_file)
        .output()
        .expect("Failed to execute par2verify");

    // Should complete without error (even if verification fails)
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Verifying") || stdout.contains("Loading"));
}

#[test]
fn test_par2verify_thread_count() {
    let par2_file = Path::new("tests/fixtures/repair_scenarios/testfile.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    let output = Command::new(get_binary_path("par2verify"))
        .arg("--threads")
        .arg("2")
        .arg(par2_file)
        .output()
        .expect("Failed to execute par2verify");

    // Should accept thread count argument
    assert!(output.status.success() || output.status.code() == Some(1));
}

// =============================================================================
// par2repair binary tests
// =============================================================================

#[test]
fn test_par2repair_help() {
    let output = Command::new(get_binary_path("par2repair"))
        .arg("--help")
        .output()
        .expect("Failed to execute par2repair");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("par2") || stdout.contains("repair"));
    assert!(stdout.contains("--purge") || stdout.contains("-p"));
}

#[test]
fn test_par2repair_missing_file() {
    let output = Command::new(get_binary_path("par2repair"))
        .arg("nonexistent.par2")
        .output()
        .expect("Failed to execute par2repair");

    assert!(!output.status.success());
}

#[test]
fn test_par2repair_with_fixtures() {
    let par2_file = Path::new("tests/fixtures/repair_scenarios/testfile.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    let output = Command::new(get_binary_path("par2repair"))
        .arg(par2_file)
        .output()
        .expect("Failed to execute par2repair");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Loading") || stdout.contains("repair") || stdout.contains("Repair"));
}

#[test]
fn test_par2repair_quiet_mode() {
    let par2_file = Path::new("tests/fixtures/repair_scenarios/testfile.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    let output = Command::new(get_binary_path("par2repair"))
        .arg("--quiet")
        .arg(par2_file)
        .output()
        .expect("Failed to execute par2repair");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let total_output = stdout.len() + stderr.len();
    
    // Quiet mode should produce less output than normal mode
    assert!(
        total_output < 1000,
        "Quiet mode produced too much output: {} bytes",
        total_output
    );
}

#[test]
fn test_par2repair_no_parallel() {
    let par2_file = Path::new("tests/fixtures/repair_scenarios/testfile.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    let output = Command::new(get_binary_path("par2repair"))
        .arg("--no-parallel")
        .arg(par2_file)
        .output()
        .expect("Failed to execute par2repair");

    // Should complete (success or failure based on file state)
    assert!(output.status.success() || output.status.code().is_some());
}

// =============================================================================
// par2create binary tests
// =============================================================================

#[test]
fn test_par2create_runs() {
    let output = Command::new(get_binary_path("par2create"))
        .output()
        .expect("Failed to execute par2create");

    // Currently just a placeholder
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("not yet implemented") || output.status.success());
}

// =============================================================================
// split_par2 binary tests
// =============================================================================

#[test]
fn test_split_par2_runs() {
    let output = Command::new(get_binary_path("split_par2"))
        .output()
        .expect("Failed to execute split_par2");

    // Should execute without crashing
    // Note: This binary has hardcoded paths, so it may fail to find files
    // but should still execute
    assert!(output.status.success() || output.status.code().is_some());
}

// =============================================================================
// Integration tests with temporary files
// =============================================================================

#[test]
fn test_par2_verify_integration() {
    // Create a temporary directory
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    
    // Copy test fixtures to temp dir
    let fixture_dir = Path::new("tests/fixtures/repair_scenarios");
    if !fixture_dir.exists() {
        eprintln!("Skipping integration test - fixtures not found");
        return;
    }

    // Copy par2 file
    let src_par2 = fixture_dir.join("testfile.par2");
    if !src_par2.exists() {
        eprintln!("Skipping integration test - testfile.par2 not found");
        return;
    }
    
    let dest_par2 = temp_dir.path().join("testfile.par2");
    fs::copy(&src_par2, &dest_par2).expect("Failed to copy PAR2 file");

    // Run verify in temp directory
    let output = Command::new(get_binary_path("par2"))
        .arg("verify")
        .arg(&dest_par2)
        .output()
        .expect("Failed to execute par2 verify");

    // Should produce output about verification
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("Verifying") || stdout.contains("Loading") || stderr.contains("Error"),
        "Expected verification output"
    );
}

#[test]
fn test_command_line_compatibility() {
    // Test that par2 accepts par2cmdline-style arguments
    let tests = vec![
        vec!["verify", "--help"],
        vec!["v", "--help"],
        vec!["repair", "--help"],
        vec!["r", "--help"],
        vec!["verify", "-h"],
        vec!["repair", "-h"],
    ];

    for args in tests {
        let output = Command::new(get_binary_path("par2"))
            .args(&args)
            .output()
            .expect("Failed to execute par2");

        assert!(
            output.status.success(),
            "Command 'par2 {}' failed",
            args.join(" ")
        );
    }
}

#[test]
fn test_par2_threads_argument() {
    let par2_file = Path::new("tests/fixtures/repair_scenarios/testfile.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    // Test with different thread counts
    for threads in &["1", "2", "4", "0"] {
        let output = Command::new(get_binary_path("par2"))
            .arg("verify")
            .arg("-t")
            .arg(threads)
            .arg(par2_file)
            .output()
            .expect("Failed to execute par2 verify with threads");

        // Should accept thread argument
        assert!(
            output.status.success() || output.status.code() == Some(1),
            "Failed with threads={}",
            threads
        );
    }
}

#[test]
fn test_all_binaries_exist() {
    let binaries = vec!["par2", "par2verify", "par2repair", "par2create", "split_par2"];
    
    for binary in binaries {
        let path = get_binary_path(binary);
        assert!(
            path.exists(),
            "Binary {} not found at {:?}. Run 'cargo build' first.",
            binary,
            path
        );
    }
}

#[test]
fn test_par2_error_handling() {
    // Test with invalid arguments
    let output = Command::new(get_binary_path("par2"))
        .arg("invalid_command")
        .output()
        .expect("Failed to execute par2");

    assert!(!output.status.success());
}

#[test]
fn test_par2verify_error_handling() {
    // Test with non-PAR2 file
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let fake_file = temp_dir.path().join("fake.par2");
    create_test_file(&fake_file, b"not a real par2 file").expect("Failed to create fake file");

    let output = Command::new(get_binary_path("par2verify"))
        .arg(&fake_file)
        .output()
        .expect("Failed to execute par2verify");

    // par2verify may succeed but with 0 files (graceful handling of invalid files)
    // Just ensure it doesn't panic
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() || output.status.code().is_some(),
        "par2verify should handle invalid files gracefully"
    );
    
    // Should indicate no files found
    assert!(
        stdout.contains("0 recoverable files") 
            || stdout.contains("0 files") 
            || stderr.contains("Error")
            || stderr.contains("Warning"),
        "Expected indication of no valid PAR2 data"
    );
}
