use par2rs::domain::{FileId, Md5Hash, RecoverySetId};
use par2rs::packets::{FileDescriptionPacket, MainPacket, Packet, RecoverySliceMetadata};
use par2rs::repair::{RepairContext, SilentReporter};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

// Helper to create a basic main packet
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

// Helper to create a file description packet
fn create_file_desc(file_id: FileId, name: &str, length: u64) -> FileDescriptionPacket {
    FileDescriptionPacket {
        packet_type: *b"PAR 2.0\0FileDesc",
        length: 120 + name.len() as u64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([1; 16]),
        file_id,
        md5_hash: Md5Hash::new([2; 16]),
        md5_16k: Md5Hash::new([3; 16]),
        file_length: length,
        file_name: name.as_bytes().to_vec(),
    }
}

#[test]
fn test_repair_context_new_basic() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 1024)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf());
    assert!(context.is_ok());

    let ctx = context.unwrap();
    assert_eq!(ctx.recovery_set.files.len(), 1);
    assert_eq!(ctx.recovery_set.files[0].file_name, "test.txt");
    assert_eq!(ctx.recovery_set.files[0].file_length, 1024);
}

#[test]
fn test_repair_context_new_no_main_packet() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![Packet::FileDescription(create_file_desc(
        file_id, "test.txt", 1024,
    ))];

    let context = RepairContext::new(packets, dir.path().to_path_buf());
    assert!(context.is_err());
}

#[test]
fn test_repair_context_new_no_file_descriptions() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![Packet::Main(create_main_packet(vec![file_id]))];

    let context = RepairContext::new(packets, dir.path().to_path_buf());
    assert!(context.is_err());
}

#[test]
fn test_repair_context_multiple_files() {
    let dir = TempDir::new().unwrap();
    let file_id1 = FileId::new([1; 16]);
    let file_id2 = FileId::new([2; 16]);
    let file_id3 = FileId::new([3; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id1, file_id2, file_id3])),
        Packet::FileDescription(create_file_desc(file_id1, "file1.txt", 10000)),
        Packet::FileDescription(create_file_desc(file_id2, "file2.txt", 20000)),
        Packet::FileDescription(create_file_desc(file_id3, "file3.txt", 30000)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf());
    assert!(context.is_ok());

    let ctx = context.unwrap();
    assert_eq!(ctx.recovery_set.files.len(), 3);
    assert_eq!(ctx.recovery_set.files[0].file_name, "file1.txt");
    assert_eq!(ctx.recovery_set.files[1].file_name, "file2.txt");
    assert_eq!(ctx.recovery_set.files[2].file_name, "file3.txt");
}

#[test]
fn test_repair_context_slice_count_calculation() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    // File size that is not evenly divisible by slice size
    // slice_size = 16384, file_length = 50000
    // Expected slices: 50000 / 16384 = 3.051... -> 4 slices
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 50000)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    assert_eq!(context.recovery_set.files[0].slice_count, 4);
}

#[test]
fn test_repair_context_global_slice_offset() {
    let dir = TempDir::new().unwrap();
    let file_id1 = FileId::new([1; 16]);
    let file_id2 = FileId::new([2; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id1, file_id2])),
        Packet::FileDescription(create_file_desc(file_id1, "file1.txt", 16384)), // 1 slice
        Packet::FileDescription(create_file_desc(file_id2, "file2.txt", 32768)), // 2 slices
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();

    // First file starts at offset 0
    assert_eq!(
        context.recovery_set.files[0].global_slice_offset.as_usize(),
        0
    );
    // Second file starts at offset 1 (after first file's 1 slice)
    assert_eq!(
        context.recovery_set.files[1].global_slice_offset.as_usize(),
        1
    );
}

#[test]
fn test_repair_context_missing_file_description() {
    let dir = TempDir::new().unwrap();
    let file_id1 = FileId::new([1; 16]);
    let file_id2 = FileId::new([2; 16]);

    // Main packet references file_id2, but only file_id1 description is provided
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id1, file_id2])),
        Packet::FileDescription(create_file_desc(file_id1, "file1.txt", 1024)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf());
    assert!(context.is_err());
}

#[test]
fn test_repair_context_with_null_terminated_filename() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let mut file_desc = create_file_desc(file_id, "test.txt", 1024);
    // Add null terminator to filename
    file_desc.file_name = b"test.txt\0\0\0".to_vec();

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(file_desc),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    // Should trim null terminators
    assert_eq!(context.recovery_set.files[0].file_name, "test.txt");
}

#[test]
fn test_repair_context_base_path() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 1024)),
    ];

    let base_path = dir.path().to_path_buf();
    let context = RepairContext::new(packets, base_path.clone()).unwrap();
    assert_eq!(context.base_path, base_path);
}

