//! Additional tests for rolling CRC to hit edge cases in gf32_multiply

use par2rs::checksum::rolling_crc::RollingCrcTable;

#[test]
fn test_extreme_window_sizes() {
    // Test very small and very large window sizes to exercise crc_exp8 edge cases
    let sizes = vec![
        1, 2, 7,       // Prime number
        13,      // Another prime
        255,     // Just under power of 2
        256,     // Power of 2
        257,     // Just over power of 2
        1000,    // Round number
        4095,    // Just under power of 2
        4096,    // Power of 2
        4097,    // Just over power of 2
        65535,   // 16-bit max
        65536,   // 64K
        1048576, // 1MB - common PAR2 block size
    ];

    for size in sizes {
        let table = RollingCrcTable::new(size);
        assert_eq!(table.window_size(), size);

        // Test that sliding works
        let test_crc = 0x12345678u32;
        let result = table.slide(test_crc, 0xAA, 0x55);

        // Should produce different CRC
        assert_ne!(result, test_crc, "Window size {} didn't change CRC", size);
    }
}

#[test]
fn test_all_byte_combinations() {
    // Test sliding with all possible byte value combinations
    let table = RollingCrcTable::new(128);

    for byte_in in [0x00, 0x01, 0x7F, 0x80, 0xFE, 0xFF] {
        for byte_out in [0x00, 0x01, 0x7F, 0x80, 0xFE, 0xFF] {
            let crc = table.slide(0x12345678, byte_in, byte_out);
            // Should not panic or produce invalid output
            let _ = crc;
        }
    }
}

#[test]
fn test_sequential_window_sizes() {
    // Test many sequential window sizes to exercise gf32_multiply
    for size in 1..=200 {
        let table = RollingCrcTable::new(size);
        assert_eq!(table.window_size(), size);

        // Quick sanity check
        let crc = table.slide(0, 0xFF, 0x00);
        let _ = crc; // Should not panic
    }
}

#[test]
fn test_window_power_calculations() {
    // Test window sizes that exercise different modulo paths in crc_exp8
    let sizes = vec![
        0xFFFF,   // Tests large n value
        0x10000,  // Power of 2
        0x10001,  // Just over power of 2
        0xFFFFF,  // Even larger
        0x100000, // 1MB
    ];

    for size in sizes {
        let table = RollingCrcTable::new(size);

        // Verify table works
        let crc1 = table.slide(0x11111111, 0xAA, 0x55);
        let crc2 = table.slide(0x22222222, 0xAA, 0x55);

        // Different input CRCs should produce different outputs
        assert_ne!(crc1, crc2);
    }
}

#[test]
fn test_gf_multiply_edge_cases_via_window() {
    // Test window sizes that will cause gf32_multiply to go through
    // different paths (high bits set, low bits set, etc.)

    // These sizes will cause crc_exp8 to exercise gf32_multiply with
    // various bit patterns
    for n in [1, 31, 32, 63, 64, 127, 128, 255, 256, 511, 512] {
        let table = RollingCrcTable::new(n);

        // Test multiple slides to exercise the internal functions
        let mut crc = 0x80000000u32; // High bit set
        for _ in 0..10 {
            crc = table.slide(crc, 0xFF, 0x00);
        }

        // Should produce valid CRC
        assert_ne!(crc, 0x80000000);
    }
}

#[test]
fn test_repeated_table_generation() {
    // Generate same table multiple times to ensure consistency
    let table1 = RollingCrcTable::new(1024);
    let table2 = RollingCrcTable::new(1024);

    // Both should produce same results
    let crc1 = table1.slide(0x12345678, 0xAA, 0x55);
    let crc2 = table2.slide(0x12345678, 0xAA, 0x55);

    assert_eq!(crc1, crc2);
}

#[test]
fn test_maximum_practical_window() {
    // Test largest practical window size (16MB PAR2 blocks)
    let table = RollingCrcTable::new(16 * 1024 * 1024);

    let crc = table.slide(0xFFFFFFFF, 0xFF, 0x00);
    assert_ne!(crc, 0xFFFFFFFF);
}

#[test]
fn test_odd_number_windows() {
    // Test various odd-numbered window sizes
    for size in [3, 5, 7, 9, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47] {
        let table = RollingCrcTable::new(size);

        let crc = table.slide(0xAAAAAAAA, 0x55, 0xAA);
        // Should not panic
        let _ = crc;
    }
}
