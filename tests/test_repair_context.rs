use par2rs::domain::{FileId, Md5Hash, RecoverySetId};
use par2rs::packets::{FileDescriptionPacket, MainPacket, Packet};
use par2rs::repair::RepairContext;
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
