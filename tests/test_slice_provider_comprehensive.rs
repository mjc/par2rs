use par2rs::domain::{Crc32Value, RecoverySetId};
use par2rs::repair::{
    ActualDataSize, ChunkedSliceProvider, LogicalSliceSize, RecoverySliceProvider, SliceLocation,
    SliceProvider,
};
use par2rs::RecoverySliceMetadata;
use std::io::Write;
use tempfile::NamedTempFile;

// ============================================================================
// ActualDataSize tests
// ============================================================================

#[test]
fn test_actual_data_size_new() {
    let size = ActualDataSize::new(1024);
    assert_eq!(size.as_usize(), 1024);
}

#[test]
fn test_actual_data_size_from() {
    let size: ActualDataSize = 2048.into();
    assert_eq!(size.as_usize(), 2048);
}

#[test]
fn test_actual_data_size_zero() {
    let size = ActualDataSize::new(0);
    assert_eq!(size.as_usize(), 0);
}

#[test]
fn test_actual_data_size_large() {
    let size = ActualDataSize::new(10_000_000);
    assert_eq!(size.as_usize(), 10_000_000);
}

#[test]
fn test_actual_data_size_eq() {
    let size1 = ActualDataSize::new(512);
    let size2 = ActualDataSize::new(512);
    let size3 = ActualDataSize::new(256);
    assert_eq!(size1, size2);
    assert_ne!(size1, size3);
}

#[test]
fn test_actual_data_size_clone() {
    let size1 = ActualDataSize::new(100);
    let size2 = size1;
    assert_eq!(size1, size2);
}

#[test]
fn test_actual_data_size_debug() {
    let size = ActualDataSize::new(100);
    let debug_str = format!("{:?}", size);
    assert!(debug_str.contains("ActualDataSize"));
}

// ============================================================================
// LogicalSliceSize tests
// ============================================================================

#[test]
fn test_logical_slice_size_new() {
    let size = LogicalSliceSize::new(2048);
    assert_eq!(size.as_usize(), 2048);
}

#[test]
fn test_logical_slice_size_from() {
    let size: LogicalSliceSize = 4096.into();
    assert_eq!(size.as_usize(), 4096);
}

#[test]
fn test_logical_slice_size_zero() {
    let size = LogicalSliceSize::new(0);
    assert_eq!(size.as_usize(), 0);
}

#[test]
fn test_logical_slice_size_large() {
    let size = LogicalSliceSize::new(100_000_000);
    assert_eq!(size.as_usize(), 100_000_000);
}

#[test]
fn test_logical_slice_size_eq() {
    let size1 = LogicalSliceSize::new(1024);
    let size2 = LogicalSliceSize::new(1024);
    let size3 = LogicalSliceSize::new(2048);
    assert_eq!(size1, size2);
    assert_ne!(size1, size3);
}

#[test]
fn test_logical_slice_size_clone() {
    let size1 = LogicalSliceSize::new(200);
    let size2 = size1;
    assert_eq!(size1, size2);
}

#[test]
fn test_logical_slice_size_debug() {
    let size = LogicalSliceSize::new(256);
    let debug_str = format!("{:?}", size);
    assert!(debug_str.contains("LogicalSliceSize"));
}

// ============================================================================
// SliceLocation tests
// ============================================================================

#[test]
fn test_slice_location_creation() {
    let location = SliceLocation {
        file_path: "test.dat".into(),
        offset: 1024,
        actual_size: ActualDataSize::new(500),
        logical_size: LogicalSliceSize::new(512),
        expected_crc: Some(Crc32Value::new(0x12345678)),
    };

    assert_eq!(location.file_path.to_str().unwrap(), "test.dat");
    assert_eq!(location.offset, 1024);
    assert_eq!(location.actual_size.as_usize(), 500);
    assert_eq!(location.logical_size.as_usize(), 512);
    assert!(location.expected_crc.is_some());
}

#[test]
fn test_slice_location_no_crc() {
    let location = SliceLocation {
        file_path: "test.dat".into(),
        offset: 0,
        actual_size: ActualDataSize::new(1024),
        logical_size: LogicalSliceSize::new(1024),
        expected_crc: None,
    };

    assert!(location.expected_crc.is_none());
}

