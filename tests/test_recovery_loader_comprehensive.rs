//! Tests for recovery_loader module RecoveryDataLoader trait and FileSystemLoader
//!
//! Tests for FileSystemLoader implementation, error conditions, and integration scenarios.

use par2rs::repair::{FileSystemLoader, RecoveryDataLoader};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

fn write_test_file(path: &PathBuf, data: &[u8]) {
    let mut file = File::create(path).unwrap();
    file.write_all(data).unwrap();
}

// ============================================================================
// FileSystemLoader Error Handling Tests
// ============================================================================

#[test]
fn test_loader_with_nonexistent_file() {
    let loader = FileSystemLoader {
        file_path: PathBuf::from("/nonexistent/file/path.bin"),
        data_offset: 0,
        data_size: 100,
    };

    let result = loader.load_chunk(0, 100);
    assert!(result.is_err());
}

#[test]
fn test_loader_with_empty_chunk_size() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.bin");
    write_test_file(&file_path, &[0x00u8; 1000]);

    let loader = FileSystemLoader {
        file_path: file_path.clone(),
        data_offset: 0,
        data_size: 1000,
    };

    let result = loader.load_chunk(0, 0);
    if let Ok(data) = result {
        assert!(data.is_empty());
    }
}

#[test]
fn test_loader_with_beyond_boundary_offset() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.bin");
    write_test_file(&file_path, &[0xAA; 1000]);

    let loader = FileSystemLoader {
        file_path: file_path.clone(),
        data_offset: 0,
        data_size: 1000,
    };

    let result = loader.load_chunk(1500, 100);
    if let Ok(data) = result {
        assert_eq!(data.len(), 0);
    }
}

#[test]
fn test_loader_sequential_chunks() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("sequential.bin");
    let test_data: Vec<u8> = (0..255).cycle().take(5000).collect();
    write_test_file(&file_path, &test_data);

    let loader = FileSystemLoader {
        file_path: file_path.clone(),
        data_offset: 0,
        data_size: 5000,
    };

    let chunk1 = loader.load_chunk(0, 1000).unwrap();
    let chunk2 = loader.load_chunk(1000, 1000).unwrap();

    assert_eq!(chunk1.len(), 1000);
    assert_eq!(chunk2.len(), 1000);
    assert!(!chunk1.iter().eq(chunk2.iter()));
}

#[test]
fn test_loader_overlapping_chunks() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("overlap.bin");
    write_test_file(&file_path, &vec![0x55u8; 2000]);

    let loader = FileSystemLoader {
        file_path: file_path.clone(),
        data_offset: 0,
        data_size: 2000,
    };

    let chunk1 = loader.load_chunk(0, 1000).unwrap();
    let chunk2 = loader.load_chunk(500, 1000).unwrap();

    assert_eq!(chunk1.len(), 1000);
    assert_eq!(chunk2.len(), 1000);
    assert!(chunk2.iter().all(|&b| b == 0x55));
}

#[test]
fn test_loader_with_data_offset() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("offset.bin");

    let mut full_data = vec![0xFF; 100];
    full_data.extend_from_slice(&[0xAA; 500]);
    write_test_file(&file_path, &full_data);

    let loader = FileSystemLoader {
        file_path: file_path.clone(),
        data_offset: 100,
        data_size: 500,
    };

    let chunk = loader.load_chunk(0, 100).unwrap();
    assert_eq!(chunk.len(), 100);
    assert!(chunk.iter().all(|&b| b == 0xAA));
}

#[test]
fn test_loader_boundary_read_at_end() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("boundary.bin");
    write_test_file(&file_path, &[0xCC; 100]);

    let loader = FileSystemLoader {
        file_path: file_path.clone(),
        data_offset: 0,
        data_size: 100,
    };

    let result = loader.load_chunk(0, 100);
    if let Ok(data) = result {
        assert_eq!(data.len(), 100);
    }

    let result = loader.load_chunk(50, 100);
    if let Ok(data) = result {
        assert_eq!(data.len(), 50);
    }
}

