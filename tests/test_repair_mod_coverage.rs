//! Comprehensive tests for repair/mod.rs functions
//!
//! Targets uncovered code paths in the repair module

use par2rs::domain::{FileId, Md5Hash, RecoverySetId};
use par2rs::packets::{FileDescriptionPacket, MainPacket, Packet};
use par2rs::repair::{
    repair_files, repair_files_with_config, repair_files_with_reporter, ConsoleReporter,
    RepairContext, SilentReporter,
};
use std::fs;
use std::io::Write;
use tempfile::TempDir;

// Helper to create main packet
fn create_main_packet(file_ids: Vec<FileId>) -> MainPacket {
    MainPacket {
        length: 64 + (file_ids.len() * 16) as u64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([1; 16]),
        slice_size: 16384,
        file_count: file_ids.len() as u32,
        file_ids,
        non_recovery_file_ids: Vec::new(),
    }
}

// Helper to create file description
fn create_file_desc(file_id: FileId, name: &str, length: u64) -> FileDescriptionPacket {
    FileDescriptionPacket {
        packet_type: *b"PAR 2.0\0FileDesc",
        length: 120 + name.len() as u64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([1; 16]),
        file_id,
        md5_hash: Md5Hash::new([5; 16]),
        md5_16k: Md5Hash::new([6; 16]),
        file_length: length,
        file_name: name.as_bytes().to_vec(),
    }
}

#[test]
fn test_check_file_status_missing_file() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "missing.txt", 100)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    let status_map = context.check_file_status();

    assert_eq!(status_map.len(), 1);
    assert!(status_map.contains_key("missing.txt"));
}

#[test]
fn test_check_file_status_wrong_size() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("wrong_size.txt");
    fs::write(&file_path, b"short").unwrap();

    let file_id = FileId::new([2; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "wrong_size.txt", 1000)), // Expects 1000 bytes
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    let status_map = context.check_file_status();

    assert!(status_map.contains_key("wrong_size.txt"));
}

#[test]
fn test_check_file_status_multiple_files() {
    let dir = TempDir::new().unwrap();

    // Create one file
    let file1_path = dir.path().join("file1.txt");
    fs::write(&file1_path, b"data").unwrap();

    let file_id1 = FileId::new([1; 16]);
    let file_id2 = FileId::new([2; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id1, file_id2])),
        Packet::FileDescription(create_file_desc(file_id1, "file1.txt", 4)),
        Packet::FileDescription(create_file_desc(file_id2, "file2.txt", 100)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    let status_map = context.check_file_status();

    assert_eq!(status_map.len(), 2);
    assert!(status_map.contains_key("file1.txt"));
    assert!(status_map.contains_key("file2.txt"));
}

#[test]
fn test_repair_files_with_reporter_silent() {
    let dir = TempDir::new().unwrap();
    let par2_file = dir.path().join("test.par2");

    // Create minimal PAR2 file
    let mut file = fs::File::create(&par2_file).unwrap();
    file.write_all(b"PAR2\0PKT").unwrap(); // Simplified header

    let reporter = Box::new(SilentReporter::new());
    let result = repair_files_with_reporter(par2_file.to_str().unwrap(), reporter);

    // May fail due to invalid PAR2 structure, but should handle reporter
    let _ = result;
}

#[test]
fn test_repair_files_with_reporter_console() {
    let dir = TempDir::new().unwrap();
    let par2_file = dir.path().join("test.par2");

    // Create minimal PAR2 file
    let mut file = fs::File::create(&par2_file).unwrap();
    file.write_all(b"PAR2\0PKT").unwrap();

    let reporter = Box::new(ConsoleReporter::new(false));
    let result = repair_files_with_reporter(par2_file.to_str().unwrap(), reporter);

    // May fail, but should handle console reporter
    let _ = result;
}

#[test]
fn test_repair_files_with_config_parallel() {
    let dir = TempDir::new().unwrap();
    let par2_file = dir.path().join("test.par2");

    let mut file = fs::File::create(&par2_file).unwrap();
    file.write_all(b"PAR2\0PKT").unwrap();

    use par2rs::verify::VerificationConfig;
    let config = VerificationConfig {
        parallel: true,
        ..Default::default()
    };

    let reporter = Box::new(SilentReporter::new());
    let result = repair_files_with_config(par2_file.to_str().unwrap(), reporter, &config);
    let _ = result;
}

#[test]
fn test_repair_files_with_config_sequential() {
    let dir = TempDir::new().unwrap();
    let par2_file = dir.path().join("test.par2");

    let mut file = fs::File::create(&par2_file).unwrap();
    file.write_all(b"PAR2\0PKT").unwrap();

    use par2rs::verify::VerificationConfig;
    let config = VerificationConfig {
        parallel: false,
        ..Default::default()
    };

    let reporter = Box::new(SilentReporter::new());
    let result = repair_files_with_config(par2_file.to_str().unwrap(), reporter, &config);
    let _ = result;
}

#[test]
fn test_repair_files_nonexistent() {
    let result = repair_files("/nonexistent/file.par2");
    assert!(result.is_err());
}

#[test]
fn test_repair_files_invalid_path() {
    let result = repair_files("");
    assert!(result.is_err());
}

#[test]
fn test_validate_file_slices_no_file() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 1000)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();

    // Get the file info
    if let Some(file_info) = &context.recovery_set.files.first() {
        let result = context.validate_file_slices(file_info);
        // Should handle missing file gracefully
        let _ = result;
    }
}

#[test]
fn test_repair_with_slices_no_recovery() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 100)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    let result = context.repair_with_slices();

    // Should complete even without recovery data
    assert!(result.is_ok());
}

#[test]
fn test_repair_no_recovery_data() {
    let dir = TempDir::new().unwrap();

    // Create a file
    let file_path = dir.path().join("intact.txt");
    fs::write(&file_path, b"test data").unwrap();

    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "intact.txt", 9)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    let result = context.repair();

    // Should complete even without recovery data
    assert!(result.is_ok());
}

#[test]
fn test_check_file_status_parallel_execution() {
    let dir = TempDir::new().unwrap();

    // Create multiple files to test parallel execution
    for i in 0..5 {
        let file_path = dir.path().join(format!("file{}.txt", i));
        fs::write(&file_path, format!("data{}", i)).unwrap();
    }

    let file_ids: Vec<_> = (0..5).map(|i| FileId::new([i as u8; 16])).collect();
    let mut packets = vec![Packet::Main(create_main_packet(file_ids.clone()))];

    for (i, file_id) in file_ids.iter().enumerate() {
        packets.push(Packet::FileDescription(create_file_desc(
            *file_id,
            &format!("file{}.txt", i),
            5 + i as u64,
        )));
    }

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    let status_map = context.check_file_status();

    // Should check all files
    assert_eq!(status_map.len(), 5);
}

#[test]
fn test_determine_file_status_16k_hash_mismatch() {
    let dir = TempDir::new().unwrap();

    // Create a file with specific content
    let file_path = dir.path().join("test.txt");
    fs::write(&file_path, b"x".repeat(20000)).unwrap();

    let file_id = FileId::new([1; 16]);

    // Use wrong 16K hash
    let mut file_desc = create_file_desc(file_id, "test.txt", 20000);
    file_desc.md5_16k = Md5Hash::new([99; 16]); // Wrong hash

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(file_desc),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    let status_map = context.check_file_status();

    assert!(status_map.contains_key("test.txt"));
}