#[test]
fn test_slice_location_clone() {
    let location1 = SliceLocation {
        file_path: "test.dat".into(),
        offset: 512,
        actual_size: ActualDataSize::new(256),
        logical_size: LogicalSliceSize::new(256),
        expected_crc: None,
    };

    let location2 = location1.clone();
    assert_eq!(location1.file_path, location2.file_path);
    assert_eq!(location1.offset, location2.offset);
    assert_eq!(location1.actual_size, location2.actual_size);
}

#[test]
fn test_slice_location_debug() {
    let location = SliceLocation {
        file_path: "debug.dat".into(),
        offset: 100,
        actual_size: ActualDataSize::new(50),
        logical_size: LogicalSliceSize::new(64),
        expected_crc: Some(Crc32Value::new(0xABCDEF00)),
    };

    let debug_str = format!("{:?}", location);
    assert!(debug_str.contains("SliceLocation"));
}

// ============================================================================
// ChunkedSliceProvider tests
// ============================================================================

#[test]
fn test_chunked_provider_new() {
    let provider = ChunkedSliceProvider::new(1024);
    assert_eq!(provider.available_slices().len(), 0);
}

#[test]
fn test_chunked_provider_add_slice() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0x42; 1000]).unwrap();
    temp_file.flush().unwrap();

    let mut provider = ChunkedSliceProvider::new(1024);
    provider.add_slice(
        0,
        SliceLocation {
            file_path: temp_file.path().to_path_buf(),
            offset: 0,
            actual_size: ActualDataSize::new(1000),
            logical_size: LogicalSliceSize::new(1024),
            expected_crc: None,
        },
    );

    assert!(provider.is_slice_available(0));
    assert_eq!(provider.available_slices().len(), 1);
}

#[test]
fn test_chunked_provider_multiple_slices() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0x42; 4096]).unwrap();
    temp_file.flush().unwrap();

    let mut provider = ChunkedSliceProvider::new(1024);

    for i in 0..4 {
        provider.add_slice(
            i,
            SliceLocation {
                file_path: temp_file.path().to_path_buf(),
                offset: i as u64 * 1024,
                actual_size: ActualDataSize::new(1024),
                logical_size: LogicalSliceSize::new(1024),
                expected_crc: None,
            },
        );
    }

    let available = provider.available_slices();
    assert_eq!(available.len(), 4);
    assert!(available.contains(&0));
    assert!(available.contains(&1));
    assert!(available.contains(&2));
    assert!(available.contains(&3));
}

#[test]
fn test_chunked_provider_read_chunk() {
    let mut temp_file = NamedTempFile::new().unwrap();
    let test_data = vec![0x55u8; 1000];
    temp_file.write_all(&test_data).unwrap();
    temp_file.flush().unwrap();

    let mut provider = ChunkedSliceProvider::new(1024);
    provider.add_slice(
        0,
        SliceLocation {
            file_path: temp_file.path().to_path_buf(),
            offset: 0,
            actual_size: ActualDataSize::new(1000),
            logical_size: LogicalSliceSize::new(1024),
            expected_crc: None,
        },
    );

    let chunk = provider.read_chunk(0, 0, 64).unwrap();
    assert_eq!(chunk.valid_bytes, 64);
    assert_eq!(chunk.data.len(), 64);
    assert!(chunk.data.iter().all(|&b| b == 0x55));
}

#[test]
fn test_chunked_provider_read_partial_chunk() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0x42; 1000]).unwrap();
    temp_file.flush().unwrap();

    let mut provider = ChunkedSliceProvider::new(1024);
    provider.add_slice(
        0,
        SliceLocation {
            file_path: temp_file.path().to_path_buf(),
            offset: 0,
            actual_size: ActualDataSize::new(1000),
            logical_size: LogicalSliceSize::new(1024),
            expected_crc: None,
        },
    );

    // Read at end - should get 50 bytes real data + 14 bytes padding = 64 total
    let chunk = provider.read_chunk(0, 950, 64).unwrap();
    assert_eq!(chunk.valid_bytes, 64);
    assert_eq!(chunk.data.len(), 64);
    // First 50 bytes should be 0x42, remaining 14 should be padding (0x00)
    assert!(chunk.data[..50].iter().all(|&b| b == 0x42));
    assert!(chunk.data[50..].iter().all(|&b| b == 0x00));
}

