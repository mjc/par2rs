//! Comprehensive tests for rolling_crc.rs module
//!
//! These tests cover edge cases and internal functions to achieve 90%+ coverage.

use par2rs::checksum::rolling_crc::RollingCrcTable;

#[test]
fn test_window_table_zero_byte() {
    // Test table generation for edge case: byte value 0
    let table = RollingCrcTable::new(1024);
    
    // Test that sliding with byte 0 works
    let crc = table.slide(0x12345678, 0, 0);
    assert_ne!(crc, 0x12345678); // CRC should change
}

#[test]
fn test_window_table_all_bytes() {
    // Test that all 256 byte values work in sliding
    let table = RollingCrcTable::new(512);
    
    // Test sliding with various byte values
    for byte in 0u8..=255 {
        let crc = table.slide(0x12345678, byte, byte);
        // Should produce valid output (no panics)
        let _ = crc;
    }
}

#[test]
fn test_slide_with_same_byte() {
    // Test sliding when input and output are the same byte
    let table = RollingCrcTable::new(256);
    let initial_crc = 0x12345678u32;
    
    let byte = 0x42;
    let new_crc = table.slide(initial_crc, byte, byte);
    
    // CRC should change even with same byte (different positions)
    assert_ne!(new_crc, initial_crc);
}

#[test]
fn test_slide_all_zeros() {
    // Test sliding with all zero bytes
    let table = RollingCrcTable::new(128);
    
    use crc32fast::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(&vec![0; 128]);
    let initial_crc = hasher.finalize();
    
    let new_crc = table.slide(initial_crc, 0, 0);
    
    // Should produce same CRC (sliding window of all zeros)
    assert_eq!(new_crc, initial_crc);
}

#[test]
fn test_slide_all_ones() {
    // Test sliding with all 0xFF bytes
    let table = RollingCrcTable::new(128);
    
    use crc32fast::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(&vec![0xFF; 128]);
    let initial_crc = hasher.finalize();
    
    let new_crc = table.slide(initial_crc, 0xFF, 0xFF);
    
    // Should produce valid CRC
    assert_ne!(new_crc, 0);
}

#[test]
fn test_compute_crc_at_position_exact_fit() {
    // Test when block exactly fits in buffer
    let table = RollingCrcTable::new(64);
    let buffer = vec![0x01, 0x02, 0x03, 0x04, 0x05];
    let block_size = 3;
    let bytes_in_buffer = 5;
    
    let crc = table.compute_crc_at_position(&buffer, 0, block_size, bytes_in_buffer);
    assert!(crc.is_some());
}

#[test]
fn test_compute_crc_at_position_not_enough_data() {
    // Test when not enough data for a full block
    let table = RollingCrcTable::new(64);
    let buffer = vec![0x01, 0x02, 0x03];
    let block_size = 10; // Bigger than buffer
    let bytes_in_buffer = 3;
    
    let crc = table.compute_crc_at_position(&buffer, 0, block_size, bytes_in_buffer);
    assert!(crc.is_none());
}

#[test]
fn test_compute_crc_at_position_mid_buffer() {
    // Test computing CRC in the middle of a buffer
    let table = RollingCrcTable::new(64);
    let buffer = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07];
    let block_size = 3;
    let bytes_in_buffer = 8;
    
    // Compute CRC at positions 0, 1, 2
    let crc0 = table.compute_crc_at_position(&buffer, 0, block_size, bytes_in_buffer);
    let crc1 = table.compute_crc_at_position(&buffer, 1, block_size, bytes_in_buffer);
    let crc2 = table.compute_crc_at_position(&buffer, 2, block_size, bytes_in_buffer);
    
    assert!(crc0.is_some());
    assert!(crc1.is_some());
    assert!(crc2.is_some());
    
    // Different positions should produce different CRCs
    assert_ne!(crc0.unwrap(), crc1.unwrap());
    assert_ne!(crc1.unwrap(), crc2.unwrap());
}

#[test]
fn test_slide_crc_forward_valid() {
    // Test sliding CRC forward with valid data
    let table = RollingCrcTable::new(64);
    let buffer = vec![0x01; 100]; // 100 bytes of 0x01
    let block_size = 64;
    let bytes_in_buffer = 100;
    
    // Start at position 1 (not 0, because we need buffer[pos-1])
    let current_crc = table.compute_crc_at_position(&buffer, 1, block_size, bytes_in_buffer).unwrap();
    
    let new_crc = table.slide_crc_forward(current_crc, &buffer, 2, block_size, bytes_in_buffer);
    assert!(new_crc.is_some());
}

#[test]
fn test_slide_crc_forward_not_enough_data() {
    // Test sliding when not enough data remaining
    let table = RollingCrcTable::new(64);
    let buffer = vec![0x01; 50]; // Only 50 bytes
    let block_size = 64;
    let bytes_in_buffer = 50;
    
    let current_crc = 0x12345678;
    let new_crc = table.slide_crc_forward(current_crc, &buffer, 1, block_size, bytes_in_buffer);
    
    assert!(new_crc.is_none()); // Not enough data for a full block
}

