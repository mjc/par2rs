use par2rs::par2_files;
/// Tests for specific bugs found during repair implementation
/// These tests document and prevent regression of critical bugs discovered during development
use par2rs::repair::RepairContext;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use tempfile::TempDir;

/// Test environment with PAR2 files
struct TestEnv {
    #[allow(dead_code)]
    temp_dir: TempDir,
    test_file: PathBuf,
    par2_file: PathBuf,
}

impl TestEnv {
    fn new() -> Self {
        let temp_dir = TempDir::new().unwrap();
        let fixtures = PathBuf::from("tests/fixtures");

        // Copy test files
        fs::copy(fixtures.join("testfile"), temp_dir.path().join("testfile")).unwrap();
        for entry in fs::read_dir(&fixtures).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("par2") {
                fs::copy(&path, temp_dir.path().join(path.file_name().unwrap())).unwrap();
            }
        }

        let test_file = temp_dir.path().join("testfile");
        let par2_file = temp_dir.path().join("testfile.par2");

        TestEnv {
            temp_dir,
            test_file,
            par2_file,
        }
    }

    fn load_context(&self) -> RepairContext {
        let par2_files = par2_files::collect_par2_files(&self.par2_file);
        let metadata = par2_files::parse_recovery_slice_metadata(&par2_files, false);
        let packets = par2_files::load_par2_packets(&par2_files, false);
        RepairContext::new_with_metadata(packets, metadata, self.temp_dir.path().to_path_buf())
            .unwrap()
    }

    fn corrupt_at(&self, offset: u64, data: &[u8]) {
        let mut file = File::options().write(true).open(&self.test_file).unwrap();
        file.seek(SeekFrom::Start(offset)).unwrap();
        file.write_all(data).unwrap();
    }

    fn corrupt_slice(&self, slice_index: usize, data: &[u8]) {
        let context = self.load_context();
        let slice_size = context.recovery_set.slice_size;
        self.corrupt_at(slice_index as u64 * slice_size, data);
    }

    fn read_file(&self) -> Vec<u8> {
        let mut contents = Vec::new();
        File::open(&self.test_file)
            .unwrap()
            .read_to_end(&mut contents)
            .unwrap();
        contents
    }

    fn count_corrupted(&self) -> usize {
        let context = self.load_context();
        let file_info = &context.recovery_set.files[0];
        // Validate slices and count how many are invalid
        let valid_slices = context.validate_file_slices(file_info).unwrap();
        file_info.slice_count - valid_slices.len()
    }

    fn repair(&self) -> par2rs::repair::RepairResult {
        let _ = env_logger::builder().is_test(true).try_init();
        match self.load_context().repair() {
            Ok(result) => {
                if !result.is_success() {
                    eprintln!("Repair returned failure: {:?}", result);
                }
                result
            }
            Err(e) => panic!("Repair failed with error: {}", e),
        }
    }

    fn verify_md5(&self) -> bool {
        use md_5::Digest;
        let context = self.load_context();
        let file_info = &context.recovery_set.files[0];
        let contents = self.read_file();
        let computed: [u8; 16] = md_5::Md5::digest(&contents).into();
        computed == file_info.md5_hash
    }
}

#[test]
fn test_bug_last_slice_padding_in_count() {
    // BUG: count_corrupted_slices was not padding the last slice with zeros before computing MD5
    // This caused the last slice to always be marked as corrupted when it was actually valid
    //
    // Symptom: "Found 1983 of 1986 data blocks" when only 2 slices (1 and 2) were corrupted,
    //          not 3 slices. Slice 1985 (the last slice, 496 bytes) was incorrectly flagged.
    //
    // Root cause: count_corrupted_slices read only actual_slice_size bytes instead of
    //            full slice_size buffer padded with zeros (PAR2 spec requirement)
    //
    // Fix: Allocate full slice_size buffer, read actual data, leave rest zero-padded

    let env = TestEnv::new();

    // Corrupt slices 1 and 2 (100 bytes at offset 1000)
    // With slice_size=528: slice 1 is bytes 528-1055, slice 2 is bytes 1056-1583
    // Corruption at 1000-1099 affects both slices
    env.corrupt_at(1000, &[0u8; 100]);

    // Should detect exactly 2 corrupted slices, not 3
    let corrupted_count = env.count_corrupted();
    assert_eq!(
        corrupted_count, 2,
        "Expected 2 corrupted slices (1 and 2), but got {}. \
         This indicates the last slice padding bug has regressed.",
        corrupted_count
    );
}

