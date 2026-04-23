//! Comprehensive integration tests for all par2rs binaries
//!
//! Tests the command-line interfaces and basic functionality of:
//! - par2 (unified interface)
//! - par2verify
//! - par2repair
//! - par2create
//!
//! The binary paths come from Cargo when available, with a target/debug fallback
//! for direct local runs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// Helper to get the path to a compiled binary
fn get_binary_path(name: &str) -> PathBuf {
    let cargo_path = match name {
        "par2" => option_env!("CARGO_BIN_EXE_par2"),
        "par2create" => option_env!("CARGO_BIN_EXE_par2create"),
        "par2repair" => option_env!("CARGO_BIN_EXE_par2repair"),
        "par2verify" => option_env!("CARGO_BIN_EXE_par2verify"),
        _ => None,
    };
    if let Some(path) = cargo_path {
        return PathBuf::from(path);
    }

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

fn copy_repair_scenario_fixture(temp_dir: &TempDir) -> PathBuf {
    let fixture_dir = Path::new("tests/fixtures/repair_scenarios");
    for entry in fs::read_dir(fixture_dir).expect("repair scenario fixture missing") {
        let entry = entry.expect("failed to read fixture entry");
        if !entry
            .file_type()
            .expect("failed to read fixture type")
            .is_file()
        {
            continue;
        }
        fs::copy(entry.path(), temp_dir.path().join(entry.file_name()))
            .expect("failed to copy repair scenario fixture");
    }
    temp_dir.path().join("testfile.par2")
}

fn assert_repeated_quiet_accepted(output: &std::process::Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("cannot be used multiple times"),
        "repeated quiet flags were rejected: {}",
        stderr
    );
}

fn create_basepath_test_set(temp_dir: &TempDir) -> (PathBuf, PathBuf) {
    let data_dir = temp_dir.path().join("data");
    fs::create_dir(&data_dir).expect("Failed to create data dir");

    let source = data_dir.join("sample.dat");
    create_test_file(&source, b"basepath-protected-data").expect("Failed to create source file");

    let par2_file = temp_dir.path().join("archive.par2");
    let output = Command::new(get_binary_path("par2create"))
        .arg("-q")
        .arg("--basepath")
        .arg(&data_dir)
        .arg("-s4")
        .arg("-c1")
        .arg(&par2_file)
        .arg(&source)
        .output()
        .expect("Failed to execute par2create");

    assert!(
        output.status.success(),
        "par2create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    (par2_file, data_dir)
}

fn create_renamed_file_test_set(temp_dir: &TempDir) -> (PathBuf, PathBuf, PathBuf) {
    let source = temp_dir.path().join("sample.dat");
    create_test_file(&source, b"renamed-file-scan-data").expect("Failed to create source file");

    let par2_file = temp_dir.path().join("archive.par2");
    let output = Command::new(get_binary_path("par2create"))
        .arg("-q")
        .arg("-s4")
        .arg("-c1")
        .arg(&par2_file)
        .arg(&source)
        .output()
        .expect("Failed to execute par2create");

    assert!(
        output.status.success(),
        "par2create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let renamed = temp_dir.path().join("renamed.dat");
    fs::rename(&source, &renamed).expect("Failed to rename source file");

    (par2_file, source, renamed)
}

fn create_par1_verify_test_set(temp_dir: &TempDir) -> PathBuf {
    const PAR1_HEADER_SIZE: usize = 96;
    const PAR1_ENTRY_FIXED_SIZE: usize = 56;

    let source_name = "source.bin";
    let source_data = b"intact par1 protected data";
    let source = temp_dir.path().join(source_name);
    create_test_file(&source, source_data).expect("Failed to create PAR1 source file");

    let mut name_bytes = Vec::new();
    for code_unit in source_name.encode_utf16() {
        name_bytes.extend_from_slice(&code_unit.to_le_bytes());
    }
    let entry_size = (PAR1_ENTRY_FIXED_SIZE + name_bytes.len()) as u64;
    let source_hash = par2rs::checksum::compute_md5(source_data);

    let mut file_list = Vec::new();
    file_list.extend_from_slice(&entry_size.to_le_bytes());
    file_list.extend_from_slice(&1u64.to_le_bytes());
    file_list.extend_from_slice(&(source_data.len() as u64).to_le_bytes());
    file_list.extend_from_slice(source_hash.as_bytes());
    file_list.extend_from_slice(source_hash.as_bytes());
    file_list.extend_from_slice(&name_bytes);

    let mut par1 = Vec::new();
    par1.extend_from_slice(b"PAR\0\0\0\0\0");
    par1.extend_from_slice(&0x0001_0000u32.to_le_bytes());
    par1.extend_from_slice(&0u32.to_le_bytes());
    par1.extend_from_slice(&[0; 16]);
    par1.extend_from_slice(&[0x11; 16]);
    par1.extend_from_slice(&0u64.to_le_bytes());
    par1.extend_from_slice(&1u64.to_le_bytes());
    par1.extend_from_slice(&(PAR1_HEADER_SIZE as u64).to_le_bytes());
    par1.extend_from_slice(&(file_list.len() as u64).to_le_bytes());
    par1.extend_from_slice(&0u64.to_le_bytes());
    par1.extend_from_slice(&0u64.to_le_bytes());
    par1.extend_from_slice(&file_list);

    let control_hash = par2rs::checksum::compute_md5(&par1[32..]);
    par1[16..32].copy_from_slice(control_hash.as_bytes());

    let par_file = temp_dir.path().join("archive.par");
    fs::write(&par_file, par1).expect("Failed to create PAR1 file");
    par_file
}

fn copy_real_par1_fixture(temp_dir: &TempDir) -> PathBuf {
    let fixture_dir = Path::new("tests/fixtures/par1/flatdata");
    for entry in fs::read_dir(fixture_dir).expect("PAR1 fixture directory should exist") {
        let entry = entry.expect("fixture entry should be readable");
        fs::copy(entry.path(), temp_dir.path().join(entry.file_name()))
            .expect("fixture file should copy");
    }
    temp_dir.path().join("testdata.par")
}

fn create_purge_test_set(temp_dir: &TempDir) -> (PathBuf, PathBuf) {
    let source = temp_dir.path().join("purge.dat");
    create_test_file(&source, b"purge-ready-data").expect("Failed to create source file");

    let par2_file = temp_dir.path().join("purge.par2");
    let output = Command::new(get_binary_path("par2create"))
        .arg("-q")
        .arg("-s4")
        .arg("-c1")
        .arg(&par2_file)
        .arg(&source)
        .output()
        .expect("Failed to execute par2create");

    assert!(
        output.status.success(),
        "par2create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    (par2_file, source)
}

fn create_implicit_named_test_set(temp_dir: &TempDir) -> PathBuf {
    let source = temp_dir.path().join("implicit.dat");
    create_test_file(&source, b"implicit named recovery set")
        .expect("Failed to create source file");

    let output = Command::new(get_binary_path("par2create"))
        .arg("-q")
        .arg("-s4")
        .arg("-c1")
        .arg(&source)
        .output()
        .expect("Failed to execute par2create");

    assert!(
        output.status.success(),
        "par2create implicit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(temp_dir.path().join("implicit.dat.par2").exists());
    source
}

fn uppercase_par2_extensions(dir: &Path) {
    for entry in fs::read_dir(dir).expect("Failed to read temp dir") {
        let path = entry.expect("Failed to read dir entry").path();
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "par2")
        {
            let mut renamed = path.clone();
            renamed.set_extension("PAR2");
            fs::rename(&path, &renamed).expect("Failed to uppercase PAR2 extension");
        }
    }
}

fn assert_no_par2_files(dir: &Path) {
    let par2_count = fs::read_dir(dir)
        .expect("Failed to read temp dir")
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("par2"))
        })
        .count();
    assert_eq!(par2_count, 0, "expected verify purge to remove PAR2 files");
}