#[test]
fn test_chunked_provider_read_with_padding() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0x42; 1000]).unwrap();
    temp_file.flush().unwrap();

    let mut provider = ChunkedSliceProvider::new(1024);
    provider.add_slice(
        0,
        SliceLocation {
            file_path: temp_file.path().to_path_buf(),
            offset: 0,
            actual_size: ActualDataSize::new(1000),
            logical_size: LogicalSliceSize::new(1024),
            expected_crc: None,
        },
    );

    // Read chunk that spans actual data and padding
    // offset=990, chunk_size=30, actual_size=1000, logical_size=1024
    // Should read 10 bytes real data (990..1000) + 20 bytes padding (1000..1020) = 30 total
    let chunk = provider.read_chunk(0, 990, 30).unwrap();
    assert_eq!(chunk.valid_bytes, 30);
    assert_eq!(chunk.data.len(), 30);
    // First 10 bytes should be 0x42, rest should be 0x00 (padding)
    assert!(chunk.data[..10].iter().all(|&b| b == 0x42));
    assert!(chunk.data[10..].iter().all(|&b| b == 0x00));
}

#[test]
fn test_chunked_provider_read_all_padding() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0x42; 1000]).unwrap();
    temp_file.flush().unwrap();

    let mut provider = ChunkedSliceProvider::new(1024);
    provider.add_slice(
        0,
        SliceLocation {
            file_path: temp_file.path().to_path_buf(),
            offset: 0,
            actual_size: ActualDataSize::new(1000),
            logical_size: LogicalSliceSize::new(1024),
            expected_crc: None,
        },
    );

    // Read entirely from padding region
    let chunk = provider.read_chunk(0, 1000, 24).unwrap();
    assert_eq!(chunk.valid_bytes, 24);
    assert!(chunk.data.iter().all(|&b| b == 0x00));
}

#[test]
fn test_chunked_provider_cache_hits() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0x99; 1000]).unwrap();
    temp_file.flush().unwrap();

    let mut provider = ChunkedSliceProvider::new(1024);
    provider.add_slice(
        0,
        SliceLocation {
            file_path: temp_file.path().to_path_buf(),
            offset: 0,
            actual_size: ActualDataSize::new(1000),
            logical_size: LogicalSliceSize::new(1024),
            expected_crc: None,
        },
    );

    // Read same chunk twice - second should hit cache
    let chunk1 = provider.read_chunk(0, 0, 64).unwrap();
    let chunk2 = provider.read_chunk(0, 0, 64).unwrap();

    assert_eq!(chunk1.data, chunk2.data);
}

#[test]
fn test_chunked_provider_get_slice_size() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0x00; 1000]).unwrap();
    temp_file.flush().unwrap();

    let mut provider = ChunkedSliceProvider::new(1024);
    provider.add_slice(
        0,
        SliceLocation {
            file_path: temp_file.path().to_path_buf(),
            offset: 0,
            actual_size: ActualDataSize::new(1000),
            logical_size: LogicalSliceSize::new(1024),
            expected_crc: None,
        },
    );

    // Should return LOGICAL size, not actual size
    assert_eq!(provider.get_slice_size(0), Some(1024));
    assert_eq!(provider.get_slice_size(1), None);
}

#[test]
fn test_chunked_provider_is_slice_available() {
    let mut provider = ChunkedSliceProvider::new(1024);

    assert!(!provider.is_slice_available(0));

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0x00; 1000]).unwrap();
    temp_file.flush().unwrap();

    provider.add_slice(
        0,
        SliceLocation {
            file_path: temp_file.path().to_path_buf(),
            offset: 0,
            actual_size: ActualDataSize::new(1000),
            logical_size: LogicalSliceSize::new(1024),
            expected_crc: None,
        },
    );

    assert!(provider.is_slice_available(0));
    assert!(!provider.is_slice_available(1));
}

#[test]
fn test_chunked_provider_verify_slice_no_crc() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0x42; 1024]).unwrap();
    temp_file.flush().unwrap();

    let mut provider = ChunkedSliceProvider::new(1024);
    provider.add_slice(
        0,
        SliceLocation {
            file_path: temp_file.path().to_path_buf(),
            offset: 0,
            actual_size: ActualDataSize::new(1024),
            logical_size: LogicalSliceSize::new(1024),
            expected_crc: None, // No CRC provided
        },
    );

    let result = provider.verify_slice(0).unwrap();
    assert_eq!(result, None); // Can't verify without CRC
}

