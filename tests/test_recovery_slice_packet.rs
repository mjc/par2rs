//! Comprehensive tests for packets/recovery_slice_packet.rs module
//!
//! Tests cover both RecoverySliceMetadata (lazy-loading) and RecoverySlicePacket (full packet).

use binrw::BinReaderExt;
use par2rs::domain::RecoverySetId;
use par2rs::packets::recovery_slice_packet::{RecoverySliceMetadata, RecoverySlicePacket};
use par2rs::repair::{FileSystemLoader, RecoveryDataLoader};
use std::io::Cursor;
use std::sync::Arc;
use tempfile::tempdir;

// Custom in-memory loader for testing
struct MemoryLoader {
    data: Vec<u8>,
}

impl RecoveryDataLoader for MemoryLoader {
    fn load_data(&self) -> std::io::Result<Vec<u8>> {
        Ok(self.data.clone())
    }

    fn load_chunk(&self, offset: usize, size: usize) -> std::io::Result<Vec<u8>> {
        let end = std::cmp::min(offset + size, self.data.len());
        if offset >= self.data.len() {
            return Ok(Vec::new());
        }
        Ok(self.data[offset..end].to_vec())
    }

    fn data_size(&self) -> usize {
        self.data.len()
    }
}

#[test]
fn test_recovery_slice_metadata_new() {
    let data = vec![1, 2, 3, 4, 5];
    let loader = Arc::new(MemoryLoader { data: data.clone() });
    let set_id = RecoverySetId::new([0u8; 16]);

    let metadata = RecoverySliceMetadata::new(42, set_id, loader);

    assert_eq!(metadata.exponent, 42);
    assert_eq!(metadata.set_id, set_id);
    assert_eq!(metadata.data_size(), 5);
}

#[test]
fn test_recovery_slice_metadata_load_data() {
    let data = vec![10, 20, 30, 40, 50];
    let loader = Arc::new(MemoryLoader { data: data.clone() });
    let set_id = RecoverySetId::new([0u8; 16]);

    let metadata = RecoverySliceMetadata::new(0, set_id, loader);

    let loaded = metadata.load_data().unwrap();
    assert_eq!(loaded, data);
}

#[test]
fn test_recovery_slice_metadata_load_chunk() {
    let data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let loader = Arc::new(MemoryLoader { data: data.clone() });
    let set_id = RecoverySetId::new([0u8; 16]);

    let metadata = RecoverySliceMetadata::new(0, set_id, loader);

    // Load chunk from middle
    let chunk = metadata.load_chunk(2, 4).unwrap();
    assert_eq!(chunk, vec![3, 4, 5, 6]);

    // Load chunk from start
    let chunk = metadata.load_chunk(0, 3).unwrap();
    assert_eq!(chunk, vec![1, 2, 3]);

    // Load chunk extending past end
    let chunk = metadata.load_chunk(7, 10).unwrap();
    assert_eq!(chunk, vec![8, 9, 10]);

    // Load chunk completely past end
    let chunk = metadata.load_chunk(20, 5).unwrap();
    assert_eq!(chunk, Vec::<u8>::new());
}

#[test]
fn test_recovery_slice_metadata_from_file() {
    let temp_dir = tempdir().unwrap();
    let test_file = temp_dir.path().join("test.par2");
    let set_id = RecoverySetId::new([1u8; 16]);

    // Create test file with some data
    std::fs::write(&test_file, b"test recovery data").unwrap();

    let metadata = RecoverySliceMetadata::from_file(123, set_id, test_file.clone(), 5, 8);

    assert_eq!(metadata.exponent, 123);
    assert_eq!(metadata.set_id, set_id);
    assert_eq!(metadata.data_size(), 8);

    // Verify data loading (should read "recovery" from offset 5, length 8)
    let loaded = metadata.load_data().unwrap();
    assert_eq!(loaded, b"recovery");
}

