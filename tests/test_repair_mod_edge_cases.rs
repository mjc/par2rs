//! Additional tests to improve repair/mod.rs coverage
//!
//! Focuses on error paths and edge cases in the main repair orchestration logic

use par2rs::domain::{FileId, Md5Hash, RecoverySetId};
use par2rs::packets::{FileDescriptionPacket, MainPacket, Packet};
use par2rs::repair::{RepairContext, SilentReporter};
use std::fs;
use tempfile::TempDir;

// Helper to create main packet
fn create_main_packet(file_ids: Vec<FileId>, slice_size: u64) -> MainPacket {
    MainPacket {
        length: 64 + (file_ids.len() * 16) as u64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([1; 16]),
        slice_size,
        file_count: file_ids.len() as u32,
        file_ids,
        non_recovery_file_ids: Vec::new(),
    }
}

// Helper to create file description
fn create_file_desc(
    file_id: FileId,
    name: &str,
    length: u64,
    md5: [u8; 16],
) -> FileDescriptionPacket {
    FileDescriptionPacket {
        packet_type: *b"PAR 2.0\0FileDesc",
        length: 120 + name.len() as u64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([1; 16]),
        file_id,
        md5_hash: Md5Hash::new(md5),
        md5_16k: Md5Hash::new([6; 16]),
        file_length: length,
        file_name: name.as_bytes().to_vec(),
    }
}

#[test]
fn test_check_file_status_metadata_error() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("test.txt");

    // Create file but make it unreadable by removing permissions
    fs::write(&file_path, b"test").unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&file_path).unwrap().permissions();
        perms.set_mode(0o000); // Remove all permissions
        fs::set_permissions(&file_path, perms).unwrap();
    }

    let file_id = FileId::new([1; 16]);
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 1024)),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 4, [0; 16])),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    let status_map = context.check_file_status();

    // File exists but metadata is unreadable -> should be Corrupted
    #[cfg(unix)]
    {
        use par2rs::repair::FileStatus;
        use std::os::unix::fs::PermissionsExt;
        assert!(matches!(
            status_map.get("test.txt"),
            Some(FileStatus::Corrupted)
        ));

        // Restore permissions for cleanup
        let mut perms = fs::metadata(&file_path).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&file_path, perms).unwrap();
    }
}

#[test]
fn test_check_file_status_16kb_md5_mismatch() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("mismatch.txt");

    // Write file with wrong content (will fail 16KB MD5 check)
    let content = vec![0xFF; 20000]; // 20KB of 0xFF
    fs::write(&file_path, &content).unwrap();

    let file_id = FileId::new([1; 16]);
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 1024)),
        Packet::FileDescription(create_file_desc(file_id, "mismatch.txt", 20000, [0; 16])),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    let status_map = context.check_file_status();

    use par2rs::repair::FileStatus;
    // Should detect corruption via 16KB MD5 check
    assert!(matches!(
        status_map.get("mismatch.txt"),
        Some(FileStatus::Corrupted)
    ));
}

#[test]
fn test_check_file_status_full_md5_check() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("correct.txt");

    // Write file with known content
    let content = b"test data";
    fs::write(&file_path, content).unwrap();

    // Calculate actual MD5 hashes
    let full_md5 = par2rs::checksum::compute_md5_bytes(content);
    let md5_16k = par2rs::checksum::compute_md5_bytes(&content[..content.len().min(16384)]);

    let file_id = FileId::new([1; 16]);
    let mut file_desc = create_file_desc(file_id, "correct.txt", content.len() as u64, full_md5);
    file_desc.md5_16k = Md5Hash::new(md5_16k);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 1024)),
        Packet::FileDescription(file_desc),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    let status_map = context.check_file_status();

    use par2rs::repair::FileStatus;
    // Should detect as Present via full MD5 check
    assert!(matches!(
        status_map.get("correct.txt"),
        Some(FileStatus::Present)
    ));
}

