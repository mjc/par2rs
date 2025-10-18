//! Tests for RecoverySliceMetadata lazy loading functionality

use par2rs::packets::RecoverySliceMetadata;
use par2rs::repair::RecoverySetId;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_recovery_slice_metadata_load_data() {
    // Create a temporary file with test recovery data
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test_recovery.par2");

    // Write some test data
    let test_data = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let mut file = File::create(&file_path).unwrap();

    // Write some header data first (64 bytes)
    let header = vec![0u8; 64];
    file.write_all(&header).unwrap();

    // Write the recovery data at offset 64
    file.write_all(&test_data).unwrap();
    file.flush().unwrap();
    drop(file);

    // Create metadata pointing to the recovery data
    let metadata = RecoverySliceMetadata::from_file(
        1,                             // exponent
        RecoverySetId::new([0u8; 16]), // set_id
        file_path.clone(),             // file_path
        64,                            // data_offset
        test_data.len(),               // data_size
    );

    // Load the data
    let loaded_data = metadata.load_data().unwrap();

    // Verify it matches
    assert_eq!(loaded_data, test_data);
}

#[test]
fn test_recovery_slice_metadata_large_data() {
    // Test with larger data size (5MB)
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test_large.par2");

    let data_size = 5 * 1024 * 1024; // 5MB
    let test_data: Vec<u8> = (0..data_size).map(|i| (i % 256) as u8).collect();

    let mut file = File::create(&file_path).unwrap();
    file.write_all(&test_data).unwrap();
    file.flush().unwrap();
    drop(file);

    let metadata = RecoverySliceMetadata::from_file(
        2,                             // exponent
        RecoverySetId::new([1u8; 16]), // set_id
        file_path.clone(),             // file_path
        0,                             // data_offset
        data_size,                     // data_size
    );

    let loaded_data = metadata.load_data().unwrap();
    assert_eq!(loaded_data.len(), data_size);
    assert_eq!(loaded_data[0], test_data[0]);
    assert_eq!(loaded_data[data_size - 1], test_data[data_size - 1]);
}

#[test]
fn test_recovery_slice_metadata_missing_file() {
    let metadata = RecoverySliceMetadata::from_file(
        1,                             // exponent
        RecoverySetId::new([0u8; 16]), // set_id
        PathBuf::from("/nonexistent/file.par2"),
        0,   // data_offset
        100, // data_size
    );

    // Should return an error
    assert!(metadata.load_data().is_err());
}

#[test]
fn test_recovery_slice_metadata_clone() {
    let metadata = RecoverySliceMetadata::from_file(
        5,                             // exponent
        RecoverySetId::new([2u8; 16]), // set_id
        PathBuf::from("/some/path.par2"),
        1024, // data_offset
        4096, // data_size
    );

    let cloned = metadata.clone();

    assert_eq!(cloned.exponent, metadata.exponent);
    assert_eq!(cloned.data_size(), metadata.data_size());
}

#[test]
fn test_metadata_memory_usage() {
    // Verify that RecoverySliceMetadata is much smaller than RecoverySlicePacket
    use par2rs::packets::RecoverySlicePacket;
    use std::mem::size_of;

    let metadata_size = size_of::<RecoverySliceMetadata>();

    // RecoverySliceMetadata should be small (just the struct fields, no Vec data)
    // PathBuf + u32 + u64 + usize + [u8;16] should be around 64-100 bytes
    assert!(
        metadata_size < 200,
        "RecoverySliceMetadata size: {} bytes",
        metadata_size
    );

    println!("RecoverySliceMetadata size: {} bytes", metadata_size);
    println!(
        "RecoverySlicePacket base size: {} bytes (excluding recovery_data Vec)",
        size_of::<RecoverySlicePacket>()
    );
}

#[test]
fn test_parse_from_reader() {
    use std::io::Cursor;

    // Create a minimal valid recovery slice packet header
    let mut packet_data = Vec::new();

    // Magic
    packet_data.extend_from_slice(b"PAR2\0PKT");

    // Length (68 + 100 = 168 bytes total)
    packet_data.extend_from_slice(&168u64.to_le_bytes());

    // MD5 (16 bytes)
    packet_data.extend_from_slice(&[0u8; 16]);

    // Set ID (16 bytes)
    packet_data.extend_from_slice(&[1u8; 16]);

    // Type
    packet_data.extend_from_slice(b"PAR 2.0\0RecvSlic");

    // Exponent
    packet_data.extend_from_slice(&5u32.to_le_bytes());

    // Recovery data (100 bytes)
    packet_data.extend_from_slice(&vec![0xAB; 100]);

    let mut cursor = Cursor::new(packet_data);
    let metadata =
        RecoverySliceMetadata::parse_from_reader(&mut cursor, PathBuf::from("/test/file.par2"))
            .unwrap();

    assert_eq!(metadata.exponent, 5);
    assert_eq!(metadata.data_size(), 100);
    // Can't test internal fields directly since they're in the loader
}

#[test]
fn test_metadata_load_chunk() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Create a temporary PAR2 file with a recovery slice packet
    let mut temp_file = NamedTempFile::new().unwrap();

    // Write packet header
    temp_file.write_all(b"PAR2\0PKT").unwrap();
    temp_file.write_all(&168u64.to_le_bytes()).unwrap(); // length: 68 + 100
    temp_file.write_all(&[0u8; 16]).unwrap(); // md5
    temp_file.write_all(&[1u8; 16]).unwrap(); // set_id
    temp_file.write_all(b"PAR 2.0\0RecvSlic").unwrap(); // type
    temp_file.write_all(&42u32.to_le_bytes()).unwrap(); // exponent

    // Write recovery data (100 bytes with pattern)
    let recovery_data: Vec<u8> = (0..100).map(|i| (i % 256) as u8).collect();
    temp_file.write_all(&recovery_data).unwrap();
    temp_file.flush().unwrap();

    // Create metadata pointing to this data
    let metadata = RecoverySliceMetadata::from_file(
        42,                             // exponent
        RecoverySetId::new([1u8; 16]),  // set_id
        temp_file.path().to_path_buf(), // file_path
        68,                             // data_offset (After header)
        100,                            // data_size
    );

    // Test loading a chunk from the middle
    let chunk = metadata.load_chunk(10, 20).unwrap();
    assert_eq!(chunk.len(), 20);
    assert_eq!(chunk, &recovery_data[10..30]);

    // Test loading chunk at the beginning
    let chunk = metadata.load_chunk(0, 10).unwrap();
    assert_eq!(chunk.len(), 10);
    assert_eq!(chunk, &recovery_data[0..10]);

    // Test loading chunk at the end (partial)
    let chunk = metadata.load_chunk(90, 20).unwrap();
    assert_eq!(chunk.len(), 10); // Only 10 bytes left
    assert_eq!(chunk, &recovery_data[90..100]);

    // Test loading beyond end
    let chunk = metadata.load_chunk(100, 10).unwrap();
    assert_eq!(chunk.len(), 0);

    // Test loading chunk larger than remaining data
    let chunk = metadata.load_chunk(95, 100).unwrap();
    assert_eq!(chunk.len(), 5);
    assert_eq!(chunk, &recovery_data[95..100]);
}