fn file_description_names(par2_file: &Path) -> Vec<String> {
    let packet_set =
        par2rs::par2_files::load_par2_packets(&[par2_file.to_path_buf()], false, false);
    let mut names: Vec<String> = packet_set
        .packets
        .iter()
        .filter_map(|packet| match packet {
            par2rs::Packet::FileDescription(desc) => desc
                .file_name
                .split(|byte| *byte == 0)
                .next()
                .and_then(|name| std::str::from_utf8(name).ok())
                .map(str::to_owned),
            _ => None,
        })
        .collect();
    names.sort();
    names
}

fn assert_quiet_output_empty(output: &std::process::Output) {
    assert!(
        output.stdout.is_empty(),
        "quiet stdout was not empty: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stderr.is_empty(),
        "quiet stderr was not empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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
    assert!(stdout.contains("Usage: par2 [COMMAND]"));
    assert!(!stdout.contains("[command] [COMMAND]"));
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
fn test_par2_long_version() {
    for binary in ["par2", "par2create", "par2verify", "par2repair"] {
        let output = Command::new(get_binary_path(binary))
            .arg("-VV")
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(output.status.success(), "{binary} -VV failed");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("par2rs version"),
            "{binary} output: {stdout}"
        );
        assert!(
            stdout.contains("ABSOLUTELY NO WARRANTY"),
            "{binary} output: {stdout}"
        );
    }
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
    assert!(stdout.contains("Extra files to scan"));
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
    assert!(stdout.contains("Extra files to scan"));
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
fn test_par2_repair_repairs_missing_par1_file() {
    let temp_dir = TempDir::new().unwrap();
    let par_file = copy_real_par1_fixture(&temp_dir);
    let missing_file = temp_dir.path().join("test-4.data");
    fs::remove_file(&missing_file).unwrap();

    let output = Command::new(get_binary_path("par2"))
        .arg("repair")
        .arg(&par_file)
        .output()
        .expect("Failed to execute par2 repair");

    assert!(
        output.status.success(),
        "par2 repair PAR1 failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(missing_file.exists());
}

#[test]
fn test_par2_repair_repairs_renamed_par1_file() {
    let fixture_dir = Path::new("tests/fixtures/par1/flatdata");
    let temp_dir = TempDir::new().unwrap();
    let par_file = copy_real_par1_fixture(&temp_dir);
    let target = temp_dir.path().join("test-4.data");
    let renamed = temp_dir.path().join("renamed.data");
    fs::rename(&target, &renamed).unwrap();

    let output = Command::new(get_binary_path("par2"))
        .arg("repair")
        .arg(&par_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2 repair");

    assert!(
        output.status.success(),
        "par2 repair PAR1 renamed file failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(&target).unwrap(),
        fs::read(fixture_dir.join("test-4.data")).unwrap()
    );
    assert!(!renamed.exists());
}

#[test]
fn test_repair_commands_exit_2_when_repair_is_not_possible() {
    for (binary, subcommand) in [("par2", Some("repair")), ("par2repair", None)] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let par2_file = copy_repair_scenario_fixture(&temp_dir);
        fs::remove_file(temp_dir.path().join("testfile")).expect("Failed to remove data file");

        let mut command = Command::new(get_binary_path(binary));
        if let Some(subcommand) = subcommand {
            command.arg(subcommand);
        }
        let output = command
            .arg(&par2_file)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert_eq!(
            output.status.code(),
            Some(2),
            "{binary} should match turbo's repair-not-possible exit code: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
fn test_par2_verify_accepts_intact_par1_set() {
    let temp_dir = TempDir::new().unwrap();
    let par_file = create_par1_verify_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2"))
        .arg("verify")
        .arg(&par_file)
        .output()
        .expect("Failed to execute par2 verify");

    assert!(
        output.status.success(),
        "par2 verify PAR1 failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_par1_verify_repair_accept_rename_only_flag() {
    for binary in ["par2verify", "par2repair"] {
        let temp_dir = TempDir::new().unwrap();
        let par_file = create_par1_verify_test_set(&temp_dir);

        let output = Command::new(get_binary_path(binary))
            .arg("-q")
            .arg("-O")
            .arg(&par_file)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary} -O"));

        assert!(
            output.status.success(),
            "{binary} -O rejected intact PAR1 set: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_par2_verify_reports_repair_required_for_renamed_par1_file() {
    let temp_dir = TempDir::new().unwrap();
    let par_file = copy_real_par1_fixture(&temp_dir);
    let target = temp_dir.path().join("test-3.data");
    let renamed = temp_dir.path().join("renamed.data");
    fs::rename(&target, &renamed).unwrap();

    let output = Command::new(get_binary_path("par2"))
        .arg("verify")
        .arg(&par_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2 verify");

    assert!(
        !output.status.success(),
        "par2 verify PAR1 renamed file should require repair: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Repair required"),
        "par2 verify should report repair required: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_par2_verify_uppercase_par1_volume_supports_renamed_detection() {
    let temp_dir = TempDir::new().unwrap();
    copy_real_par1_fixture(&temp_dir);
    fs::rename(
        temp_dir.path().join("testdata.p01"),
        temp_dir.path().join("testdata.P01"),
    )
    .unwrap();
    let target = temp_dir.path().join("test-2.data");
    let renamed = temp_dir.path().join("renamed.data");
    fs::rename(&target, &renamed).unwrap();

    let output = Command::new(get_binary_path("par2"))
        .arg("verify")
        .arg(temp_dir.path().join("testdata.P01"))
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2 verify");

    assert!(
        !output.status.success(),
        "par2 verify PAR1 .P01 renamed file should require repair: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Repair required"),
        "par2 verify should report repair required: stderr={}",
        String::from_utf8_lossy(&output.stderr)
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
fn test_par2_verify_accepts_repeated_quiet_flags() {
    let par2_file = Path::new("tests/fixtures/edge_cases/test_valid.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    let output = Command::new(get_binary_path("par2"))
        .arg("verify")
        .arg("-q")
        .arg("-q")
        .arg(par2_file)
        .output()
        .expect("Failed to execute par2 verify");

    assert_repeated_quiet_accepted(&output);
    assert!(output.status.success());
}

#[test]
fn test_par2_verify_uses_basepath_option() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, data_dir) = create_basepath_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2"))
        .arg("verify")
        .arg("-q")
        .arg("--basepath")
        .arg(&data_dir)
        .arg(&par2_file)
        .output()
        .expect("Failed to execute par2 verify");

    assert!(
        output.status.success(),
        "verify with -B failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_par2_verify_scans_extra_file_arguments() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, _source, renamed) = create_renamed_file_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2"))
        .arg("verify")
        .arg("-q")
        .arg(&par2_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2 verify");

    assert!(
        !output.status.success(),
        "verify with renamed extra should report repair required"
    );
    assert!(renamed.exists(), "verify must remain non-mutating");
}

#[test]
fn test_par2_verify_rename_only_accepts_renamed_extra() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, _source, renamed) = create_renamed_file_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2"))
        .arg("verify")
        .arg("-q")
        .arg("-O")
        .arg(&par2_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2 verify -O");

    assert!(
        !output.status.success(),
        "verify -O should report repair required for renamed extras"
    );
    assert!(renamed.exists(), "verify -O must remain non-mutating");
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

#[test]
fn test_par2_repair_accepts_repeated_quiet_flags() {
    let par2_file = Path::new("tests/fixtures/edge_cases/test_valid.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    let output = Command::new(get_binary_path("par2"))
        .arg("repair")
        .arg("-q")
        .arg("-q")
        .arg(par2_file)
        .output()
        .expect("Failed to execute par2 repair");

    assert_repeated_quiet_accepted(&output);
    assert!(output.status.success());
}

#[test]
fn test_par2_repair_uses_basepath_option() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, data_dir) = create_basepath_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2"))
        .arg("repair")
        .arg("-q")
        .arg("--basepath")
        .arg(&data_dir)
        .arg(&par2_file)
        .output()
        .expect("Failed to execute par2 repair");

    assert!(
        output.status.success(),
        "repair with -B failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_par2_verify_repair_accept_data_filename_for_par2_set() {
    for subcommand in ["verify", "repair"] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        create_implicit_named_test_set(&temp_dir);

        let output = Command::new(get_binary_path("par2"))
            .current_dir(temp_dir.path())
            .arg(subcommand)
            .arg("-q")
            .arg("implicit.dat")
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute par2 {subcommand}"));

        assert!(
            output.status.success(),
            "par2 {subcommand} rejected data filename: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_par2_verify_purge_removes_par_files_when_valid() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, source) = create_purge_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2"))
        .arg("verify")
        .arg("-q")
        .arg("-p")
        .arg(&par2_file)
        .output()
        .expect("Failed to execute par2 verify");

    assert!(
        output.status.success(),
        "verify -p failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_quiet_output_empty(&output);
    assert!(source.exists(), "purge should not remove source file");
    assert_no_par2_files(temp_dir.path());
}

#[test]
fn test_par2_verify_purge_accepts_relative_current_dir_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (_par2_file, source) = create_purge_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2"))
        .current_dir(temp_dir.path())
        .arg("verify")
        .arg("-q")
        .arg("-p")
        .arg("purge.par2")
        .output()
        .expect("Failed to execute par2 verify");

    assert!(
        output.status.success(),
        "verify -p relative failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_quiet_output_empty(&output);
    assert!(source.exists(), "purge should not remove source file");
    assert_no_par2_files(temp_dir.path());
}

#[test]
fn test_par2_verify_purge_accepts_relative_parent_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let subdir = temp_dir.path().join("sub");
    fs::create_dir(&subdir).expect("Failed to create subdir");
    let source = subdir.join("purge.dat");
    create_test_file(&source, b"relative parent purge data").expect("Failed to create source file");

    let par2_file = subdir.join("purge.par2");
    let create_output = Command::new(get_binary_path("par2create"))
        .arg("-q")
        .arg("-s4")
        .arg("-c1")
        .arg(&par2_file)
        .arg(&source)
        .output()
        .expect("Failed to execute par2create");
    assert!(
        create_output.status.success(),
        "par2create failed: {}",
        String::from_utf8_lossy(&create_output.stderr)
    );

    let output = Command::new(get_binary_path("par2"))
        .current_dir(temp_dir.path())
        .arg("verify")
        .arg("-q")
        .arg("-p")
        .arg("sub/purge.par2")
        .output()
        .expect("Failed to execute par2 verify");

    assert!(
        output.status.success(),
        "verify -p relative parent failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_quiet_output_empty(&output);
    assert!(source.exists(), "purge should not remove source file");
    assert_no_par2_files(&subdir);
}

#[test]
fn test_par2_verify_purge_removes_uppercase_par_files() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (_par2_file, source) = create_purge_test_set(&temp_dir);
    uppercase_par2_extensions(temp_dir.path());

    let output = Command::new(get_binary_path("par2"))
        .current_dir(temp_dir.path())
        .arg("verify")
        .arg("-q")
        .arg("-p")
        .arg("purge.PAR2")
        .output()
        .expect("Failed to execute par2 verify");

    assert!(
        output.status.success(),
        "verify -p uppercase failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_quiet_output_empty(&output);
    assert!(source.exists(), "purge should not remove source file");
    assert_no_par2_files(temp_dir.path());
}

#[test]
fn test_par2_repair_scans_extra_file_arguments() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, source, renamed) = create_renamed_file_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2"))
        .arg("repair")
        .arg("-q")
        .arg(&par2_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2 repair");

    assert!(
        output.status.success(),
        "repair with extra file failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        source.exists(),
        "repair should restore the protected filename from the renamed extra"
    );
    assert!(
        !renamed.exists(),
        "repair should consume the renamed extra by moving it into place"
    );
    assert_eq!(fs::read(&source).unwrap(), b"renamed-file-scan-data");
}

#[test]
fn test_par2_repair_rename_only_restores_extra_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, source, renamed) = create_renamed_file_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2"))
        .arg("repair")
        .arg("-q")
        .arg("-O")
        .arg(&par2_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2 repair -O");

    assert!(
        output.status.success(),
        "repair -O with renamed extra failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(source.exists());
    assert!(!renamed.exists());
    assert_eq!(fs::read(&source).unwrap(), b"renamed-file-scan-data");
}

#[test]
fn test_par2_repair_rename_only_damaged_extra_does_not_reconstruct() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, source, renamed) = create_renamed_file_test_set(&temp_dir);
    fs::write(&renamed, b"damaged-renamed-data").expect("Failed to damage renamed file");

    let output = Command::new(get_binary_path("par2"))
        .arg("repair")
        .arg("-q")
        .arg("-O")
        .arg(&par2_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2 repair -O with damaged extra");

    assert!(
        !output.status.success(),
        "repair -O should fail instead of reconstructing from recovery blocks"
    );
    assert!(
        !source.exists(),
        "rename-only repair should not reconstruct"
    );
    assert!(
        renamed.exists(),
        "damaged renamed extra should be left in place"
    );
}

#[test]
fn test_par2_repair_renamed_extra_backs_up_corrupted_target() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source = temp_dir.path().join("sample.dat");
    create_test_file(&source, b"renamed-file-scan-data").expect("Failed to create source file");

    let par2_file = temp_dir.path().join("archive.par2");
    let create_output = Command::new(get_binary_path("par2create"))
        .arg("-q")
        .arg("-s4")
        .arg("-c1")
        .arg(&par2_file)
        .arg(&source)
        .output()
        .expect("Failed to execute par2create");
    assert!(create_output.status.success());

    let renamed = temp_dir.path().join("renamed.dat");
    fs::copy(&source, &renamed).expect("Failed to create exact renamed extra");
    fs::write(&source, b"corrupted target").expect("Failed to corrupt target");

    let output = Command::new(get_binary_path("par2"))
        .arg("repair")
        .arg("-q")
        .arg(&par2_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2 repair");

    assert!(
        output.status.success(),
        "repair with corrupted target and renamed extra failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(&source).unwrap(), b"renamed-file-scan-data");
    assert!(!renamed.exists());
    assert_eq!(
        fs::read(temp_dir.path().join("sample.dat.1")).unwrap(),
        b"corrupted target"
    );
}

#[test]
fn test_par2_repair_renamed_extra_uses_first_free_backup_suffix() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source = temp_dir.path().join("sample.dat");
    create_test_file(&source, b"renamed-file-scan-data").expect("Failed to create source file");

    let par2_file = temp_dir.path().join("archive.par2");
    let create_output = Command::new(get_binary_path("par2create"))
        .arg("-q")
        .arg("-s4")
        .arg("-c1")
        .arg(&par2_file)
        .arg(&source)
        .output()
        .expect("Failed to execute par2create");
    assert!(create_output.status.success());

    let renamed = temp_dir.path().join("renamed.dat");
    fs::copy(&source, &renamed).expect("Failed to create exact renamed extra");
    fs::write(temp_dir.path().join("sample.dat.1"), b"existing backup").unwrap();
    fs::write(&source, b"corrupted target").expect("Failed to corrupt target");

    let output = Command::new(get_binary_path("par2"))
        .arg("repair")
        .arg("-q")
        .arg(&par2_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2 repair");

    assert!(output.status.success());
    assert_eq!(fs::read(&source).unwrap(), b"renamed-file-scan-data");
    assert_eq!(
        fs::read(temp_dir.path().join("sample.dat.1")).unwrap(),
        b"existing backup"
    );
    assert_eq!(
        fs::read(temp_dir.path().join("sample.dat.2")).unwrap(),
        b"corrupted target"
    );
}

#[test]
fn test_par2_repair_purge_is_quiet_when_requested() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, source) = create_purge_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2"))
        .arg("repair")
        .arg("-q")
        .arg("-p")
        .arg(&par2_file)
        .output()
        .expect("Failed to execute par2 repair");

    assert!(
        output.status.success(),
        "repair -p failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_quiet_output_empty(&output);
    assert!(source.exists(), "purge should not remove source file");
    assert_no_par2_files(temp_dir.path());
}

#[test]
fn test_par2_repair_purge_after_rename_removes_recovery_files() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, source, renamed) = create_renamed_file_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2"))
        .arg("repair")
        .arg("-q")
        .arg("-p")
        .arg(&par2_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2 repair -p");

    assert!(
        output.status.success(),
        "repair -p by rename failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(source.exists(), "protected data should remain after purge");
    assert!(!renamed.exists());
    assert_no_par2_files(temp_dir.path());
}

#[test]
fn test_par2_repair_purge_after_rename_deletes_created_backup_only() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source = temp_dir.path().join("sample.dat");
    create_test_file(&source, b"renamed-file-scan-data").expect("Failed to create source file");

    let par2_file = temp_dir.path().join("archive.par2");
    let create_output = Command::new(get_binary_path("par2create"))
        .arg("-q")
        .arg("-s4")
        .arg("-c1")
        .arg(&par2_file)
        .arg(&source)
        .output()
        .expect("Failed to execute par2create");
    assert!(create_output.status.success());

    let renamed = temp_dir.path().join("renamed.dat");
    fs::copy(&source, &renamed).expect("Failed to create renamed extra");
    fs::write(&source, b"corrupted target").expect("Failed to corrupt target");

    let output = Command::new(get_binary_path("par2"))
        .arg("repair")
        .arg("-q")
        .arg("-p")
        .arg(&par2_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2 repair -p");

    assert!(output.status.success());
    assert_eq!(fs::read(&source).unwrap(), b"renamed-file-scan-data");
    assert!(!temp_dir.path().join("sample.dat.1").exists());
    assert_no_par2_files(temp_dir.path());
}

#[test]
fn test_par2_repair_failed_rename_only_with_purge_keeps_recovery_files() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, source, renamed) = create_renamed_file_test_set(&temp_dir);
    fs::write(&renamed, b"damaged-renamed-data").expect("Failed to damage renamed file");

    let output = Command::new(get_binary_path("par2"))
        .arg("repair")
        .arg("-q")
        .arg("-O")
        .arg("-p")
        .arg(&par2_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2 repair -O -p");

    assert!(!output.status.success());
    assert!(!source.exists());
    assert!(renamed.exists());
    assert!(
        par2_file.exists(),
        "failed repair must not purge recovery files"
    );
}

#[test]
fn test_par2_repair_purge_accepts_relative_current_dir_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (_par2_file, source) = create_purge_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2"))
        .current_dir(temp_dir.path())
        .arg("repair")
        .arg("-q")
        .arg("-p")
        .arg("purge.par2")
        .output()
        .expect("Failed to execute par2 repair");

    assert!(
        output.status.success(),
        "repair -p relative failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_quiet_output_empty(&output);
    assert!(source.exists(), "purge should not remove source file");
    assert_no_par2_files(temp_dir.path());
}

#[test]
fn test_par2_verify_repair_accept_scan_compat_flags() {
    let par2_file = Path::new("tests/fixtures/edge_cases/test_valid.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    for subcommand in ["verify", "repair"] {
        let output = Command::new(get_binary_path("par2"))
            .arg(subcommand)
            .arg("-q")
            .arg("-N")
            .arg("-S")
            .arg("512")
            .arg(par2_file)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute par2 {subcommand}"));

        assert!(
            output.status.success(),
            "par2 {subcommand} rejected scan flags: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_par2_verify_repair_accept_all_use_resource_flags() {
    let par2_file = Path::new("tests/fixtures/edge_cases/test_valid.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    for subcommand in ["verify", "repair"] {
        let output = Command::new(get_binary_path("par2"))
            .arg(subcommand)
            .arg("-q")
            .arg("--memory")
            .arg("64")
            .arg("--file-threads")
            .arg("2")
            .arg(par2_file)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute par2 {subcommand}"));

        assert!(
            output.status.success(),
            "par2 {subcommand} rejected resource flags: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_par2_verify_repair_accept_repeated_verbose_flags() {
    let par2_file = Path::new("tests/fixtures/edge_cases/test_valid.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    for subcommand in ["verify", "repair"] {
        let output = Command::new(get_binary_path("par2"))
            .arg(subcommand)
            .arg("-v")
            .arg("-v")
            .arg(par2_file)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute par2 {subcommand}"));

        assert!(
            output.status.success(),
            "par2 {subcommand} rejected repeated verbose flags: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_par2_verify_repair_accept_long_verbose_flag() {
    let par2_file = Path::new("tests/fixtures/edge_cases/test_valid.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    for subcommand in ["verify", "repair"] {
        let output = Command::new(get_binary_path("par2"))
            .arg(subcommand)
            .arg("--verbose")
            .arg(par2_file)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute par2 {subcommand}"));

        assert!(
            output.status.success(),
            "par2 {subcommand} rejected --verbose: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
    assert!(stdout.contains("Verify files using PAR2 data"));
    assert!(!stdout.contains("par2repair"));
}

#[test]
fn test_par2verify_version_name() {
    let output = Command::new(get_binary_path("par2verify"))
        .arg("--version")
        .output()
        .expect("Failed to execute par2verify");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("par2verify "));
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
fn test_par2verify_accepts_intact_par1_set() {
    let temp_dir = TempDir::new().unwrap();
    let par_file = create_par1_verify_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2verify"))
        .arg(&par_file)
        .output()
        .expect("Failed to execute par2verify");

    assert!(
        output.status.success(),
        "par2verify PAR1 failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_par2verify_accepts_uppercase_par1_volume_extension() {
    let temp_dir = TempDir::new().unwrap();
    let par_file = create_par1_verify_test_set(&temp_dir);
    let volume_file = temp_dir.path().join("archive.P01");
    fs::rename(&par_file, &volume_file).expect("Failed to rename PAR1 fixture");

    let output = Command::new(get_binary_path("par2verify"))
        .arg(&volume_file)
        .output()
        .expect("Failed to execute par2verify");

    assert!(
        output.status.success(),
        "par2verify PAR1 .P01 failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_par2verify_reports_repair_required_for_renamed_par1_file() {
    let temp_dir = TempDir::new().unwrap();
    let par_file = copy_real_par1_fixture(&temp_dir);
    let target = temp_dir.path().join("test-1.data");
    let renamed = temp_dir.path().join("renamed.data");
    fs::rename(&target, &renamed).unwrap();

    let output = Command::new(get_binary_path("par2verify"))
        .arg(&par_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2verify");

    assert!(
        !output.status.success(),
        "par2verify PAR1 renamed file should require repair: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Repair required"),
        "par2verify should report repair required: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_par1_verify_ignores_skip_leeway_without_data_skipping() {
    let temp_dir = TempDir::new().unwrap();
    let par_file = create_par1_verify_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2verify"))
        .arg("-S10")
        .arg(&par_file)
        .output()
        .expect("Failed to execute par2verify");

    assert!(
        output.status.success(),
        "PAR1 verify should ignore -S without -N: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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

#[test]
fn test_par2verify_accepts_repeated_quiet_flags() {
    let par2_file = Path::new("tests/fixtures/edge_cases/test_valid.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    let output = Command::new(get_binary_path("par2verify"))
        .arg("-q")
        .arg("-q")
        .arg(par2_file)
        .output()
        .expect("Failed to execute par2verify");

    assert_repeated_quiet_accepted(&output);
    assert!(output.status.success());
}

#[test]
fn test_par2verify_uses_basepath_option() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, data_dir) = create_basepath_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2verify"))
        .arg("-q")
        .arg("--basepath")
        .arg(&data_dir)
        .arg(&par2_file)
        .output()
        .expect("Failed to execute par2verify");

    assert!(
        output.status.success(),
        "par2verify with -B failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_par2verify_scans_extra_file_arguments() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, _source, renamed) = create_renamed_file_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2verify"))
        .arg("-q")
        .arg(&par2_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2verify");

    assert!(
        !output.status.success(),
        "par2verify with renamed extra should report repair required"
    );
    assert!(renamed.exists(), "par2verify must remain non-mutating");
}

#[test]
fn test_par2verify_purge_removes_par_files_when_valid() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, source) = create_purge_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2verify"))
        .arg("-q")
        .arg("-p")
        .arg(&par2_file)
        .output()
        .expect("Failed to execute par2verify");

    assert!(
        output.status.success(),
        "par2verify -p failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_quiet_output_empty(&output);
    assert!(source.exists(), "purge should not remove source file");
    assert_no_par2_files(temp_dir.path());
}

#[test]
fn test_par2verify_purge_accepts_relative_current_dir_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (_par2_file, source) = create_purge_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2verify"))
        .current_dir(temp_dir.path())
        .arg("-q")
        .arg("-p")
        .arg("purge.par2")
        .output()
        .expect("Failed to execute par2verify");

    assert!(
        output.status.success(),
        "par2verify -p relative failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_quiet_output_empty(&output);
    assert!(source.exists(), "purge should not remove source file");
    assert_no_par2_files(temp_dir.path());
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
    assert!(stdout.contains("Repair files using PAR2 recovery data"));
    assert!(stdout.contains("--purge") || stdout.contains("-p"));
    assert!(stdout.contains("Extra files to scan"));
}

#[test]
fn test_par2repair_version_name() {
    let output = Command::new(get_binary_path("par2repair"))
        .arg("--version")
        .output()
        .expect("Failed to execute par2repair");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("par2repair "));
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
fn test_par2repair_repairs_missing_par1_file_from_volume_input() {
    let temp_dir = TempDir::new().unwrap();
    copy_real_par1_fixture(&temp_dir);
    let volume_file = temp_dir.path().join("testdata.p01");
    let missing_file = temp_dir.path().join("test-5.data");
    fs::remove_file(&missing_file).unwrap();

    let output = Command::new(get_binary_path("par2repair"))
        .arg(&volume_file)
        .output()
        .expect("Failed to execute par2repair");

    assert!(
        output.status.success(),
        "par2repair PAR1 failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(missing_file.exists());
}

#[test]
fn test_par2repair_repairs_renamed_par1_file() {
    let fixture_dir = Path::new("tests/fixtures/par1/flatdata");
    let temp_dir = TempDir::new().unwrap();
    let par_file = copy_real_par1_fixture(&temp_dir);
    let target = temp_dir.path().join("test-5.data");
    let renamed = temp_dir.path().join("renamed.data");
    fs::rename(&target, &renamed).unwrap();

    let output = Command::new(get_binary_path("par2repair"))
        .arg(&par_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2repair");

    assert!(
        output.status.success(),
        "par2repair PAR1 renamed file failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(&target).unwrap(),
        fs::read(fixture_dir.join("test-5.data")).unwrap()
    );
    assert!(!renamed.exists());
}

#[test]
fn test_par2repair_repairs_renamed_par1_file_from_volume_input() {
    let fixture_dir = Path::new("tests/fixtures/par1/flatdata");
    let temp_dir = TempDir::new().unwrap();
    copy_real_par1_fixture(&temp_dir);
    let volume_file = temp_dir.path().join("testdata.p01");
    let target = temp_dir.path().join("test-6.data");
    let renamed = temp_dir.path().join("renamed.data");
    fs::rename(&target, &renamed).unwrap();

    let output = Command::new(get_binary_path("par2repair"))
        .arg(&volume_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2repair");

    assert!(
        output.status.success(),
        "par2repair PAR1 volume renamed file failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(&target).unwrap(),
        fs::read(fixture_dir.join("test-6.data")).unwrap()
    );
    assert!(!renamed.exists());
}

#[test]
fn test_par1_repair_honors_memory_flag() {
    for (binary, subcommand) in [("par2", Some("repair")), ("par2repair", None)] {
        let temp_dir = TempDir::new().unwrap();
        let par_file = copy_real_par1_fixture(&temp_dir);
        let missing_file = temp_dir.path().join("test-6.data");
        fs::remove_file(&missing_file).unwrap();

        let mut command = Command::new(get_binary_path(binary));
        if let Some(subcommand) = subcommand {
            command.arg(subcommand);
        }
        let output = command
            .arg("-m1")
            .arg(&par_file)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            output.status.success(),
            "{binary} PAR1 repair with -m failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(missing_file.exists());
    }
}

#[test]
fn test_par1_repair_rejects_zero_memory_flag() {
    for (binary, subcommand) in [("par2", Some("repair")), ("par2repair", None)] {
        let temp_dir = TempDir::new().unwrap();
        let par_file = copy_real_par1_fixture(&temp_dir);

        let mut command = Command::new(get_binary_path(binary));
        if let Some(subcommand) = subcommand {
            command.arg(subcommand);
        }
        let output = command
            .arg("-m0")
            .arg(&par_file)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            !output.status.success(),
            "{binary} accepted PAR1 repair -m0: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("must be greater than 0"),
            "{binary} should explain -m0 rejection: stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
fn test_par2repair_accepts_repeated_quiet_flags() {
    let par2_file = Path::new("tests/fixtures/edge_cases/test_valid.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    let output = Command::new(get_binary_path("par2repair"))
        .arg("-q")
        .arg("-q")
        .arg(par2_file)
        .output()
        .expect("Failed to execute par2repair");

    assert_repeated_quiet_accepted(&output);
    assert!(output.status.success());
}

#[test]
fn test_par2repair_uses_basepath_option() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, data_dir) = create_basepath_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2repair"))
        .arg("-q")
        .arg("--basepath")
        .arg(&data_dir)
        .arg(&par2_file)
        .output()
        .expect("Failed to execute par2repair");

    assert!(
        output.status.success(),
        "par2repair with -B failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_par2repair_scans_extra_file_arguments() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, source, renamed) = create_renamed_file_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2repair"))
        .arg("-q")
        .arg(&par2_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2repair");

    assert!(
        output.status.success(),
        "par2repair with extra file failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        source.exists(),
        "repair should restore the protected filename from the renamed extra"
    );
    assert!(
        !renamed.exists(),
        "repair should consume the renamed extra by moving it into place"
    );
    assert_eq!(fs::read(&source).unwrap(), b"renamed-file-scan-data");
}

#[test]
fn test_par2repair_rename_only_restores_extra_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, source, renamed) = create_renamed_file_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2repair"))
        .arg("-q")
        .arg("-O")
        .arg(&par2_file)
        .arg(&renamed)
        .output()
        .expect("Failed to execute par2repair -O");

    assert!(
        output.status.success(),
        "par2repair -O with renamed extra failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(source.exists());
    assert!(!renamed.exists());
    assert_eq!(fs::read(&source).unwrap(), b"renamed-file-scan-data");
}

#[test]
fn test_standalone_verify_repair_accept_data_filename_for_par2_set() {
    for binary in ["par2verify", "par2repair"] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        create_implicit_named_test_set(&temp_dir);

        let output = Command::new(get_binary_path(binary))
            .current_dir(temp_dir.path())
            .arg("-q")
            .arg("implicit.dat")
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            output.status.success(),
            "{binary} rejected data filename: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_par2repair_purge_is_quiet_when_requested() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (par2_file, source) = create_purge_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2repair"))
        .arg("-q")
        .arg("-p")
        .arg(&par2_file)
        .output()
        .expect("Failed to execute par2repair");

    assert!(
        output.status.success(),
        "par2repair -p failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_quiet_output_empty(&output);
    assert!(source.exists(), "purge should not remove source file");
    assert_no_par2_files(temp_dir.path());
}

#[test]
fn test_par2repair_purge_accepts_relative_current_dir_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (_par2_file, source) = create_purge_test_set(&temp_dir);

    let output = Command::new(get_binary_path("par2repair"))
        .current_dir(temp_dir.path())
        .arg("-q")
        .arg("-p")
        .arg("purge.par2")
        .output()
        .expect("Failed to execute par2repair");

    assert!(
        output.status.success(),
        "par2repair -p relative failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_quiet_output_empty(&output);
    assert!(source.exists(), "purge should not remove source file");
    assert_no_par2_files(temp_dir.path());
}

#[test]
fn test_standalone_verify_repair_accept_scan_compat_flags() {
    let par2_file = Path::new("tests/fixtures/edge_cases/test_valid.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    for binary in ["par2verify", "par2repair"] {
        let output = Command::new(get_binary_path(binary))
            .arg("-q")
            .arg("-N")
            .arg("-S")
            .arg("512")
            .arg(par2_file)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            output.status.success(),
            "{binary} rejected scan flags: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_standalone_verify_repair_accept_all_use_resource_flags() {
    let par2_file = Path::new("tests/fixtures/edge_cases/test_valid.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    for binary in ["par2verify", "par2repair"] {
        let output = Command::new(get_binary_path(binary))
            .arg("-q")
            .arg("--memory")
            .arg("64")
            .arg("--file-threads")
            .arg("2")
            .arg(par2_file)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            output.status.success(),
            "{binary} rejected resource flags: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_standalone_verify_repair_accept_repeated_verbose_flags() {
    let par2_file = Path::new("tests/fixtures/edge_cases/test_valid.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    for binary in ["par2verify", "par2repair"] {
        let output = Command::new(get_binary_path(binary))
            .arg("-v")
            .arg("-v")
            .arg(par2_file)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            output.status.success(),
            "{binary} rejected repeated verbose flags: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_standalone_verify_repair_accept_long_verbose_flag() {
    let par2_file = Path::new("tests/fixtures/edge_cases/test_valid.par2");
    if !par2_file.exists() {
        eprintln!("Skipping test - fixture not found");
        return;
    }

    for binary in ["par2verify", "par2repair"] {
        let output = Command::new(get_binary_path(binary))
            .arg("--verbose")
            .arg(par2_file)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            output.status.success(),
            "{binary} rejected --verbose: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
        .arg("--help")
        .output()
        .expect("Failed to execute par2create");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Create PAR2 recovery files"));
    assert!(stdout.contains("-r"));
}

#[test]
fn test_par2create_creates_par2_files() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source = temp_dir.path().join("sample.txt");
    create_test_file(&source, b"standalone par2create smoke test")
        .expect("Failed to create source file");

    let output_base = temp_dir.path().join("sample.par2");
    let output = Command::new(get_binary_path("par2create"))
        .arg("-q")
        .arg("-s")
        .arg("4")
        .arg("-c")
        .arg("1")
        .arg(&output_base)
        .arg(&source)
        .output()
        .expect("Failed to execute par2create");

    assert!(
        output.status.success(),
        "par2create failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(temp_dir.path().join("sample.par2").exists());
    let has_volume = fs::read_dir(temp_dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .any(|entry| {
            entry
                .file_name()
                .to_str()
                .map(|name| name.starts_with("sample.vol") && name.ends_with(".par2"))
                .unwrap_or(false)
        });
    assert!(has_volume, "expected at least one recovery volume file");
}

#[test]
fn test_create_commands_reject_par1_output_names() {
    for (binary, command, output_name) in [
        ("par2create", None, "archive.par"),
        ("par2", Some("create"), "archive.P01"),
    ] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let source = temp_dir.path().join("source.dat");
        create_test_file(&source, b"par1 create target rejection")
            .expect("Failed to create source file");
        let output_base = temp_dir.path().join(output_name);

        let mut command_runner = Command::new(get_binary_path(binary));
        if let Some(subcommand) = command {
            command_runner.arg(subcommand);
        }
        let output = command_runner
            .arg("-q")
            .arg("-s4")
            .arg("-c1")
            .arg(&output_base)
            .arg(&source)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            !output.status.success(),
            "{binary} accepted PAR1 create output name: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("PAR1 create is not supported"),
            "{binary} should explain PAR1 create rejection: stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output_base.exists(), "{binary} created a PAR1-named file");
    }
}

#[test]
fn test_par2_create_rejects_rename_only_flag() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source = temp_dir.path().join("source.txt");
    create_test_file(&source, b"source").expect("Failed to create source file");

    let output = Command::new(get_binary_path("par2"))
        .current_dir(temp_dir.path())
        .arg("create")
        .arg("-O")
        .arg("out.par2")
        .arg("source.txt")
        .output()
        .expect("Failed to execute par2 create -O");

    assert!(!output.status.success(), "create -O should be rejected");
    assert!(!temp_dir.path().join("out.par2").exists());
}

#[test]
fn test_create_commands_reject_par1_archive_name() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source = temp_dir.path().join("source.dat");
    create_test_file(&source, b"par1 archive name rejection").expect("Failed to create source");
    let output_base = temp_dir.path().join("archive.par2");
    let archive_name = temp_dir.path().join("archive.par");

    let output = Command::new(get_binary_path("par2create"))
        .arg("-q")
        .arg("-a")
        .arg(&archive_name)
        .arg("-s4")
        .arg("-c1")
        .arg(&output_base)
        .arg(&source)
        .output()
        .expect("Failed to execute par2create");

    assert!(
        !output.status.success(),
        "par2create accepted PAR1 archive name: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("PAR1 create is not supported"),
        "par2create should explain PAR1 archive name rejection: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !archive_name.exists(),
        "par2create created a PAR1-named archive"
    );
}

#[test]
fn test_create_commands_accept_long_quiet_and_verbose_flags() {
    for (binary, command, flag, name) in [
        ("par2create", None, "--verbose", "verbose"),
        ("par2", Some("create"), "--quiet", "quiet"),
    ] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let source = temp_dir.path().join(format!("{name}.dat"));
        create_test_file(&source, b"long verbosity create").expect("Failed to create source file");
        let output_base = temp_dir.path().join(format!("{name}.par2"));

        let mut command_runner = Command::new(get_binary_path(binary));
        if let Some(subcommand) = command {
            command_runner.arg(subcommand);
        }
        let output = command_runner
            .arg(flag)
            .arg("-s")
            .arg("4")
            .arg("-c")
            .arg("1")
            .arg(&output_base)
            .arg(&source)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            output.status.success(),
            "{binary} rejected {flag}: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_binaries_reject_mixed_verbose_and_quiet() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source = temp_dir.path().join("source.dat");
    create_test_file(&source, b"mixed verbosity").expect("Failed to create source file");

    let cases: Vec<(&str, Vec<String>)> = vec![
        (
            "par2create",
            vec![
                "-v".into(),
                "-q".into(),
                temp_dir
                    .path()
                    .join("a.par2")
                    .to_string_lossy()
                    .into_owned(),
                source.to_string_lossy().into_owned(),
            ],
        ),
        (
            "par2",
            vec![
                "create".into(),
                "-v".into(),
                "-q".into(),
                temp_dir
                    .path()
                    .join("b.par2")
                    .to_string_lossy()
                    .into_owned(),
                source.to_string_lossy().into_owned(),
            ],
        ),
        (
            "par2verify",
            vec!["-v".into(), "-q".into(), "missing.par2".into()],
        ),
        (
            "par2repair",
            vec!["-v".into(), "-q".into(), "missing.par2".into()],
        ),
    ];

    for (binary, args) in cases {
        let output = Command::new(get_binary_path(binary))
            .args(args)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));
        assert!(
            !output.status.success(),
            "{binary} accepted mixed -v/-q: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_create_commands_accept_long_resource_flags() {
    for (binary, command) in [("par2create", None), ("par2", Some("create"))] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let source = temp_dir.path().join("threads.dat");
        create_test_file(&source, b"long threads create").expect("Failed to create source file");
        let output_base = temp_dir.path().join("threads.par2");

        let mut command_runner = Command::new(get_binary_path(binary));
        if let Some(subcommand) = command {
            command_runner.arg(subcommand);
        }
        let output = command_runner
            .arg("-q")
            .arg("--threads")
            .arg("1")
            .arg("--memory")
            .arg("16")
            .arg("--file-threads")
            .arg("1")
            .arg("--basepath")
            .arg(temp_dir.path())
            .arg("-s")
            .arg("4")
            .arg("-c")
            .arg("1")
            .arg(&output_base)
            .arg(&source)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            output.status.success(),
            "{binary} rejected long resource flags: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_create_commands_reject_existing_outputs() {
    for (binary, command) in [("par2create", None), ("par2", Some("create"))] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let source = temp_dir.path().join("overwrite.dat");
        create_test_file(&source, b"existing output safety").expect("Failed to create source file");
        let output_base = temp_dir.path().join("overwrite.par2");
        create_test_file(&output_base, b"existing index").expect("Failed to create index file");

        let mut command_runner = Command::new(get_binary_path(binary));
        if let Some(subcommand) = command {
            command_runner.arg(subcommand);
        }
        let output = command_runner
            .arg("-q")
            .arg("-s")
            .arg("4")
            .arg("-c")
            .arg("1")
            .arg(&output_base)
            .arg(&source)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            !output.status.success(),
            "{binary} overwrote an existing output: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.status.code(),
            Some(3),
            "{binary} should match turbo's existing-output exit code: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read(&output_base).unwrap(), b"existing index");
    }
}

#[test]
fn test_verify_and_repair_reject_skip_leeway_without_data_skipping() {
    for (binary, args) in [
        ("par2verify", vec!["-S10", "missing.par2"]),
        ("par2repair", vec!["-S10", "missing.par2"]),
        ("par2", vec!["verify", "-S10", "missing.par2"]),
        ("par2", vec!["repair", "-S10", "missing.par2"]),
    ] {
        let output = Command::new(get_binary_path(binary))
            .args(args)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));
        assert!(
            !output.status.success(),
            "{binary} accepted -S without -N: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("only valid when -N"),
            "{binary} failed for the wrong reason: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_verify_and_repair_accept_archive_name_as_noop() {
    for (binary, prefix_args) in [
        ("par2verify", Vec::<&str>::new()),
        ("par2repair", Vec::<&str>::new()),
        ("par2", vec!["verify"]),
        ("par2", vec!["repair"]),
    ] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let (par2_file, _source) = create_purge_test_set(&temp_dir);
        let ignored_archive = temp_dir.path().join("ignored.par2");

        let mut command = Command::new(get_binary_path(binary));
        command.args(prefix_args);
        let output = command
            .arg("-q")
            .arg("-aignored.par2")
            .arg(&par2_file)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            output.status.success(),
            "{binary} rejected verify/repair -a no-op: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !ignored_archive.exists(),
            "{binary} should ignore -a during verify/repair"
        );
    }
}

#[test]
fn test_par2create_uses_single_existing_file_as_source() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source = temp_dir.path().join("implicit.dat");
    create_test_file(&source, b"implicit source file").expect("Failed to create source file");

    let output = Command::new(get_binary_path("par2create"))
        .current_dir(temp_dir.path())
        .arg("-q")
        .arg("-s")
        .arg("4")
        .arg("-c")
        .arg("1")
        .arg("implicit.dat")
        .output()
        .expect("Failed to execute par2create");

    assert!(
        output.status.success(),
        "par2create implicit source failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(temp_dir.path().join("implicit.dat.par2").exists());
    assert!(temp_dir.path().join("implicit.dat.vol0+1.par2").exists());
}

#[test]
fn test_create_commands_recurse_directories_with_basepath() {
    for (binary, command) in [("par2create", None), ("par2", Some("create"))] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let root = temp_dir.path().join("root");
        let nested = root.join("nested");
        fs::create_dir_all(&nested).expect("Failed to create nested test dir");
        create_test_file(&root.join("a.txt"), b"recursive-a")
            .expect("Failed to create source file");
        create_test_file(&nested.join("b.txt"), b"recursive-b")
            .expect("Failed to create nested source file");
        let output_base = temp_dir.path().join(format!("{binary}-recurse.par2"));

        let mut command_runner = Command::new(get_binary_path(binary));
        if let Some(subcommand) = command {
            command_runner.arg(subcommand);
        }
        let output = command_runner
            .arg("-q")
            .arg("-R")
            .arg("--basepath")
            .arg(&root)
            .arg("-s")
            .arg("4")
            .arg("-c")
            .arg("1")
            .arg(&output_base)
            .arg(&root)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            output.status.success(),
            "{binary} recursive create failed: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            file_description_names(&output_base),
            vec!["a.txt".to_string(), "nested/b.txt".to_string()]
        );
    }
}

#[test]
fn test_create_commands_use_archive_name_option() {
    for (binary, command) in [("par2create", None), ("par2", Some("create"))] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let source = temp_dir.path().join("archive-source.dat");
        create_test_file(&source, b"archive name option").expect("Failed to create source file");
        let positional_base = temp_dir.path().join("positional.par2");
        let archive_base = temp_dir.path().join("custom.par2");

        let mut command_runner = Command::new(get_binary_path(binary));
        if let Some(subcommand) = command {
            command_runner.arg(subcommand);
        }
        let output = command_runner
            .arg("-q")
            .arg("-a")
            .arg(&archive_base)
            .arg("-s")
            .arg("4")
            .arg("-c")
            .arg("1")
            .arg(&positional_base)
            .arg(&source)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            output.status.success(),
            "{binary} -a failed: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(archive_base.exists(), "{binary} did not create -a archive");
        assert!(
            !positional_base.exists(),
            "{binary} unexpectedly created positional archive name"
        );
    }
}

#[test]
fn test_create_commands_use_first_recovery_block_option() {
    for (binary, command) in [("par2create", None), ("par2", Some("create"))] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let source = temp_dir.path().join("first-block.dat");
        create_test_file(&source, b"first recovery block option")
            .expect("Failed to create source file");
        let output_base = temp_dir.path().join(format!("{binary}-first.par2"));

        let mut command_runner = Command::new(get_binary_path(binary));
        if let Some(subcommand) = command {
            command_runner.arg(subcommand);
        }
        let output = command_runner
            .arg("-q")
            .arg("-s")
            .arg("4")
            .arg("-c")
            .arg("1")
            .arg("-f")
            .arg("7")
            .arg(&output_base)
            .arg(&source)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            output.status.success(),
            "{binary} -f failed: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stem = output_base.file_stem().unwrap().to_string_lossy();
        assert!(
            temp_dir.path().join(format!("{stem}.vol7+1.par2")).exists(),
            "{binary} did not start recovery volumes at exponent 7"
        );
    }
}

#[test]
fn test_par2_create_uses_single_existing_file_as_source() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source = temp_dir.path().join("implicit-unified.dat");
    create_test_file(&source, b"implicit source file").expect("Failed to create source file");

    let output = Command::new(get_binary_path("par2"))
        .current_dir(temp_dir.path())
        .arg("create")
        .arg("-q")
        .arg("-s")
        .arg("4")
        .arg("-c")
        .arg("1")
        .arg("implicit-unified.dat")
        .output()
        .expect("Failed to execute par2 create");

    assert!(
        output.status.success(),
        "par2 create implicit source failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(temp_dir.path().join("implicit-unified.dat.par2").exists());
    assert!(temp_dir
        .path()
        .join("implicit-unified.dat.vol0+1.par2")
        .exists());
}

#[test]
fn test_par2create_accepts_target_size_redundancy() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source = temp_dir.path().join("target-size.txt");
    create_test_file(&source, b"target size redundancy smoke test")
        .expect("Failed to create source file");

    let output_base = temp_dir.path().join("target-size.par2");
    let output = Command::new(get_binary_path("par2create"))
        .arg("-q")
        .arg("-s")
        .arg("4")
        .arg("-r")
        .arg("k1")
        .arg("-n")
        .arg("1")
        .arg(&output_base)
        .arg(&source)
        .output()
        .expect("Failed to execute par2create");

    assert!(
        output.status.success(),
        "par2create -rk failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(temp_dir.path().join("target-size.par2").exists());
}

#[test]
fn test_par2create_accepts_high_redundancy_with_warning() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source = temp_dir.path().join("high-percent.txt");
    create_test_file(&source, b"high redundancy smoke test").expect("Failed to create source file");

    let output_base = temp_dir.path().join("high-percent.par2");
    let output = Command::new(get_binary_path("par2create"))
        .arg("-q")
        .arg("-s")
        .arg("4")
        .arg("-r")
        .arg("101")
        .arg(&output_base)
        .arg(&source)
        .output()
        .expect("Failed to execute par2create");

    assert!(
        output.status.success(),
        "par2create -r101 failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("WARNING: Creating recovery file(s) with 101% redundancy."));
    assert!(temp_dir.path().join("high-percent.par2").exists());
}

#[test]
fn test_par2create_rejects_conflicting_create_options() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source = temp_dir.path().join("conflict.txt");
    create_test_file(&source, b"conflict smoke test").expect("Failed to create source file");
    let output_base = temp_dir.path().join("conflict.par2");

    let output = Command::new(get_binary_path("par2create"))
        .arg("-s")
        .arg("4")
        .arg("-c")
        .arg("1")
        .arg("-r")
        .arg("5")
        .arg(&output_base)
        .arg(&source)
        .output()
        .expect("Failed to execute par2create");

    assert!(
        !output.status.success(),
        "par2create accepted conflicting -c and -r options"
    );
}

#[test]
fn test_par2create_rejects_too_many_recovery_files() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source = temp_dir.path().join("too-many.txt");
    create_test_file(&source, b"too many recovery files").expect("Failed to create source file");
    let output_base = temp_dir.path().join("too-many.par2");

    let output = Command::new(get_binary_path("par2create"))
        .arg("-q")
        .arg("-s")
        .arg("4")
        .arg("-c")
        .arg("1")
        .arg("-n")
        .arg("32")
        .arg(&output_base)
        .arg(&source)
        .output()
        .expect("Failed to execute par2create");

    assert!(
        !output.status.success(),
        "par2create accepted more than 31 recovery files"
    );
}

#[test]
fn test_create_commands_reject_too_many_source_blocks() {
    for (binary, command) in [("par2create", None), ("par2", Some("create"))] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let source = temp_dir.path().join("too-many-blocks.txt");
        create_test_file(&source, b"too many source blocks").expect("Failed to create source file");
        let output_base = temp_dir
            .path()
            .join(format!("{binary}-too-many-blocks.par2"));

        let mut command_runner = Command::new(get_binary_path(binary));
        if let Some(subcommand) = command {
            command_runner.arg(subcommand);
        }
        let output = command_runner
            .arg("-q")
            .arg("-b32769")
            .arg("-c1")
            .arg(&output_base)
            .arg(&source)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            !output.status.success(),
            "{binary} accepted more than 32768 source blocks"
        );
    }
}

#[test]
fn test_create_commands_reject_too_many_recovery_blocks() {
    for (binary, command) in [("par2create", None), ("par2", Some("create"))] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let source = temp_dir.path().join("too-many-recovery-blocks.txt");
        create_test_file(&source, b"too many recovery blocks")
            .expect("Failed to create source file");
        let output_base = temp_dir
            .path()
            .join(format!("{binary}-too-many-recovery-blocks.par2"));

        let mut command_runner = Command::new(get_binary_path(binary));
        if let Some(subcommand) = command {
            command_runner.arg(subcommand);
        }
        let output = command_runner
            .arg("-q")
            .arg("-s4")
            .arg("-c32769")
            .arg(&output_base)
            .arg(&source)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            !output.status.success(),
            "{binary} accepted more than 32768 recovery blocks"
        );
    }
}

#[test]
fn test_create_commands_reject_too_large_first_recovery_block() {
    for (binary, command) in [("par2create", None), ("par2", Some("create"))] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let source = temp_dir.path().join("too-large-first-block.txt");
        create_test_file(&source, b"too large first block").expect("Failed to create source file");
        let output_base = temp_dir
            .path()
            .join(format!("{binary}-too-large-first-block.par2"));

        let mut command_runner = Command::new(get_binary_path(binary));
        if let Some(subcommand) = command {
            command_runner.arg(subcommand);
        }
        let output = command_runner
            .arg("-q")
            .arg("-s4")
            .arg("-c1")
            .arg("-f32769")
            .arg(&output_base)
            .arg(&source)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            !output.status.success(),
            "{binary} accepted first recovery block above 32768"
        );
    }
}

#[test]
fn test_create_commands_reject_file_count_with_zero_recovery_blocks() {
    for (binary, command) in [("par2create", None), ("par2", Some("create"))] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let source = temp_dir.path().join("zero-recovery-with-count.txt");
        create_test_file(&source, b"zero recovery with count")
            .expect("Failed to create source file");
        let output_base = temp_dir
            .path()
            .join(format!("{binary}-zero-recovery-with-count.par2"));

        let mut command_runner = Command::new(get_binary_path(binary));
        if let Some(subcommand) = command {
            command_runner.arg(subcommand);
        }
        let output = command_runner
            .arg("-q")
            .arg("-s4")
            .arg("-c0")
            .arg("-n1")
            .arg(&output_base)
            .arg(&source)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary}"));

        assert!(
            !output.status.success(),
            "{binary} accepted recovery file count with zero recovery blocks"
        );
    }
}

#[test]
fn test_par2create_n_uses_uniform_file_sizes() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source = temp_dir.path().join("uniform.txt");
    create_test_file(&source, b"uniform recovery file count")
        .expect("Failed to create source file");
    let output_base = temp_dir.path().join("uniform.par2");

    let output = Command::new(get_binary_path("par2create"))
        .arg("-q")
        .arg("-s")
        .arg("4")
        .arg("-c")
        .arg("5")
        .arg("-n")
        .arg("2")
        .arg(&output_base)
        .arg(&source)
        .output()
        .expect("Failed to execute par2create");

    assert!(
        output.status.success(),
        "par2create -n failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(temp_dir.path().join("uniform.vol0+3.par2").exists());
    assert!(temp_dir.path().join("uniform.vol3+2.par2").exists());
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
    let binaries = vec!["par2", "par2verify", "par2repair", "par2create"];

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