#[test]
fn test_slide_crc_forward_boundary() {
    // Test sliding at buffer boundary
    let table = RollingCrcTable::new(32);
    let buffer = vec![0x42; 64]; // 64 bytes of 0x42
    let block_size = 32;
    let bytes_in_buffer = 64;
    
    // Position 32 should be the last valid position (32 + 32 = 64)
    let current_crc = table.compute_crc_at_position(&buffer, 32, block_size, bytes_in_buffer).unwrap();
    
    // Try to slide forward from position 33 (should fail: 33 + 32 = 65 > 64)
    let new_crc = table.slide_crc_forward(current_crc, &buffer, 33, block_size, bytes_in_buffer);
    assert!(new_crc.is_none());
}

#[test]
fn test_very_small_window() {
    // Test with minimum practical window size
    let table = RollingCrcTable::new(1);
    assert_eq!(table.window_size(), 1);
    
    let crc = table.slide(0, 0xFF, 0x00);
    assert_ne!(crc, 0);
}

#[test]
fn test_very_large_window() {
    // Test with large window size (common in PAR2)
    let table = RollingCrcTable::new(1024 * 1024); // 1MB
    assert_eq!(table.window_size(), 1024 * 1024);
    
    // Test that table works by sliding
    let crc = table.slide(0x12345678, 0xFF, 0x00);
    assert_ne!(crc, 0);
}

#[test]
fn test_power_of_two_window_sizes() {
    // Test various power-of-2 window sizes
    for &size in &[1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096] {
        let table = RollingCrcTable::new(size);
        assert_eq!(table.window_size(), size);
        
        // Verify table works by sliding
        let crc = table.slide(0x12345678, 0xAA, 0x55);
        assert_ne!(crc, 0x12345678, "Window size {} doesn't change CRC", size);
    }
}

#[test]
fn test_non_power_of_two_window_sizes() {
    // Test non-power-of-2 window sizes (common in PAR2 with slice sizes)
    for &size in &[100, 250, 500, 1000, 1500, 3000] {
        let table = RollingCrcTable::new(size);
        assert_eq!(table.window_size(), size);
        
        // Verify rolling CRC works
        let data: Vec<u8> = (0..(size * 2)).map(|i| (i % 256) as u8).collect();
        
        use crc32fast::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(&data[0..size]);
        let mut crc = hasher.finalize();
        
        // Slide once
        crc = table.slide(crc, data[size], data[0]);
        
        // Compute expected
        let mut expected_hasher = Hasher::new();
        expected_hasher.update(&data[1..=size]);
        let expected = expected_hasher.finalize();
        
        assert_eq!(crc, expected, "Failed for window size {}", size);
    }
}

#[test]
fn test_slide_multiple_times_consistency() {
    // Test that sliding multiple times is consistent
    let table = RollingCrcTable::new(64);
    let data: Vec<u8> = (0..200).map(|i| ((i * 7) % 256) as u8).collect();
    
    use crc32fast::Hasher;
    
    // Compute initial CRC for window [0..64]
    let mut hasher = Hasher::new();
    hasher.update(&data[0..64]);
    let mut crc = hasher.finalize();
    
    // Slide through positions 1..100
    for i in 1..100 {
        crc = table.slide(crc, data[i + 63], data[i - 1]);
        
        // Verify against direct computation
        let mut expected_hasher = Hasher::new();
        expected_hasher.update(&data[i..i + 64]);
        let expected = expected_hasher.finalize();
        
        assert_eq!(crc, expected, "Mismatch at position {}", i);
    }
}

#[test]
fn test_clone_table() {
    // Test that RollingCrcTable can be cloned
    let table1 = RollingCrcTable::new(256);
    let table2 = table1.clone();
    
    assert_eq!(table1.window_size(), table2.window_size());
    
    // Verify clones produce same results
    let crc1 = table1.slide(0x12345678, 0xAA, 0x55);
    let crc2 = table2.slide(0x12345678, 0xAA, 0x55);
    assert_eq!(crc1, crc2);
}

#[test]
fn test_edge_case_255_byte() {
    // Test with maximum byte value
    let table = RollingCrcTable::new(128);
    
    let crc = table.slide(0x12345678, 0xFF, 0xFF);
    assert_ne!(crc, 0);
}

#[test]
fn test_alternating_bytes() {
    // Test with alternating byte pattern
    let table = RollingCrcTable::new(64);
    let data: Vec<u8> = (0..128).map(|i| if i % 2 == 0 { 0xAA } else { 0x55 }).collect();
    
    use crc32fast::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(&data[0..64]);
    let mut crc = hasher.finalize();
    
    // Slide a few times
    for i in 1..10 {
        crc = table.slide(crc, data[i + 63], data[i - 1]);
        
        let mut expected = Hasher::new();
        expected.update(&data[i..i + 64]);
        assert_eq!(crc, expected.finalize());
    }
}
