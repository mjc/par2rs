use par2rs::par1::parser::parse_par1_bytes;
use par2rs::par1::repair::{repair_par1_file_with_options, Par1RepairOptions};
use par2rs::par1::verify::{verify_par1_file, verify_par1_file_with_options, Par1VerifyOptions};
use par2rs::verify::FileStatus;
use std::path::Path;

fn copy_real_par1_fixture(temp: &tempfile::TempDir) {
    let fixture_dir = Path::new("tests/fixtures/par1/flatdata");
    for entry in std::fs::read_dir(fixture_dir).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), temp.path().join(entry.file_name())).unwrap();
    }
}

#[test]
fn real_par1_fixture_parses_main_and_volumes() {
    let fixture_dir = Path::new("tests/fixtures/par1/flatdata");
    let main = std::fs::read(fixture_dir.join("testdata.par")).unwrap();
    let volume = std::fs::read(fixture_dir.join("testdata.p01")).unwrap();

    let main_set = parse_par1_bytes(&main).unwrap();
    let volume_set = parse_par1_bytes(&volume).unwrap();

    assert_eq!(main_set.files.len(), 10);
    assert_eq!(volume_set.files.len(), 10);
    assert!(main_set.volume.is_none());
    assert!(volume_set.volume.is_some());
    assert_eq!(main_set.set_hash, volume_set.set_hash);
}

#[test]
fn real_par1_fixture_verifies_intact_files() {
    let fixture_dir = Path::new("tests/fixtures/par1/flatdata");
    let results = verify_par1_file(&fixture_dir.join("testdata.par")).unwrap();

    assert_eq!(results.files.len(), 10);
    assert_eq!(results.present_file_count, 10);
    assert_eq!(results.missing_file_count, 0);
    assert_eq!(results.corrupted_file_count, 0);
    assert!(results
        .files
        .iter()
        .all(|file| file.status == FileStatus::Present));
}

#[test]
fn real_par1_fixture_verifies_from_volume_input() {
    let fixture_dir = Path::new("tests/fixtures/par1/flatdata");
    let results = verify_par1_file(&fixture_dir.join("testdata.p01")).unwrap();

    assert_eq!(results.files.len(), 10);
    assert_eq!(results.present_file_count, 10);
}

#[test]
fn par1_verify_purge_on_intact_set_deletes_only_recovery_files() {
    let temp = tempfile::tempdir().unwrap();
    copy_real_par1_fixture(&temp);

    let results = verify_par1_file_with_options(
        &temp.path().join("testdata.par"),
        &Par1VerifyOptions {
            purge: true,
            ..Par1VerifyOptions::default()
        },
    )
    .unwrap();

    assert_eq!(results.present_file_count, 10);
    assert!(!temp.path().join("testdata.par").exists());
    assert!(!temp.path().join("testdata.p01").exists());
    assert!(!temp.path().join("testdata.p02").exists());
    assert!(temp.path().join("test-0.data").exists());
}

#[test]
fn par1_verify_purge_with_missing_file_keeps_recovery_files() {
    let temp = tempfile::tempdir().unwrap();
    copy_real_par1_fixture(&temp);
    std::fs::remove_file(temp.path().join("test-0.data")).unwrap();

    let results = verify_par1_file_with_options(
        &temp.path().join("testdata.par"),
        &Par1VerifyOptions {
            purge: true,
            ..Par1VerifyOptions::default()
        },
    )
    .unwrap();

    assert_eq!(results.missing_file_count, 1);
    assert!(temp.path().join("testdata.par").exists());
    assert!(temp.path().join("testdata.p01").exists());
    assert!(temp.path().join("testdata.p02").exists());
}

#[test]
fn par1_repair_purge_after_reconstruction_deletes_recovery_files_only() {
    let temp = tempfile::tempdir().unwrap();
    copy_real_par1_fixture(&temp);
    std::fs::remove_file(temp.path().join("test-3.data")).unwrap();

    let results = repair_par1_file_with_options(
        &temp.path().join("testdata.par"),
        &Par1RepairOptions {
            purge: true,
            ..Par1RepairOptions::default()
        },
    )
    .unwrap();

    assert_eq!(results.present_file_count, 10);
    assert!(!temp.path().join("testdata.par").exists());
    assert!(!temp.path().join("testdata.p01").exists());
    assert!(!temp.path().join("testdata.p02").exists());
    assert!(temp.path().join("test-3.data").exists());
}

#[test]
fn par1_repair_purge_after_rename_deletes_recovery_files_and_created_backups() {
    let fixture_dir = Path::new("tests/fixtures/par1/flatdata");
    let temp = tempfile::tempdir().unwrap();
    copy_real_par1_fixture(&temp);
    let target = temp.path().join("test-2.data");
    let extra = temp.path().join("renamed.data");
    std::fs::copy(fixture_dir.join("test-2.data"), &extra).unwrap();
    std::fs::write(&target, b"corrupted").unwrap();

    let results = repair_par1_file_with_options(
        &temp.path().join("testdata.par"),
        &Par1RepairOptions {
            extra_files: vec![extra.clone()],
            purge: true,
            ..Par1RepairOptions::default()
        },
    )
    .unwrap();

    assert_eq!(results.present_file_count, 10);
    assert_eq!(
        std::fs::read(&target).unwrap(),
        std::fs::read(fixture_dir.join("test-2.data")).unwrap()
    );
    assert!(!temp.path().join("testdata.par").exists());
    assert!(!temp.path().join("testdata.p01").exists());
    assert!(!temp.path().join("testdata.p02").exists());
    assert!(!temp.path().join("test-2.data.1").exists());
    assert!(!extra.exists());
}

#[test]
fn par1_repair_failure_with_purge_keeps_recovery_files() {
    let temp = tempfile::tempdir().unwrap();
    copy_real_par1_fixture(&temp);
    std::fs::remove_file(temp.path().join("testdata.p01")).unwrap();
    std::fs::remove_file(temp.path().join("testdata.p02")).unwrap();
    std::fs::remove_file(temp.path().join("test-1.data")).unwrap();

    let error = repair_par1_file_with_options(
        &temp.path().join("testdata.par"),
        &Par1RepairOptions {
            purge: true,
            ..Par1RepairOptions::default()
        },
    )
    .unwrap_err();

    assert!(matches!(
        error,
        par2rs::par1::repair::Par1RepairError::NotEnoughRecoveryBlocks
    ));
    assert!(temp.path().join("testdata.par").exists());
    assert!(temp.path().join("test-0.data").exists());
}