#[test]
fn test_recovery_slice_metadata_debug_format() {
    let data = vec![1, 2, 3];
    let loader = Arc::new(MemoryLoader { data });
    let set_id = RecoverySetId::new([0u8; 16]);

    let metadata = RecoverySliceMetadata::new(5, set_id, loader);

    let debug_str = format!("{:?}", metadata);
    assert!(debug_str.contains("RecoverySliceMetadata"));
    assert!(debug_str.contains("exponent: 5"));
    assert!(debug_str.contains("data_size: 3"));
}

#[test]
fn test_recovery_slice_metadata_clone() {
    let data = vec![1, 2, 3];
    let loader = Arc::new(MemoryLoader { data });
    let set_id = RecoverySetId::new([0u8; 16]);

    let metadata = RecoverySliceMetadata::new(10, set_id, loader);
    let cloned = metadata.clone();

    assert_eq!(cloned.exponent, metadata.exponent);
    assert_eq!(cloned.set_id, metadata.set_id);
    assert_eq!(cloned.data_size(), metadata.data_size());
}

/// Helper to create a minimal valid recovery slice packet bytes
fn create_test_packet_bytes(exponent: u32, recovery_data: &[u8]) -> Vec<u8> {
    let mut buffer = Vec::new();

    // Magic: "PAR2\0PKT"
    buffer.extend_from_slice(b"PAR2\0PKT");

    // Calculate length: 8 (magic) + 8 (length) + 16 (md5) + 16 (set_id) + 16 (type) + 4 (exponent) + data_len
    let total_length = 68 + recovery_data.len() as u64;
    buffer.extend_from_slice(&total_length.to_le_bytes());

    // Compute MD5 over: set_id + type + exponent + recovery_data
    let mut md5_data = Vec::new();
    let set_id_bytes = [0u8; 16];
    md5_data.extend_from_slice(&set_id_bytes);
    md5_data.extend_from_slice(b"PAR 2.0\0RecvSlic");
    md5_data.extend_from_slice(&exponent.to_le_bytes());
    md5_data.extend_from_slice(recovery_data);
    let md5_hash = crate::checksum::compute_md5_bytes(&md5_data);
    buffer.extend_from_slice(&md5_hash);

    // Set ID
    buffer.extend_from_slice(&set_id_bytes);

    // Type: "PAR 2.0\0RecvSlic"
    buffer.extend_from_slice(b"PAR 2.0\0RecvSlic");

    // Exponent
    buffer.extend_from_slice(&exponent.to_le_bytes());

    // Recovery data
    buffer.extend_from_slice(recovery_data);

    buffer
}

use par2rs::checksum;

#[test]
fn test_recovery_slice_packet_parse() {
    let recovery_data = vec![1, 2, 3, 4, 5];
    let packet_bytes = create_test_packet_bytes(10, &recovery_data);

    let mut cursor = Cursor::new(packet_bytes);
    let packet: RecoverySlicePacket = cursor.read_le().unwrap();

    assert_eq!(packet.exponent, 10);
    assert_eq!(packet.recovery_data, recovery_data);
    assert_eq!(packet.set_id, RecoverySetId::new([0u8; 16]));
    assert_eq!(packet.type_of_packet, *b"PAR 2.0\0RecvSlic");
}

#[test]
fn test_recovery_slice_packet_verify_success() {
    let recovery_data = vec![10, 20, 30];
    let packet_bytes = create_test_packet_bytes(5, &recovery_data);

    let mut cursor = Cursor::new(packet_bytes);
    let packet: RecoverySlicePacket = cursor.read_le().unwrap();

    assert!(packet.verify(), "Valid packet should verify successfully");
}

#[test]
fn test_recovery_slice_packet_verify_corrupted_md5() {
    let recovery_data = vec![1, 2, 3];
    let mut packet_bytes = create_test_packet_bytes(0, &recovery_data);

    // Corrupt the MD5 hash (bytes 16-31)
    packet_bytes[16] ^= 0xFF;

    let mut cursor = Cursor::new(packet_bytes);
    let packet: RecoverySlicePacket = cursor.read_le().unwrap();

    assert!(!packet.verify(), "Corrupted MD5 should fail verification");
}