// Test removed - load_slices_except_file() no longer exists
// Chunked reconstruction handles slice loading differently

#[test]
fn test_bug_reconstruct_slices_needs_valid_slices() {
    // BUG: reconstruct_slices() was loading only MD5-verified slices from load_all_slices(),
    //      which excluded the corrupted slices from the damaged file. Reed-Solomon needs
    //      ALL valid slices from non-corrupted positions, not just MD5-verified ones.
    //
    // Symptom: "Total slices available: 1984" when we need 1983 valid + 3 recovery = repair
    //          Reed-Solomon reconstruction got wrong input data.
    //
    // Root cause: Didn't pass current_file_slices (which has the actual valid slices we loaded)
    //            to the reconstruction function
    //
    // Fix: Pass current_file_slices to reconstruct_slices() and use those as the base,
    //     only adding slices from OTHER files in the recovery set

    let env = TestEnv::new();
    env.corrupt_at(1000, &[0u8; 100]);

    let result = env.repair();
    assert!(
        result.is_success(),
        "Repair should succeed with 2 corrupted slices and 99 recovery blocks"
    );
    assert_eq!(
        result.repaired_files().len(),
        1,
        "Should repair exactly 1 file"
    );

    assert!(
        env.verify_md5(),
        "Repaired file MD5 should match expected. \
         This indicates the reconstruct_slices input bug has regressed."
    );
}

#[test]
fn test_bug_repair_actually_writes_correct_data() {
    // BUG: repair() was calling reconstruct_slices() and write_repaired_file(),
    //      printing "Repair complete" but the file remained damaged.
    //
    // Symptom: "Repair complete." printed, but verification shows "Target: testfile - damaged"
    //          and par2cmdline also confirms file is still broken
    //
    // Root cause: Combination of the above bugs - wrong slice counts, wrong input to Reed-Solomon,
    //            not excluding current file from load_all_slices
    //
    // Fix: All of the above fixes combined make repair actually work

    let env = TestEnv::new();
    let original = env.read_file();

    env.corrupt_at(1000, &[0xFFu8; 100]);
    let corrupted = env.read_file();
    assert_ne!(original, corrupted, "File should be corrupted");

    let result = env.repair();
    assert!(
        result.is_success(),
        "Repair should report success. Got: {:?}",
        result
    );

    let repaired = env.read_file();
    assert_eq!(
        repaired, original,
        "Repaired file should match original content byte-for-byte. \
         This indicates repair is not actually writing correct data."
    );
}

#[test]
fn test_bug_multiple_corrupted_slices_repair() {
    // BUG: Repair would fail or produce wrong results with multiple corrupted slices
    //
    // This test verifies that repair works correctly when multiple slices are corrupted
    // across different parts of the file

    let env = TestEnv::new();

    // Corrupt 5 different slices
    env.corrupt_slice(5, &vec![0xAAu8; 528]);
    env.corrupt_slice(10, &vec![0xBBu8; 528]);
    env.corrupt_slice(15, &vec![0xCCu8; 528]);
    env.corrupt_slice(20, &vec![0xDDu8; 528]);
    env.corrupt_slice(25, &vec![0xEEu8; 528]);

    assert_eq!(env.count_corrupted(), 5, "Should detect 5 corrupted slices");

    let result = env.repair();
    assert!(
        result.is_success(),
        "Repair should succeed with 5 corrupted slices"
    );
    assert_eq!(
        result.repaired_files().len(),
        1,
        "Should repair exactly 1 file"
    );

    assert!(
        env.verify_md5(),
        "Repaired file with 5 corrupted slices should have correct MD5"
    );
}

#[test]
fn test_bug_last_slice_reconstruction() {
    // BUG: The last slice (which is shorter than slice_size) might not be reconstructed correctly
    //      due to padding issues
    //
    // This specifically tests that corrupting the last slice can be repaired correctly

    let env = TestEnv::new();
    let original = env.read_file();

    // Get file info to find last slice
    let context = env.load_context();
    let file_info = &context.recovery_set.files[0];
    let last_slice_index = file_info.slice_count - 1;

    // Corrupt ONLY the last slice
    env.corrupt_slice(last_slice_index, &[0xFFu8; 100]);

    assert_eq!(
        env.count_corrupted(),
        1,
        "Should detect exactly 1 corrupted slice (the last one)"
    );

    let result = env.repair();
    assert!(
        result.is_success(),
        "Should successfully repair the last slice"
    );

    let repaired = env.read_file();
    assert_eq!(
        repaired, original,
        "Repaired file should match original, including correct last slice reconstruction"
    );
}
