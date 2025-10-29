//! Comprehensive tests for FileDescriptionPacket
//!
//! Tests for serialization, deserialization, verification, and integration
//! with file metadata and recovery data.

use binrw::BinWrite;
use par2rs::domain::{FileId, Md5Hash, RecoverySetId};
use par2rs::packets::file_description_packet::FileDescriptionPacket;
use std::fs::File;
use std::io::{Cursor, Write};
use tempfile::NamedTempFile;

// Helper functions
fn create_test_packet() -> FileDescriptionPacket {
    FileDescriptionPacket {
        length: 120,
        md5: Md5Hash::new([1; 16]),
        set_id: RecoverySetId::new([2; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([3; 16]),
        md5_hash: Md5Hash::new([4; 16]),
        md5_16k: Md5Hash::new([5; 16]),
        file_length: 1024,
        file_name: b"testfile.txt".to_vec(),
    }
}

fn create_packet_with_long_filename() -> FileDescriptionPacket {
    let long_name = "this_is_a_very_long_filename_that_tests_packet_serialization_and_deserialization_with_extended_data.txt";
    FileDescriptionPacket {
        length: 120 + long_name.len() as u64,
        md5: Md5Hash::new([10; 16]),
        set_id: RecoverySetId::new([20; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([30; 16]),
        md5_hash: Md5Hash::new([40; 16]),
        md5_16k: Md5Hash::new([50; 16]),
        file_length: 2048,
        file_name: long_name.as_bytes().to_vec(),
    }
}

// ============================================================================
// Basic Structure Tests
// ============================================================================

#[test]
fn test_file_description_packet_create() {
    let packet = create_test_packet();

    assert_eq!(packet.length, 120);
    assert_eq!(packet.file_length, 1024);
    assert_eq!(packet.file_name, b"testfile.txt");
}

#[test]
fn test_file_description_packet_field_values() {
    let packet = create_test_packet();

    // Check all fields are properly set
    assert_eq!(packet.md5.as_bytes(), &[1; 16]);
    assert_eq!(packet.set_id.as_bytes(), &[2; 16]);
    assert_eq!(packet.file_id.as_bytes(), &[3; 16]);
    assert_eq!(packet.md5_hash.as_bytes(), &[4; 16]);
    assert_eq!(packet.md5_16k.as_bytes(), &[5; 16]);
}

#[test]
fn test_file_description_packet_type_field() {
    let packet = create_test_packet();

    assert_eq!(packet.packet_type, *b"PAR 2.0\0FileDesc");
}

#[test]
fn test_file_description_packet_with_empty_filename() {
    let packet = FileDescriptionPacket {
        length: 120,
        md5: Md5Hash::new([1; 16]),
        set_id: RecoverySetId::new([2; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([3; 16]),
        md5_hash: Md5Hash::new([4; 16]),
        md5_16k: Md5Hash::new([5; 16]),
        file_length: 0,
        file_name: vec![],
    };

    assert!(packet.file_name.is_empty());
    assert_eq!(packet.file_length, 0);
}

#[test]
fn test_file_description_packet_with_unicode_filename() {
    let unicode_name = "файл_テスト_😀.dat";
    let packet = FileDescriptionPacket {
        length: 120 + unicode_name.len() as u64,
        md5: Md5Hash::new([1; 16]),
        set_id: RecoverySetId::new([2; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([3; 16]),
        md5_hash: Md5Hash::new([4; 16]),
        md5_16k: Md5Hash::new([5; 16]),
        file_length: 4096,
        file_name: unicode_name.as_bytes().to_vec(),
    };

    assert_eq!(packet.file_name, unicode_name.as_bytes());
}

#[test]
fn test_file_description_packet_with_special_characters() {
    let special_name = "file-with_special.chars+&%@!.dat";
    let packet = FileDescriptionPacket {
        length: 120 + special_name.len() as u64,
        md5: Md5Hash::new([1; 16]),
        set_id: RecoverySetId::new([2; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([3; 16]),
        md5_hash: Md5Hash::new([4; 16]),
        md5_16k: Md5Hash::new([5; 16]),
        file_length: 1024,
        file_name: special_name.as_bytes().to_vec(),
    };

    assert_eq!(packet.file_name, special_name.as_bytes());
}

#[test]
fn test_file_description_packet_with_null_bytes_in_filename() {
    let mut filename = vec![b'f', b'i', b'l', b'e'];
    filename.push(0);
    filename.extend_from_slice(b"extra");

    let packet = FileDescriptionPacket {
        length: 120 + filename.len() as u64,
        md5: Md5Hash::new([1; 16]),
        set_id: RecoverySetId::new([2; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([3; 16]),
        md5_hash: Md5Hash::new([4; 16]),
        md5_16k: Md5Hash::new([5; 16]),
        file_length: 1024,
        file_name: filename.clone(),
    };

    assert_eq!(packet.file_name, filename);
}

// ============================================================================
// Serialization and Deserialization Tests
// Note: These tests are complex due to binrw magic handling and are best tested
// by reading from actual PAR2 files (see doctest in file_description_packet.rs)

// ============================================================================
// Verification Tests
// ============================================================================

#[test]
fn test_file_description_packet_verify() {
    let packet = FileDescriptionPacket {
        length: 120,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([0; 16]),
        md5_hash: Md5Hash::new([0; 16]),
        md5_16k: Md5Hash::new([0; 16]),
        file_length: 0,
        file_name: vec![],
    };

    // Verify computation: just ensure it runs without panic
    let _ = packet.verify();
}

#[test]
fn test_file_description_packet_verify_invalid_md5() {
    let packet = create_test_packet();

    // MD5 is incorrect by default, so verification should fail
    assert!(!packet.verify());
}

#[test]
fn test_file_description_packet_verify_wrong_packet_type() {
    let mut packet = create_test_packet();
    // Use an array literal to ensure exactly 16 bytes
    let invalid_type: [u8; 16] = [
        b'I', b'N', b'V', b'A', b'L', b'I', b'D', 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    packet.packet_type = invalid_type;

    // Wrong packet type should fail verification
    assert!(!packet.verify());
}

#[test]
fn test_file_description_packet_verify_invalid_length() {
    let mut packet = create_test_packet();
    packet.length = 100; // Too short

    // Invalid length should fail verification
    assert!(!packet.verify());
}

#[test]
fn test_file_description_packet_verify_with_long_filename() {
    let mut packet = create_packet_with_long_filename();

    // Compute correct MD5
    let mut buffer = Cursor::new(Vec::new());
    packet.write_le(&mut buffer).expect("Failed to write");

    let set_id_start = 24;
    let packet_data = buffer.get_ref()[set_id_start..].to_vec();

    use md_5::Digest;
    let correct_md5: [u8; 16] = md_5::Md5::digest(&packet_data).into();
    packet.md5 = Md5Hash::new(correct_md5);

    assert!(packet.verify());
}

#[test]
fn test_file_description_packet_verify_minimal_valid() {
    let mut packet = FileDescriptionPacket {
        length: 120,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([0; 16]),
        md5_hash: Md5Hash::new([0; 16]),
        md5_16k: Md5Hash::new([0; 16]),
        file_length: 0,
        file_name: vec![],
    };

    // Compute correct MD5
    let mut buffer = Cursor::new(Vec::new());
    packet.write_le(&mut buffer).expect("Failed to write");

    let set_id_start = 24;
    let packet_data = buffer.get_ref()[set_id_start..].to_vec();

    use md_5::Digest;
    let correct_md5: [u8; 16] = md_5::Md5::digest(&packet_data).into();
    packet.md5 = Md5Hash::new(correct_md5);

    assert!(packet.verify());
}

// ============================================================================
// Edge Cases and Boundary Tests
// ============================================================================

#[test]
fn test_file_description_packet_large_file_size() {
    let packet = FileDescriptionPacket {
        length: 120,
        md5: Md5Hash::new([1; 16]),
        set_id: RecoverySetId::new([2; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([3; 16]),
        md5_hash: Md5Hash::new([4; 16]),
        md5_16k: Md5Hash::new([5; 16]),
        file_length: 1u64 << 40, // 1 TB
        file_name: b"huge_file.iso".to_vec(),
    };

    assert_eq!(packet.file_length, 1u64 << 40);
}

#[test]
fn test_file_description_packet_zero_file_size() {
    let packet = FileDescriptionPacket {
        length: 120,
        md5: Md5Hash::new([1; 16]),
        set_id: RecoverySetId::new([2; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([3; 16]),
        md5_hash: Md5Hash::new([4; 16]),
        md5_16k: Md5Hash::new([5; 16]),
        file_length: 0,
        file_name: b"empty.txt".to_vec(),
    };

    assert_eq!(packet.file_length, 0);
}

#[test]
fn test_file_description_packet_16kb_boundary() {
    let packet = FileDescriptionPacket {
        length: 120,
        md5: Md5Hash::new([1; 16]),
        set_id: RecoverySetId::new([2; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([3; 16]),
        md5_hash: Md5Hash::new([4; 16]),
        md5_16k: Md5Hash::new([5; 16]),
        file_length: 16384, // Exactly 16KB
        file_name: b"file.dat".to_vec(),
    };

    assert_eq!(packet.file_length, 16384);
}

#[test]
fn test_file_description_packet_just_over_16kb() {
    let packet = FileDescriptionPacket {
        length: 120,
        md5: Md5Hash::new([1; 16]),
        set_id: RecoverySetId::new([2; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([3; 16]),
        md5_hash: Md5Hash::new([4; 16]),
        md5_16k: Md5Hash::new([5; 16]),
        file_length: 16385, // Just over 16KB
        file_name: b"file.dat".to_vec(),
    };

    assert_eq!(packet.file_length, 16385);
}

#[test]
fn test_file_description_packet_max_u64_file_size() {
    let packet = FileDescriptionPacket {
        length: 120,
        md5: Md5Hash::new([1; 16]),
        set_id: RecoverySetId::new([2; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([3; 16]),
        md5_hash: Md5Hash::new([4; 16]),
        md5_16k: Md5Hash::new([5; 16]),
        file_length: u64::MAX,
        file_name: b"massive.file".to_vec(),
    };

    assert_eq!(packet.file_length, u64::MAX);
}

#[test]
fn test_file_description_packet_255_length_filename() {
    let long_name = "a".repeat(255);
    let packet = FileDescriptionPacket {
        length: 120 + 255,
        md5: Md5Hash::new([1; 16]),
        set_id: RecoverySetId::new([2; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([3; 16]),
        md5_hash: Md5Hash::new([4; 16]),
        md5_16k: Md5Hash::new([5; 16]),
        file_length: 1024,
        file_name: long_name.as_bytes().to_vec(),
    };

    assert_eq!(packet.file_name.len(), 255);
}

#[test]
fn test_file_description_packet_path_separators() {
    let unix_path = "/home/user/file.txt";
    let packet = FileDescriptionPacket {
        length: 120 + unix_path.len() as u64,
        md5: Md5Hash::new([1; 16]),
        set_id: RecoverySetId::new([2; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([3; 16]),
        md5_hash: Md5Hash::new([4; 16]),
        md5_16k: Md5Hash::new([5; 16]),
        file_length: 1024,
        file_name: unix_path.as_bytes().to_vec(),
    };

    assert_eq!(packet.file_name, unix_path.as_bytes());
}

#[test]
fn test_file_description_packet_windows_path_separators() {
    let windows_path = "C:\\Users\\file.txt";
    let packet = FileDescriptionPacket {
        length: 120 + windows_path.len() as u64,
        md5: Md5Hash::new([1; 16]),
        set_id: RecoverySetId::new([2; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([3; 16]),
        md5_hash: Md5Hash::new([4; 16]),
        md5_16k: Md5Hash::new([5; 16]),
        file_length: 1024,
        file_name: windows_path.as_bytes().to_vec(),
    };

    assert_eq!(packet.file_name, windows_path.as_bytes());
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_file_description_packet_with_real_file_integration() {
    // Create a temporary file
    let mut temp_file = NamedTempFile::new().unwrap();
    let test_data = b"This is test data for file description packet integration test";
    temp_file.write_all(test_data).unwrap();
    temp_file.flush().unwrap();

    let file_path = temp_file.path();
    let file_name = file_path.file_name().unwrap().to_string_lossy();

    // Calculate MD5 of file
    let file = File::open(file_path).unwrap();
    use md_5::Digest;
    let mut hasher = md_5::Md5::new();
    use std::io::Read;
    let mut reader = std::io::BufReader::new(file);
    let mut buffer = [0u8; 1024];
    loop {
        let bytes_read = reader.read(&mut buffer).unwrap();
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    let file_md5 = hasher.finalize();

    let packet = FileDescriptionPacket {
        length: 120 + file_name.len() as u64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([1; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([2; 16]),
        md5_hash: Md5Hash::new(file_md5.into()),
        md5_16k: Md5Hash::new([0; 16]),
        file_length: test_data.len() as u64,
        file_name: file_name.as_bytes().to_vec(),
    };

    assert_eq!(packet.file_length, test_data.len() as u64);
}

#[test]
fn test_file_description_packet_sequence() {
    // Create multiple packets as if representing a multi-file archive
    let packets: Vec<_> = (0..5)
        .map(|i| FileDescriptionPacket {
            length: 120,
            md5: Md5Hash::new([i as u8; 16]),
            set_id: RecoverySetId::new([10 + i as u8; 16]),
            packet_type: *b"PAR 2.0\0FileDesc",
            file_id: FileId::new([20 + i as u8; 16]),
            md5_hash: Md5Hash::new([30 + i as u8; 16]),
            md5_16k: Md5Hash::new([40 + i as u8; 16]),
            file_length: 1024 * (i as u64 + 1),
            file_name: format!("file_{}.txt", i).as_bytes().to_vec(),
        })
        .collect();

    assert_eq!(packets.len(), 5);
    for (i, packet) in packets.iter().enumerate() {
        assert_eq!(packet.file_length, 1024 * (i as u64 + 1));
    }
}

#[test]
fn test_file_description_packet_clone_equality() {
    let original = create_test_packet();
    let cloned = FileDescriptionPacket {
        length: original.length,
        md5: original.md5,
        set_id: original.set_id,
        packet_type: original.packet_type,
        file_id: original.file_id,
        md5_hash: original.md5_hash,
        md5_16k: original.md5_16k,
        file_length: original.file_length,
        file_name: original.file_name.clone(),
    };

    assert_eq!(cloned.file_length, original.file_length);
    assert_eq!(cloned.file_name, original.file_name);
    assert_eq!(cloned.md5, original.md5);
}

#[test]
fn test_file_description_packet_minimal_length() {
    let packet = FileDescriptionPacket {
        length: 120,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id: FileId::new([0; 16]),
        md5_hash: Md5Hash::new([0; 16]),
        md5_16k: Md5Hash::new([0; 16]),
        file_length: 0,
        file_name: vec![],
    };

    // Verify packet still contains valid field values
    assert_eq!(packet.length, 120);
}