#[test]
fn test_repair_context_empty_file() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "empty.txt", 0)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    assert_eq!(context.recovery_set.files[0].file_length, 0);
    assert_eq!(context.recovery_set.files[0].slice_count, 0);
}

#[test]
fn test_repair_context_large_file() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    // 100 MB file
    let file_size = 100 * 1024 * 1024u64;
    let slice_size = 16384u64;
    let expected_slices = (file_size + slice_size - 1) / slice_size;

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "large.bin", file_size)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    assert_eq!(
        context.recovery_set.files[0].slice_count,
        expected_slices as usize
    );
}

#[test]
fn test_repair_context_file_ordering() {
    let dir = TempDir::new().unwrap();
    let file_id1 = FileId::new([1; 16]);
    let file_id2 = FileId::new([2; 16]);
    let file_id3 = FileId::new([3; 16]);

    // Create packets with descriptions in different order than main packet
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id2, file_id1, file_id3])),
        Packet::FileDescription(create_file_desc(file_id1, "file1.txt", 1024)),
        Packet::FileDescription(create_file_desc(file_id2, "file2.txt", 2048)),
        Packet::FileDescription(create_file_desc(file_id3, "file3.txt", 3072)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();

    // Files should be ordered according to main packet's file_ids array
    assert_eq!(context.recovery_set.files[0].file_name, "file2.txt");
    assert_eq!(context.recovery_set.files[1].file_name, "file1.txt");
    assert_eq!(context.recovery_set.files[2].file_name, "file3.txt");
}

#[test]
fn test_repair_context_recovery_set_id() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 1024)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    assert_eq!(context.recovery_set.set_id, RecoverySetId::new([1; 16]));
}

#[test]
fn test_repair_context_slice_size() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 1024)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    assert_eq!(context.recovery_set.slice_size, 16384);
}

#[test]
fn test_repair_context_unicode_filename() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let unicode_name = "测试文件.txt";
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, unicode_name, 1024)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    assert_eq!(context.recovery_set.files[0].file_name, unicode_name);
}

#[test]
fn test_repair_context_with_metadata() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 1024)),
    ];

    // Empty metadata for simplicity
    let metadata = vec![];

    let context = RepairContext::new_with_metadata(packets, metadata, dir.path().to_path_buf());
    assert!(context.is_ok());
}

#[test]
fn test_repair_context_file_count_matches() {
    let dir = TempDir::new().unwrap();
    let file_id1 = FileId::new([1; 16]);
    let file_id2 = FileId::new([2; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id1, file_id2])),
        Packet::FileDescription(create_file_desc(file_id1, "file1.txt", 1024)),
        Packet::FileDescription(create_file_desc(file_id2, "file2.txt", 2048)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    assert_eq!(context.recovery_set.files.len(), 2);
}

#[test]
fn test_repair_context_new_with_reporter() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 1024)),
    ];

    let reporter = Box::new(SilentReporter);
    let context = RepairContext::new_with_reporter(packets, dir.path().to_path_buf(), reporter);
    assert!(context.is_ok());

    let ctx = context.unwrap();
    assert_eq!(ctx.recovery_set.files.len(), 1);
}

#[test]
fn test_repair_context_new_with_metadata_and_reporter() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 1024)),
    ];

    let metadata = vec![];
    let reporter = Box::new(SilentReporter);

    let context = RepairContext::new_with_metadata_and_reporter(
        packets,
        metadata,
        dir.path().to_path_buf(),
        reporter,
    );
    assert!(context.is_ok());
}

#[test]
fn test_repair_context_purge_files_no_backups() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    // Create a PAR2 file
    let par2_file = dir.path().join("test.par2");
    fs::write(&par2_file, b"dummy par2 data").unwrap();

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 1024)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();

    // Should not error even if no backup files exist
    let result = context.purge_files(par2_file.to_str().unwrap());
    assert!(result.is_ok());

    // PAR2 file should be deleted
    assert!(!par2_file.exists());
}

#[test]
fn test_repair_context_purge_files_with_backups() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    // Create main file and backups with replaced extensions
    let main_file = dir.path().join("test.txt");
    let backup_1 = dir.path().join("test.1"); // with_extension replaces .txt with .1
    let backup_bak = dir.path().join("test.bak"); // with_extension replaces .txt with .bak

    fs::write(&main_file, b"main").unwrap();
    fs::write(&backup_1, b"backup1").unwrap();
    fs::write(&backup_bak, b"backup bak").unwrap();

    // Create PAR2 file
    let par2_file = dir.path().join("test.par2");
    fs::write(&par2_file, b"dummy par2").unwrap();

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 1024)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();

    let result = context.purge_files(par2_file.to_str().unwrap());
    assert!(result.is_ok());

    // Backup files should be deleted
    assert!(!backup_1.exists());
    assert!(!backup_bak.exists());

    // Main file should still exist
    assert!(main_file.exists());

    // PAR2 file should be deleted
    assert!(!par2_file.exists());
}