#[test]
fn test_recovery_slice_packet_verify_invalid_length() {
    // Create packet with length < 64 (invalid)
    let mut buffer = Vec::new();
    buffer.extend_from_slice(b"PAR2\0PKT");
    buffer.extend_from_slice(&50u64.to_le_bytes()); // Invalid length < 64
    buffer.extend_from_slice(&[0u8; 48]); // Padding to minimum size

    let mut cursor = Cursor::new(buffer);
    if let Ok(packet) = cursor.read_le::<RecoverySlicePacket>() {
        assert!(!packet.verify(), "Invalid length should fail verification");
    }
    // If parsing fails, that's also acceptable
}

#[test]
fn test_recovery_slice_packet_write() {
    use binrw::BinWriterExt;

    let recovery_data = vec![5, 6, 7, 8];
    let packet_bytes = create_test_packet_bytes(15, &recovery_data);

    let mut cursor = Cursor::new(packet_bytes.clone());
    let packet: RecoverySlicePacket = cursor.read_le().unwrap();

    // Write packet back
    let mut output = Cursor::new(Vec::new());
    output.write_le(&packet).unwrap();

    // Should match original bytes
    assert_eq!(output.get_ref(), &packet_bytes);
}

#[test]
fn test_recovery_slice_packet_large_data() {
    // Test with larger recovery data
    let recovery_data: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
    let packet_bytes = create_test_packet_bytes(100, &recovery_data);

    let mut cursor = Cursor::new(packet_bytes);
    let packet: RecoverySlicePacket = cursor.read_le().unwrap();

    assert_eq!(packet.exponent, 100);
    assert_eq!(packet.recovery_data.len(), 1024);
    assert!(packet.verify());
}

#[test]
fn test_recovery_slice_metadata_parse_from_reader() {
    let temp_dir = tempdir().unwrap();
    let test_file = temp_dir.path().join("test.par2");

    let recovery_data = vec![11, 22, 33, 44, 55];
    let packet_bytes = create_test_packet_bytes(25, &recovery_data);

    std::fs::write(&test_file, &packet_bytes).unwrap();

    let mut file = std::fs::File::open(&test_file).unwrap();
    let metadata = RecoverySliceMetadata::parse_from_reader(&mut file, test_file.clone()).unwrap();

    assert_eq!(metadata.exponent, 25);
    assert_eq!(metadata.set_id, RecoverySetId::new([0u8; 16]));
    assert_eq!(metadata.data_size(), 5);

    // Verify data can be loaded
    let loaded = metadata.load_data().unwrap();
    assert_eq!(loaded, recovery_data);
}