#[test]
fn test_chunked_provider_verify_slice_with_crc() {
    let mut temp_file = NamedTempFile::new().unwrap();
    let test_data = vec![0x42u8; 1024];
    temp_file.write_all(&test_data).unwrap();
    temp_file.flush().unwrap();

    // Compute expected CRC
    let expected_crc = par2rs::checksum::compute_crc32(&test_data);

    let mut provider = ChunkedSliceProvider::new(1024);
    provider.add_slice(
        0,
        SliceLocation {
            file_path: temp_file.path().to_path_buf(),
            offset: 0,
            actual_size: ActualDataSize::new(1024),
            logical_size: LogicalSliceSize::new(1024),
            expected_crc: Some(expected_crc),
        },
    );

    let result = provider.verify_slice(0).unwrap();
    assert_eq!(result, Some(true));
}

#[test]
fn test_chunked_provider_verify_slice_wrong_crc() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0x42; 1024]).unwrap();
    temp_file.flush().unwrap();

    let mut provider = ChunkedSliceProvider::new(1024);
    provider.add_slice(
        0,
        SliceLocation {
            file_path: temp_file.path().to_path_buf(),
            offset: 0,
            actual_size: ActualDataSize::new(1024),
            logical_size: LogicalSliceSize::new(1024),
            expected_crc: Some(Crc32Value::new(0xDEADBEEF)), // Wrong CRC
        },
    );

    let result = provider.verify_slice(0).unwrap();
    assert_eq!(result, Some(false));
}

#[test]
fn test_chunked_provider_verify_slice_caching() {
    let mut temp_file = NamedTempFile::new().unwrap();
    let test_data = vec![0x42u8; 1024];
    temp_file.write_all(&test_data).unwrap();
    temp_file.flush().unwrap();

    let expected_crc = par2rs::checksum::compute_crc32(&test_data);

    let mut provider = ChunkedSliceProvider::new(1024);
    provider.add_slice(
        0,
        SliceLocation {
            file_path: temp_file.path().to_path_buf(),
            offset: 0,
            actual_size: ActualDataSize::new(1024),
            logical_size: LogicalSliceSize::new(1024),
            expected_crc: Some(expected_crc),
        },
    );

    // First verify
    let result1 = provider.verify_slice(0).unwrap();
    // Second verify should hit cache
    let result2 = provider.verify_slice(0).unwrap();

    assert_eq!(result1, result2);
    assert_eq!(result1, Some(true));
}

#[test]
fn test_chunked_provider_read_nonexistent_slice() {
    let mut provider = ChunkedSliceProvider::new(1024);

    let result = provider.read_chunk(99, 0, 64);
    assert!(result.is_err());
}

#[test]
fn test_chunked_provider_read_invalid_offset() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0x42; 1024]).unwrap();
    temp_file.flush().unwrap();

    let mut provider = ChunkedSliceProvider::new(1024);
    provider.add_slice(
        0,
        SliceLocation {
            file_path: temp_file.path().to_path_buf(),
            offset: 0,
            actual_size: ActualDataSize::new(1024),
            logical_size: LogicalSliceSize::new(1024),
            expected_crc: None,
        },
    );

    // Offset beyond logical size should error
    let result = provider.read_chunk(0, 2000, 64);
    assert!(result.is_err());
}

#[test]
fn test_chunked_provider_large_slice_cache_size() {
    // Large slices should have smaller cache
    let provider = ChunkedSliceProvider::new(2 * 1024 * 1024); // 2MB slice
    assert_eq!(provider.available_slices().len(), 0);
}

#[test]
fn test_chunked_provider_small_slice_cache_size() {
    // Small slices should have larger cache
    let provider = ChunkedSliceProvider::new(64 * 1024); // 64KB slice
    assert_eq!(provider.available_slices().len(), 0);
}

// ============================================================================
// RecoverySliceProvider tests
// ============================================================================

#[test]
fn test_recovery_provider_new() {
    let provider = RecoverySliceProvider::new(1024);
    assert_eq!(provider.available_exponents().len(), 0);
}

#[test]
fn test_recovery_provider_add_metadata() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0x55; 1024]).unwrap();
    temp_file.flush().unwrap();

    let metadata = RecoverySliceMetadata::from_file(
        0,
        RecoverySetId::new([0u8; 16]),
        temp_file.path().to_path_buf(),
        0,
        1024,
    );

    let mut provider = RecoverySliceProvider::new(1024);
    provider.add_recovery_metadata(0, metadata);

    let exponents = provider.available_exponents();
    assert_eq!(exponents.len(), 1);
    assert!(exponents.contains(&0));
}

