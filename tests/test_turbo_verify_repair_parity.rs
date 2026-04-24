use par2rs::create::{CreateContextBuilder, SilentCreateReporter};
use par2rs::par2_files;
use par2rs::repair::{repair_files, SilentReporter};
use par2rs::reporters::SilentVerificationReporter;
use par2rs::verify::{comprehensive_verify_files, FileStatus, VerificationConfig};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn create_small_recovery_set(temp_dir: &TempDir, file_name: &str, data: &[u8]) -> PathBuf {
    let source = temp_dir.path().join(file_name);
    fs::write(&source, data).unwrap();

    let par2_file = temp_dir.path().join(format!("{file_name}.par2"));
    let mut context = CreateContextBuilder::new()
        .output_name(par2_file.to_string_lossy())
        .source_files(vec![source])
        .base_path(temp_dir.path())
        .block_size(4)
        .recovery_block_count(1)
        .reporter(Box::new(SilentCreateReporter))
        .build()
        .unwrap();

    context.create().unwrap();
    par2_file
}

fn verify_with_config(
    par2_file: &Path,
    config: &VerificationConfig,
) -> par2rs::verify::VerificationResults {
    let par2_files = par2_files::collect_par2_files(par2_file);
    let packet_set = par2_files::load_par2_packets(&par2_files, false, false);
    comprehensive_verify_files(
        packet_set,
        config,
        &SilentVerificationReporter,
        par2_file.parent().unwrap(),
    )
}

#[test]
fn verify_marks_complete_extra_file_as_renamed() {
    let temp_dir = TempDir::new().unwrap();
    let par2_file = create_small_recovery_set(&temp_dir, "data.bin", b"abcdefghijkl");

    let target = temp_dir.path().join("data.bin");
    let misplaced = temp_dir.path().join("misplaced.bin");
    fs::rename(&target, &misplaced).unwrap();

    let config = VerificationConfig::default().with_extra_files(vec![misplaced]);
    let results = verify_with_config(&par2_file, &config);

    assert_eq!(results.renamed_file_count, 1);
    assert_eq!(results.missing_block_count, 0);
    assert_eq!(results.files[0].status, FileStatus::Renamed);
}

#[test]
fn repair_accepts_complete_extra_file_without_consuming_it() {
    let temp_dir = TempDir::new().unwrap();
    let par2_file = create_small_recovery_set(&temp_dir, "data.bin", b"abcdefghijkl");

    let target = temp_dir.path().join("data.bin");
    let misplaced = temp_dir.path().join("misplaced.bin");
    fs::rename(&target, &misplaced).unwrap();

    let config = VerificationConfig::default().with_extra_files(vec![misplaced.clone()]);
    let (_, result) = repair_files(
        par2_file.to_str().unwrap(),
        Box::new(SilentReporter),
        &config,
    )
    .unwrap();

    assert!(result.is_success(), "{result:?}");
    assert_eq!(fs::read(target).unwrap(), b"abcdefghijkl");
    assert_eq!(fs::read(misplaced).unwrap(), b"abcdefghijkl");
}

#[test]
fn repair_rewrites_misaligned_file_when_all_blocks_are_available() {
    let temp_dir = TempDir::new().unwrap();
    let par2_file = create_small_recovery_set(&temp_dir, "data.bin", b"abcdefghijkl");

    let target = temp_dir.path().join("data.bin");
    fs::write(&target, b"Xabcdefghijkl").unwrap();

    let (_, result) = repair_files(
        par2_file.to_str().unwrap(),
        Box::new(SilentReporter),
        &VerificationConfig::default(),
    )
    .unwrap();

    assert!(result.is_success(), "{result:?}");
    assert_eq!(fs::read(target).unwrap(), b"abcdefghijkl");
}

#[test]
fn repair_rewrites_canonical_corruption_when_all_blocks_are_available() {
    let temp_dir = TempDir::new().unwrap();
    let par2_file = create_small_recovery_set(&temp_dir, "data.bin", b"abcdefghijkl");

    let target = temp_dir.path().join("data.bin");
    fs::write(&target, b"abcdefghijklX").unwrap();

    let (_, result) = repair_files(
        par2_file.to_str().unwrap(),
        Box::new(SilentReporter),
        &VerificationConfig::default(),
    )
    .unwrap();

    assert!(result.is_success(), "{result:?}");
    assert_eq!(fs::read(target).unwrap(), b"abcdefghijkl");
}

#[test]
fn repair_uses_partial_extra_file_blocks_as_repair_sources() {
    let temp_dir = TempDir::new().unwrap();
    let par2_file = create_small_recovery_set(&temp_dir, "data.bin", b"abcdefghijkl");

    let target = temp_dir.path().join("data.bin");
    fs::remove_file(&target).unwrap();
    let partial_extra = temp_dir.path().join("partial.bin");
    fs::write(&partial_extra, b"abcdefgh").unwrap();

    let config = VerificationConfig::default().with_extra_files(vec![partial_extra.clone()]);
    let results = verify_with_config(&par2_file, &config);
    assert_eq!(results.missing_block_count, 1);

    let (_, result) = repair_files(
        par2_file.to_str().unwrap(),
        Box::new(SilentReporter),
        &config,
    )
    .unwrap();

    assert!(result.is_success(), "{result:?}");
    assert_eq!(fs::read(target).unwrap(), b"abcdefghijkl");
    assert!(partial_extra.exists());
}
