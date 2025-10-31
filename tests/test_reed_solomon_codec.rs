//! Comprehensive tests for Reed-Solomon codec
//!
//! These tests cover the exported functions from src/reed_solomon/codec.rs

use par2rs::domain::{Md5Hash, RecoverySetId};
use par2rs::reed_solomon::galois::Galois16;
use par2rs::reed_solomon::{build_split_mul_table, ReconstructionEngine};
use par2rs::RecoverySlicePacket;

// ============================================================================
// SplitMulTable Tests
// ============================================================================

#[test]
fn test_split_mul_table_zero_coefficient() {
    let table = build_split_mul_table(Galois16::new(0));

    // All entries should be zero for zero coefficient
    assert!(table.low.iter().all(|&x| x == 0));
    assert!(table.high.iter().all(|&x| x == 0));
}

#[test]
fn test_split_mul_table_identity_coefficient() {
    let table = build_split_mul_table(Galois16::new(1));

    // Test identity property
    for i in 0u8..=255 {
        assert_eq!(table.low[i as usize], i as u16);
        assert_eq!(table.high[i as usize], (i as u16) << 8);
    }
}

#[test]
fn test_split_mul_table_coefficient_2() {
    let table = build_split_mul_table(Galois16::new(2));

    // Test a few values
    let val = 0x1234u16;
    let low = val as u8;
    let high = (val >> 8) as u8;

    let result = table.low[low as usize] ^ table.high[high as usize];
    let expected = (Galois16::new(2) * Galois16::new(val)).value();

    assert_eq!(result, expected);
}

#[test]
fn test_split_mul_table_consistency() {
    let coeff = Galois16::new(42);
    let table = build_split_mul_table(coeff);

    // Test that table lookup matches direct multiplication
    for val in [0u16, 1, 255, 256, 0x1234, 0xFFFF] {
        let low = val as u8;
        let high = (val >> 8) as u8;

        let table_result = table.low[low as usize] ^ table.high[high as usize];
        let direct_result = (coeff * Galois16::new(val)).value();

        assert_eq!(table_result, direct_result, "Mismatch for value {:#x}", val);
    }
}

#[test]
fn test_split_mul_table_large_coefficient() {
    let table = build_split_mul_table(Galois16::new(0xFFFF));

    // Verify table is populated (non-zero entries exist)
    assert!(table.low.iter().any(|&x| x != 0));
    assert!(table.high.iter().any(|&x| x != 0));
}

#[test]
fn test_split_mul_table_deterministic() {
    let coeff = Galois16::new(123);
    let table1 = build_split_mul_table(coeff);
    let table2 = build_split_mul_table(coeff);

    // Tables should be identical
    for i in 0..256 {
        assert_eq!(table1.low[i], table2.low[i]);
        assert_eq!(table1.high[i], table2.high[i]);
    }
}

// ============================================================================
// ReconstructionEngine Tests
// ============================================================================

#[test]
fn test_reconstruction_engine_new() {
    let recovery_slices = vec![];
    let engine = ReconstructionEngine::new(4, 100, recovery_slices);
    let _ = engine; // Ensure it constructs
}

#[test]
fn test_reconstruction_empty_recovery() {
    let engine = ReconstructionEngine::new(0, 0, vec![]);
    assert!(!engine.can_reconstruct(1));
}

#[test]
fn test_reconstruction_zero_missing_always_ok() {
    let engine = ReconstructionEngine::new(4, 100, vec![]);
    assert!(engine.can_reconstruct(0));
}

#[test]
fn test_reconstruction_with_recovery_blocks() {
    let recovery_slices = vec![
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 0,
            recovery_data: vec![0x01; 100],
        },
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 1,
            recovery_data: vec![0x02; 100],
        },
    ];

    let engine = ReconstructionEngine::new(4, 100, recovery_slices);

    // With 2 recovery blocks, can reconstruct up to 2 missing
    assert!(engine.can_reconstruct(1));
    assert!(engine.can_reconstruct(2));
    assert!(!engine.can_reconstruct(3));
}

#[test]
fn test_reconstruction_cannot_exceed_recovery_count() {
    let recovery_slices = vec![RecoverySlicePacket {
        length: 64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
        type_of_packet: *b"PAR 2.0\0RecvSlic",
        exponent: 0,
        recovery_data: vec![0xFF; 100],
    }];

    let engine = ReconstructionEngine::new(5, 100, recovery_slices);

    assert!(engine.can_reconstruct(1));
    assert!(!engine.can_reconstruct(2));
}

#[test]
fn test_reconstruction_multiple_blocks() {
    let recovery_slices = vec![
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([1; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 0,
            recovery_data: vec![0xAA; 50],
        },
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([2; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 1,
            recovery_data: vec![0xBB; 50],
        },
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([3; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 2,
            recovery_data: vec![0xCC; 50],
        },
    ];

    let engine = ReconstructionEngine::new(6, 50, recovery_slices);

    assert!(engine.can_reconstruct(3));
    assert!(!engine.can_reconstruct(4));
}

#[test]
fn test_reconstruction_different_slice_sizes() {
    let large = vec![RecoverySlicePacket {
        length: 64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
        type_of_packet: *b"PAR 2.0\0RecvSlic",
        exponent: 0,
        recovery_data: vec![0xFF; 1024],
    }];

    let small = vec![RecoverySlicePacket {
        length: 64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
        type_of_packet: *b"PAR 2.0\0RecvSlic",
        exponent: 0,
        recovery_data: vec![0xFF; 16],
    }];

    let engine_large = ReconstructionEngine::new(4, 1024, large);
    let engine_small = ReconstructionEngine::new(4, 16, small);

    assert!(engine_large.can_reconstruct(1));
    assert!(engine_small.can_reconstruct(1));
}

#[test]
fn test_reconstruction_varying_exponents() {
    let recovery_slices = vec![
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 5, // Non-sequential exponents
            recovery_data: vec![0x01; 100],
        },
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 10,
            recovery_data: vec![0x02; 100],
        },
    ];

    let engine = ReconstructionEngine::new(8, 100, recovery_slices);
    assert!(engine.can_reconstruct(2));
}

#[test]
fn test_reconstruction_max_recovery_blocks() {
    let large_count: usize = 50;
    let recovery_slices: Vec<_> = (0..large_count as u32)
        .map(|i| RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([i as u8; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: i,
            recovery_data: vec![i as u8; 64],
        })
        .collect();

    let engine = ReconstructionEngine::new(100, 64, recovery_slices);

    assert!(engine.can_reconstruct(large_count));
    assert!(!engine.can_reconstruct(large_count + 1));
}
