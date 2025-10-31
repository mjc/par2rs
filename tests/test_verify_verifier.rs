use par2rs::domain::{FileId, Md5Hash, RecoverySetId};
use par2rs::packets::FileDescriptionPacket;
use par2rs::verify::{calculate_file_md5, calculate_file_md5_16k};
use par2rs::verify::{FileStatus, FileVerifier};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

// Helper to create a test file
fn create_test_file(dir: &TempDir, name: &str, content: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    let mut file = fs::File::create(&path).unwrap();
    file.write_all(content).unwrap();
    path
}

// Helper to create a FileDescription packet from a file
fn create_file_description(file_path: &std::path::Path, file_name: &str) -> FileDescriptionPacket {
    let metadata = fs::metadata(file_path).unwrap();
    let md5_full = calculate_file_md5(file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(file_path).unwrap();

    FileDescriptionPacket {
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([1; 16]),
        md5_hash: md5_full,
        md5_16k,
        file_length: metadata.len(),
        file_name: file_name.as_bytes().to_vec(),
        length: 120 + file_name.len() as u64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
    }
}

#[test]
fn test_file_verifier_new() {
    let dir = TempDir::new().unwrap();
    let _verifier = FileVerifier::new(dir.path());
    // Should create successfully
}

#[test]
fn test_determine_file_status_present() {
    let dir = TempDir::new().unwrap();
    let content = b"Hello, world!";
    let file_path = create_test_file(&dir, "test.txt", content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();
    let file_length = content.len() as u64;

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.determine_file_status("test.txt", &md5_16k, &md5_full, file_length);

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_determine_file_status_missing() {
    let dir = TempDir::new().unwrap();
    let verifier = FileVerifier::new(dir.path());

    let dummy_hash = Md5Hash::new([0; 16]);
    let status = verifier.determine_file_status("nonexistent.txt", &dummy_hash, &dummy_hash, 100);

    assert_eq!(status, FileStatus::Missing);
}

#[test]
fn test_determine_file_status_wrong_size() {
    let dir = TempDir::new().unwrap();
    let content = b"Hello, world!";
    let file_path = create_test_file(&dir, "test.txt", content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    // Pass wrong length
    let status = verifier.determine_file_status("test.txt", &md5_16k, &md5_full, 999);

    assert_eq!(status, FileStatus::Corrupted);
}

#[test]
fn test_determine_file_status_wrong_16k_hash() {
    let dir = TempDir::new().unwrap();
    let content = b"Hello, world!";
    let file_path = create_test_file(&dir, "test.txt", content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let file_length = content.len() as u64;

    let verifier = FileVerifier::new(dir.path());
    let wrong_hash = Md5Hash::new([0xFF; 16]);
    let status = verifier.determine_file_status("test.txt", &wrong_hash, &md5_full, file_length);

    assert_eq!(status, FileStatus::Corrupted);
}

#[test]
fn test_determine_file_status_wrong_full_hash() {
    let dir = TempDir::new().unwrap();
    let content = b"Hello, world!";
    let file_path = create_test_file(&dir, "test.txt", content);

    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();
    let file_length = content.len() as u64;

    let verifier = FileVerifier::new(dir.path());
    let wrong_hash = Md5Hash::new([0xFF; 16]);
    let status = verifier.determine_file_status("test.txt", &md5_16k, &wrong_hash, file_length);

    assert_eq!(status, FileStatus::Corrupted);
}

#[test]
fn test_verify_file_from_description_present() {
    let dir = TempDir::new().unwrap();
    let content = b"Test file content";
    let file_path = create_test_file(&dir, "test.txt", content);

    let file_desc = create_file_description(&file_path, "test.txt");
    let verifier = FileVerifier::new(dir.path());

    let status = verifier.verify_file_from_description(&file_desc);
    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verify_file_from_description_missing() {
    let dir = TempDir::new().unwrap();

    let file_desc = FileDescriptionPacket {
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([1; 16]),
        md5_hash: Md5Hash::new([0; 16]),
        md5_16k: Md5Hash::new([0; 16]),
        file_length: 100,
        file_name: b"nonexistent.txt".to_vec(),
        length: 135,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
    };

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.verify_file_from_description(&file_desc);
    assert_eq!(status, FileStatus::Missing);
}

#[test]
fn test_verify_file_from_description_corrupted() {
    let dir = TempDir::new().unwrap();
    let content = b"Test file content";
    let file_path = create_test_file(&dir, "test.txt", content);

    let mut file_desc = create_file_description(&file_path, "test.txt");
    // Corrupt the hash
    file_desc.md5_hash = Md5Hash::new([0xFF; 16]);

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.verify_file_from_description(&file_desc);
    assert_eq!(status, FileStatus::Corrupted);
}

#[test]
fn test_verifier_with_large_file() {
    let dir = TempDir::new().unwrap();
    // Create a file larger than 16KB
    let content = vec![0xAB; 20000];
    let file_path = create_test_file(&dir, "large.bin", &content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.determine_file_status("large.bin", &md5_16k, &md5_full, 20000);

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verifier_edge_case_exactly_16kb() {
    let dir = TempDir::new().unwrap();
    let content = vec![0x42; 16384]; // Exactly 16KB
    let file_path = create_test_file(&dir, "exact16k.bin", &content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.determine_file_status("exact16k.bin", &md5_16k, &md5_full, 16384);

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verifier_with_zero_byte_file() {
    let dir = TempDir::new().unwrap();
    let content = vec![];
    let file_path = create_test_file(&dir, "empty.txt", &content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.determine_file_status("empty.txt", &md5_16k, &md5_full, 0);

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verifier_with_subdirectory() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("subdir")).unwrap();
    let content = b"Nested file";
    let file_path = dir.path().join("subdir").join("nested.txt");
    let mut file = fs::File::create(&file_path).unwrap();
    file.write_all(content).unwrap();

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.determine_file_status(
        "subdir/nested.txt",
        &md5_16k,
        &md5_full,
        content.len() as u64,
    );

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verifier_binary_content() {
    let dir = TempDir::new().unwrap();
    let content: Vec<u8> = (0..=255).collect(); // All possible byte values
    let file_path = create_test_file(&dir, "binary.dat", &content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    let status =
        verifier.determine_file_status("binary.dat", &md5_16k, &md5_full, content.len() as u64);

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verifier_file_path_with_spaces() {
    let dir = TempDir::new().unwrap();
    let content = b"File with spaces";
    let file_path = create_test_file(&dir, "file with spaces.txt", content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.determine_file_status(
        "file with spaces.txt",
        &md5_16k,
        &md5_full,
        content.len() as u64,
    );

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_verifier_unicode_filename() {
    let dir = TempDir::new().unwrap();
    let content = b"Unicode test";
    let file_path = create_test_file(&dir, "тест_测试.txt", content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let md5_16k = calculate_file_md5_16k(&file_path).unwrap();

    let verifier = FileVerifier::new(dir.path());
    let status =
        verifier.determine_file_status("тест_测试.txt", &md5_16k, &md5_full, content.len() as u64);

    assert_eq!(status, FileStatus::Present);
}

#[test]
fn test_multiple_files_same_verifier() {
    let dir = TempDir::new().unwrap();

    let content1 = b"File 1";
    let file1 = create_test_file(&dir, "file1.txt", content1);
    let md5_1 = calculate_file_md5(&file1).unwrap();
    let md5_16k_1 = calculate_file_md5_16k(&file1).unwrap();

    let content2 = b"File 2 with different content";
    let file2 = create_test_file(&dir, "file2.txt", content2);
    let md5_2 = calculate_file_md5(&file2).unwrap();
    let md5_16k_2 = calculate_file_md5_16k(&file2).unwrap();

    let verifier = FileVerifier::new(dir.path());

    let status1 =
        verifier.determine_file_status("file1.txt", &md5_16k_1, &md5_1, content1.len() as u64);
    let status2 =
        verifier.determine_file_status("file2.txt", &md5_16k_2, &md5_2, content2.len() as u64);

    assert_eq!(status1, FileStatus::Present);
    assert_eq!(status2, FileStatus::Present);
}

#[test]
fn test_verifier_large_file_wrong_16k_hash() {
    let dir = TempDir::new().unwrap();
    let content = vec![0u8; 50000]; // Larger than 16KB
    let file_path = create_test_file(&dir, "large.bin", &content);

    let md5_full = calculate_file_md5(&file_path).unwrap();
    let wrong_md5_16k = Md5Hash::new([0xFF; 16]);

    let verifier = FileVerifier::new(dir.path());
    let status = verifier.determine_file_status(
        "large.bin",
        &wrong_md5_16k,
        &md5_full,
        content.len() as u64,
    );

    // Should be detected as corrupted based on 16KB hash alone
    assert_eq!(status, FileStatus::Corrupted);
}