#[test]
fn test_repair_context_purge_multiple_par2_files() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    // Create multiple PAR2 files
    let par2_main = dir.path().join("test.par2");
    let par2_vol1 = dir.path().join("test.vol01+02.par2");
    let par2_vol2 = dir.path().join("test.vol03+04.par2");

    fs::write(&par2_main, b"main").unwrap();
    fs::write(&par2_vol1, b"vol1").unwrap();
    fs::write(&par2_vol2, b"vol2").unwrap();

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 1024)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();

    let result = context.purge_files(par2_main.to_str().unwrap());
    assert!(result.is_ok());

    // All PAR2 files should be deleted
    assert!(!par2_main.exists());
    assert!(!par2_vol1.exists());
    assert!(!par2_vol2.exists());
}

#[test]
fn test_repair_context_slice_count_calc_verification() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    // File size: 50000 bytes, slice size: 16384
    // Expected slices: ceil(50000 / 16384) = 4
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt", 50000)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    assert_eq!(context.recovery_set.files[0].slice_count, 4);
}

#[test]
fn test_repair_context_global_offset_multifile() {
    let dir = TempDir::new().unwrap();
    let file_id1 = FileId::new([1; 16]);
    let file_id2 = FileId::new([2; 16]);

    // File 1: 50000 bytes = 4 slices
    // File 2: 30000 bytes = 2 slices
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id1, file_id2])),
        Packet::FileDescription(create_file_desc(file_id1, "file1.txt", 50000)),
        Packet::FileDescription(create_file_desc(file_id2, "file2.txt", 30000)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();

    // File 1 should start at offset 0
    assert_eq!(
        context.recovery_set.files[0].global_slice_offset.as_usize(),
        0
    );
    assert_eq!(context.recovery_set.files[0].slice_count, 4);

    // File 2 should start at offset 4 (after file 1's slices)
    assert_eq!(
        context.recovery_set.files[1].global_slice_offset.as_usize(),
        4
    );
    assert_eq!(context.recovery_set.files[1].slice_count, 2);
}

#[test]
fn test_repair_context_many_files_debug_output() {
    let dir = TempDir::new().unwrap();

    // Create 10 files to test debug output truncation
    let mut file_ids = Vec::new();
    let mut packets = Vec::new();

    for i in 0..10 {
        let file_id = FileId::new([i as u8; 16]);
        file_ids.push(file_id);
        packets.push(Packet::FileDescription(create_file_desc(
            file_id,
            &format!("file{}.txt", i),
            1024,
        )));
    }

    packets.insert(0, Packet::Main(create_main_packet(file_ids)));

    let context = RepairContext::new(packets, dir.path().to_path_buf());
    assert!(context.is_ok());

    let ctx = context.unwrap();
    assert_eq!(ctx.recovery_set.files.len(), 10);
}

#[test]
fn test_repair_context_unicode_filename_handling() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "测试文件.txt", 1024)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    assert_eq!(context.recovery_set.files[0].file_name, "测试文件.txt");
}

#[test]
fn test_repair_context_zero_length_file_handling() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "empty.txt", 0)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    assert_eq!(context.recovery_set.files[0].slice_count, 0);
}

#[test]
fn test_repair_context_exactly_one_slice() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    // Exactly one slice worth of data
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "exact.txt", 16384)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    assert_eq!(context.recovery_set.files[0].slice_count, 1);
}

#[test]
fn test_repair_context_missing_file_description_for_id() {
    let dir = TempDir::new().unwrap();
    let file_id1 = FileId::new([1; 16]);
    let file_id2 = FileId::new([2; 16]);

    // Main packet references file_id2, but we only provide FileDescription for file_id1
    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id1, file_id2])),
        Packet::FileDescription(create_file_desc(file_id1, "file1.txt", 1024)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf());
    assert!(context.is_err());
}

#[test]
fn test_repair_context_file_name_with_null_bytes() {
    let dir = TempDir::new().unwrap();
    let file_id = FileId::new([1; 16]);

    let packets = vec![
        Packet::Main(create_main_packet(vec![file_id])),
        Packet::FileDescription(create_file_desc(file_id, "test.txt\0\0\0", 1024)),
    ];

    let context = RepairContext::new(packets, dir.path().to_path_buf()).unwrap();
    // Null bytes should be trimmed
    assert_eq!(context.recovery_set.files[0].file_name, "test.txt");
}