#[test]
fn test_check_file_status_16kb_match_but_full_md5_mismatch() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("partial_match.txt");

    // Write file where first 16KB might match but full content differs
    let mut content = vec![0x42; 20000];
    content[19999] = 0xFF; // Change last byte
    fs::write(&file_path, &content).unwrap();

    let file_id = FileId::new([1; 16]);
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 1024)),
        Packet::FileDescription(create_file_desc(
            file_id,
            "partial_match.txt",
            20000,
            [0xAA; 16],
        )),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    let status_map = context.check_file_status();

    use par2rs::repair::FileStatus;
    // Should detect as Corrupted (either via 16KB or full MD5)
    assert!(matches!(
        status_map.get("partial_match.txt"),
        Some(FileStatus::Corrupted)
    ));
}

#[test]
fn test_check_file_status_parallel_processing() {
    let dir = TempDir::new().unwrap();

    // Create multiple files to test parallel processing
    let mut file_ids = Vec::new();
    let mut packets = Vec::new();

    for i in 0..10 {
        let file_name = format!("file{}.txt", i);
        let file_path = dir.path().join(&file_name);
        let file_id = FileId::new([i as u8; 16]);
        file_ids.push(file_id);

        // Create some files, leave others missing
        if i % 2 == 0 {
            fs::write(&file_path, b"data").unwrap();
        }

        packets.push(Packet::FileDescription(create_file_desc(
            file_id, &file_name, 4, [0; 16],
        )));
    }

    packets.insert(0, Packet::Main(create_main_packet(file_ids, 1024)));

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    let status_map = context.check_file_status();

    // Should have status for all 10 files
    assert_eq!(status_map.len(), 10);

    use par2rs::repair::FileStatus;
    // Files with even indices should exist (but may be corrupted)
    // Files with odd indices should be missing
    for i in 0..10 {
        let file_name = format!("file{}.txt", i);
        assert!(status_map.contains_key(&file_name));

        if i % 2 == 1 {
            assert!(matches!(
                status_map.get(&file_name),
                Some(FileStatus::Missing)
            ));
        }
    }
}

#[test]
fn test_silent_reporter_usage() {
    // Test that SilentReporter can be used with RepairContext
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id], 1024)),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 100, [0; 16])),
    ];

    // Create context with SilentReporter
    let context = RepairContext::new_with_reporter(
        packets,
        dir.path().to_path_buf(),
        Box::new(SilentReporter::new()),
    )
    .unwrap();

    // Should work without printing anything
    let _status_map = context.check_file_status();
}

#[test]
fn test_file_status_needs_repair_helper() {
    use par2rs::repair::FileStatus;

    assert!(!FileStatus::Present.needs_repair());
    assert!(FileStatus::Missing.needs_repair());
    assert!(FileStatus::Corrupted.needs_repair());
}

#[test]
fn test_multiple_status_types_in_single_check() {
    let dir = TempDir::new().unwrap();

    let file1_id = FileId::new([1; 16]);
    let file2_id = FileId::new([2; 16]);
    let file3_id = FileId::new([3; 16]);

    // File 1: Missing
    // File 2: Wrong size (corrupted)
    // File 3: Present with correct content

    let file2_path = dir.path().join("file2.txt");
    fs::write(&file2_path, b"short").unwrap(); // Wrong size

    let file3_path = dir.path().join("file3.txt");
    let file3_content = b"correct content";
    fs::write(&file3_path, file3_content).unwrap();

    let file3_md5 = par2rs::checksum::compute_md5_bytes(file3_content);
    let file3_md5_16k =
        par2rs::checksum::compute_md5_bytes(&file3_content[..file3_content.len().min(16384)]);

    let mut file3_desc =
        create_file_desc(file3_id, "file3.txt", file3_content.len() as u64, file3_md5);
    file3_desc.md5_16k = Md5Hash::new(file3_md5_16k);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file1_id, file2_id, file3_id], 1024)),
        Packet::FileDescription(create_file_desc(file1_id, "file1.txt", 100, [0; 16])),
        Packet::FileDescription(create_file_desc(file2_id, "file2.txt", 1000, [0; 16])),
        Packet::FileDescription(file3_desc),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    let status_map = context.check_file_status();

    use par2rs::repair::FileStatus;
    assert_eq!(status_map.len(), 3);
    assert!(matches!(
        status_map.get("file1.txt"),
        Some(FileStatus::Missing)
    ));
    assert!(matches!(
        status_map.get("file2.txt"),
        Some(FileStatus::Corrupted)
    ));
    assert!(matches!(
        status_map.get("file3.txt"),
        Some(FileStatus::Present)
    ));
}