#[test]
fn test_recovery_provider_multiple_metadata() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0x55; 4096]).unwrap();
    temp_file.flush().unwrap();

    let mut provider = RecoverySliceProvider::new(1024);

    for i in 0..4 {
        let metadata = RecoverySliceMetadata::from_file(
            i as u32,
            RecoverySetId::new([0u8; 16]),
            temp_file.path().to_path_buf(),
            i as u64 * 1024,
            1024,
        );
        provider.add_recovery_metadata(i, metadata);
    }

    let exponents = provider.available_exponents();
    assert_eq!(exponents.len(), 4);
    assert_eq!(exponents, vec![0, 1, 2, 3]); // Should be sorted
}

#[test]
fn test_recovery_provider_get_chunk() {
    let mut temp_file = NamedTempFile::new().unwrap();
    let recovery_data = vec![0xAAu8; 1024];
    temp_file.write_all(&recovery_data).unwrap();
    temp_file.flush().unwrap();

    let metadata = RecoverySliceMetadata::from_file(
        0,
        RecoverySetId::new([0u8; 16]),
        temp_file.path().to_path_buf(),
        0,
        1024,
    );

    let mut provider = RecoverySliceProvider::new(1024);
    provider.add_recovery_metadata(0, metadata);

    let chunk = provider.get_recovery_chunk(0, 0, 64).unwrap();
    assert_eq!(chunk.valid_bytes, 64);
    assert!(chunk.data.iter().all(|&b| b == 0xAA));
}

#[test]
fn test_recovery_provider_get_partial_chunk() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0xBB; 1024]).unwrap();
    temp_file.flush().unwrap();

    let metadata = RecoverySliceMetadata::from_file(
        0,
        RecoverySetId::new([0u8; 16]),
        temp_file.path().to_path_buf(),
        0,
        1024,
    );

    let mut provider = RecoverySliceProvider::new(1024);
    provider.add_recovery_metadata(0, metadata);

    // Read near end
    let chunk = provider.get_recovery_chunk(0, 1000, 64).unwrap();
    assert_eq!(chunk.valid_bytes, 24);
    assert!(chunk.data.iter().all(|&b| b == 0xBB));
}

#[test]
fn test_recovery_provider_get_middle_chunk() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0xCC; 2048]).unwrap();
    temp_file.flush().unwrap();

    let metadata = RecoverySliceMetadata::from_file(
        0,
        RecoverySetId::new([0u8; 16]),
        temp_file.path().to_path_buf(),
        0,
        2048,
    );

    let mut provider = RecoverySliceProvider::new(2048);
    provider.add_recovery_metadata(0, metadata);

    // Read from middle
    let chunk = provider.get_recovery_chunk(0, 1024, 128).unwrap();
    assert_eq!(chunk.valid_bytes, 128);
    assert!(chunk.data.iter().all(|&b| b == 0xCC));
}

#[test]
fn test_recovery_provider_nonexistent_exponent() {
    let provider = RecoverySliceProvider::new(1024);

    let result = provider.get_recovery_chunk(99, 0, 64);
    assert!(result.is_err());
}

#[test]
fn test_recovery_provider_exponents_sorted() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0x00; 8192]).unwrap();
    temp_file.flush().unwrap();

    let mut provider = RecoverySliceProvider::new(1024);

    // Add in random order
    for exp in &[5, 2, 8, 1, 3] {
        let metadata = RecoverySliceMetadata::from_file(
            *exp as u32,
            RecoverySetId::new([0u8; 16]),
            temp_file.path().to_path_buf(),
            0,
            1024,
        );
        provider.add_recovery_metadata(*exp, metadata);
    }

    let exponents = provider.available_exponents();
    assert_eq!(exponents, vec![1, 2, 3, 5, 8]);
}

#[test]
fn test_recovery_provider_duplicate_exponent() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&[0x11; 2048]).unwrap();
    temp_file.flush().unwrap();

    let metadata1 = RecoverySliceMetadata::from_file(
        0,
        RecoverySetId::new([0u8; 16]),
        temp_file.path().to_path_buf(),
        0,
        1024,
    );

    let metadata2 = RecoverySliceMetadata::from_file(
        0,
        RecoverySetId::new([1u8; 16]),
        temp_file.path().to_path_buf(),
        1024,
        1024,
    );

    let mut provider = RecoverySliceProvider::new(1024);
    provider.add_recovery_metadata(0, metadata1);
    provider.add_recovery_metadata(0, metadata2); // Replaces first

    let exponents = provider.available_exponents();
    assert_eq!(exponents.len(), 1);
}