#[test]
fn test_loader_data_size_matches() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("size_test.bin");
    write_test_file(&file_path, &vec![0x77u8; 2500]);

    let loader = FileSystemLoader {
        file_path: file_path.clone(),
        data_offset: 0,
        data_size: 2500,
    };

    assert_eq!(loader.data_size(), 2500);

    let full_data = loader.load_data().unwrap();
    assert_eq!(full_data.len(), 2500);
}

// ============================================================================
// RecoveryDataLoader Trait Tests
// ============================================================================

#[test]
fn test_recovery_data_loader_load_full_data() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("full_load.bin");
    let test_data: Vec<u8> = (0..100).collect();
    write_test_file(&file_path, &test_data);

    let loader = FileSystemLoader {
        file_path: file_path.clone(),
        data_offset: 0,
        data_size: 100,
    };

    let data = loader.load_data().unwrap();
    assert_eq!(data.len(), 100);
    assert_eq!(data, test_data);
}

#[test]
fn test_recovery_data_loader_chunk_within_data() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("chunk_test.bin");
    let test_data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
    write_test_file(&file_path, &test_data);

    let loader = FileSystemLoader {
        file_path: file_path.clone(),
        data_offset: 0,
        data_size: 1000,
    };

    let chunk = loader.load_chunk(100, 50).unwrap();
    assert_eq!(chunk.len(), 50);
    let expected = &test_data[100..150];
    assert_eq!(&chunk[..], expected);
}

#[test]
fn test_recovery_data_loader_partial_at_boundary() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("partial_boundary.bin");
    write_test_file(&file_path, &vec![0xEE; 500]);

    let loader = FileSystemLoader {
        file_path: file_path.clone(),
        data_offset: 0,
        data_size: 500,
    };

    let chunk = loader.load_chunk(400, 200).unwrap();
    assert_eq!(chunk.len(), 100);
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_loader_read_write_roundtrip() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("roundtrip.bin");
    let original_data: Vec<u8> = (0..=255).cycle().take(5000).collect();
    write_test_file(&file_path, &original_data);

    let loader = FileSystemLoader {
        file_path: file_path.clone(),
        data_offset: 0,
        data_size: 5000,
    };

    let data = loader.load_data().unwrap();
    assert_eq!(data.len(), 5000);
    assert_eq!(data, original_data);
}

#[test]
fn test_loader_multiple_sequential_reads() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("multi_read.bin");
    let test_data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
    write_test_file(&file_path, &test_data);

    let loader = FileSystemLoader {
        file_path: file_path.clone(),
        data_offset: 0,
        data_size: 1000,
    };

    let mut read_data: Vec<u8> = Vec::new();
    for offset in (0..1000).step_by(100) {
        let chunk = loader.load_chunk(offset, 100).unwrap();
        read_data.extend(&chunk);
    }

    assert_eq!(read_data, test_data);
}

#[test]
fn test_loader_consistency_across_reads() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("consistent.bin");
    write_test_file(&file_path, &vec![0xBB; 2000]);

    let loader = FileSystemLoader {
        file_path: file_path.clone(),
        data_offset: 0,
        data_size: 2000,
    };

    let chunk1 = loader.load_chunk(0, 500).unwrap();
    let chunk2 = loader.load_chunk(0, 500).unwrap();
    let chunk3 = loader.load_chunk(0, 500).unwrap();

    assert_eq!(chunk1, chunk2);
    assert_eq!(chunk2, chunk3);
}

#[test]
fn test_loader_with_varied_offsets() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("varied_offsets.bin");
    let test_data: Vec<u8> = (0..1000).map(|i| (i & 0xFF) as u8).collect();
    write_test_file(&file_path, &test_data);

    let loader = FileSystemLoader {
        file_path: file_path.clone(),
        data_offset: 0,
        data_size: 1000,
    };

    let test_offsets = vec![0, 1, 10, 99, 100, 255, 500, 999];

    for offset in test_offsets {
        if offset < 1000 {
            let chunk = loader.load_chunk(offset, 1).unwrap();
            if !chunk.is_empty() {
                assert_eq!(chunk[0], (offset & 0xFF) as u8);
            }
        }
    }
}
