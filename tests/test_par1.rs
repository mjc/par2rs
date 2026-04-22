use par2rs::par1::parser::parse_par1_bytes;
use par2rs::par1::verify::verify_par1_file;
use par2rs::verify::FileStatus;
use std::path::Path;

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