#[test]
fn test_recovery_slice_metadata_parse_invalid_magic() {
    let temp_dir = tempdir().unwrap();
    let test_file = temp_dir.path().join("invalid.par2");

    // Write invalid magic
    let mut buffer = Vec::new();
    buffer.extend_from_slice(b"INVALID!");
    buffer.extend_from_slice(&[0u8; 100]);

    std::fs::write(&test_file, &buffer).unwrap();

    let mut file = std::fs::File::open(&test_file).unwrap();
    let result = RecoverySliceMetadata::parse_from_reader(&mut file, test_file);

    assert!(result.is_err(), "Invalid magic should fail parsing");
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn test_recovery_slice_metadata_parse_wrong_type() {
    let temp_dir = tempdir().unwrap();
    let test_file = temp_dir.path().join("wrong_type.par2");

    let mut buffer = Vec::new();
    buffer.extend_from_slice(b"PAR2\0PKT");
    buffer.extend_from_slice(&100u64.to_le_bytes()); // Length
    buffer.extend_from_slice(&[0u8; 16]); // MD5
    buffer.extend_from_slice(&[0u8; 16]); // Set ID
    buffer.extend_from_slice(b"PAR 2.0\0FileDesc"); // Wrong type (not RecvSlic)

    std::fs::write(&test_file, &buffer).unwrap();

    let mut file = std::fs::File::open(&test_file).unwrap();
    let result = RecoverySliceMetadata::parse_from_reader(&mut file, test_file);

    assert!(result.is_err(), "Wrong packet type should fail parsing");
}

#[test]
fn test_recovery_slice_metadata_parse_invalid_length() {
    let temp_dir = tempdir().unwrap();
    let test_file = temp_dir.path().join("invalid_length.par2");

    let mut buffer = Vec::new();
    buffer.extend_from_slice(b"PAR2\0PKT");
    buffer.extend_from_slice(&50u64.to_le_bytes()); // Length < header size (invalid)
    buffer.extend_from_slice(&[0u8; 48]);

    std::fs::write(&test_file, &buffer).unwrap();

    let mut file = std::fs::File::open(&test_file).unwrap();
    let result = RecoverySliceMetadata::parse_from_reader(&mut file, test_file);

    assert!(result.is_err(), "Invalid packet length should fail parsing");
}

#[test]
fn test_recovery_slice_packet_empty_data() {
    // Test packet with no recovery data
    let recovery_data = vec![];
    let packet_bytes = create_test_packet_bytes(0, &recovery_data);

    let mut cursor = Cursor::new(packet_bytes);
    let packet: RecoverySlicePacket = cursor.read_le().unwrap();

    assert_eq!(packet.exponent, 0);
    assert_eq!(packet.recovery_data.len(), 0);
    assert!(packet.verify());
}

#[test]
fn test_filesystem_loader_data_size() {
    let temp_dir = tempdir().unwrap();
    let test_file = temp_dir.path().join("loader_test.dat");

    std::fs::write(&test_file, b"0123456789").unwrap();

    let loader = FileSystemLoader {
        file_path: test_file,
        data_offset: 2,
        data_size: 5,
    };

    assert_eq!(loader.data_size(), 5);
}

#[test]
fn test_filesystem_loader_load_data() {
    let temp_dir = tempdir().unwrap();
    let test_file = temp_dir.path().join("loader_test.dat");

    std::fs::write(&test_file, b"0123456789").unwrap();

    let loader = FileSystemLoader {
        file_path: test_file,
        data_offset: 2,
        data_size: 5,
    };

    let data = loader.load_data().unwrap();
    assert_eq!(data, b"23456");
}

#[test]
fn test_filesystem_loader_load_chunk() {
    let temp_dir = tempdir().unwrap();
    let test_file = temp_dir.path().join("loader_test.dat");

    std::fs::write(&test_file, b"0123456789").unwrap();

    let loader = FileSystemLoader {
        file_path: test_file,
        data_offset: 2, // Start at '2'
        data_size: 5,   // Read "23456"
    };

    // Read middle chunk
    let chunk = loader.load_chunk(1, 3).unwrap();
    assert_eq!(chunk, b"345");

    // Read from start
    let chunk = loader.load_chunk(0, 2).unwrap();
    assert_eq!(chunk, b"23");

    // Read past end
    let chunk = loader.load_chunk(3, 10).unwrap();
    assert_eq!(chunk, b"56");
}

#[test]
fn test_recovery_slice_packet_debug_format() {
    let recovery_data = vec![1, 2, 3];
    let packet_bytes = create_test_packet_bytes(7, &recovery_data);

    let mut cursor = Cursor::new(packet_bytes);
    let packet: RecoverySlicePacket = cursor.read_le().unwrap();

    let debug_str = format!("{:?}", packet);
    assert!(debug_str.contains("RecoverySlicePacket"));
    assert!(debug_str.contains("exponent: 7"));
}

#[test]
fn test_recovery_slice_packet_clone() {
    let recovery_data = vec![10, 20];
    let packet_bytes = create_test_packet_bytes(3, &recovery_data);

    let mut cursor = Cursor::new(packet_bytes);
    let packet: RecoverySlicePacket = cursor.read_le().unwrap();
    let cloned = packet.clone();

    assert_eq!(cloned.exponent, packet.exponent);
    assert_eq!(cloned.recovery_data, packet.recovery_data);
    assert_eq!(cloned.set_id, packet.set_id);
}
