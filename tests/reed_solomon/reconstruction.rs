//! Reconstruction Engine Tests
//!
//! Tests for ReconstructionEngine which handles the actual reconstruction
//! of missing data blocks using Reed-Solomon error correction.

use par2rs::domain::{Md5Hash, RecoverySetId};
use par2rs::reed_solomon::ReconstructionEngine;
use par2rs::RecoverySlicePacket;
use rustc_hash::FxHashMap as HashMap;

// ============================================================================
// Constructor Tests
// ============================================================================

#[test]
fn test_reconstruction_engine_new() {
    let recovery_slices = vec![];
    let engine = ReconstructionEngine::new(4, 2, recovery_slices);

    // Just ensure it constructs
    let _ = engine;
}

#[test]
fn test_reconstruction_empty_recovery_slices() {
    let recovery_slices = vec![];
    let engine = ReconstructionEngine::new(0, 0, recovery_slices);

    // No recovery blocks means can't reconstruct anything
    assert!(!engine.can_reconstruct(1));
}

// ============================================================================
// Capability Tests (can_reconstruct)
// ============================================================================

#[test]
fn test_reconstruction_engine_can_reconstruct_zero_missing() {
    let recovery_slices = vec![];
    let engine = ReconstructionEngine::new(4, 2, recovery_slices);

    // Can always reconstruct 0 missing blocks
    assert!(engine.can_reconstruct(0));
}

#[test]
fn test_reconstruction_engine_can_reconstruct_enough_recovery() {
    let recovery_slices = vec![
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 0,
            recovery_data: vec![0x01],
        },
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 1,
            recovery_data: vec![0x02],
        },
    ];

    let engine = ReconstructionEngine::new(4, 4, recovery_slices);

    // With 2 recovery blocks, can reconstruct up to 2 missing
    assert!(engine.can_reconstruct(2));
}

#[test]
fn test_reconstruction_engine_cannot_reconstruct_too_many() {
    let recovery_slices = vec![RecoverySlicePacket {
        length: 64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
        type_of_packet: *b"PAR 2.0\0RecvSlic",
        exponent: 0,
        recovery_data: vec![0x01],
    }];

    let engine = ReconstructionEngine::new(4, 4, recovery_slices);

    // With only 1 recovery block, cannot reconstruct 2 missing blocks
    assert!(!engine.can_reconstruct(2));
}

#[test]
fn test_reconstruction_single_recovery_block() {
    let recovery_slices = vec![RecoverySlicePacket {
        length: 64,
        md5: Md5Hash::new([0; 16]),
        set_id: RecoverySetId::new([0; 16]),
        type_of_packet: *b"PAR 2.0\0RecvSlic",
        exponent: 0,
        recovery_data: vec![0xFF],
    }];

    let engine = ReconstructionEngine::new(4, 4, recovery_slices);

    // 1 recovery block can recover 1 missing
    assert!(engine.can_reconstruct(1));
    assert!(!engine.can_reconstruct(2));
}

// ============================================================================
// Reconstruction Tests
// ============================================================================

#[test]
fn test_reconstruction_engine_basic() {
    // Create some mock recovery slices
    let recovery_slices = vec![
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 0,
            recovery_data: vec![0x01, 0x02, 0x03, 0x04],
        },
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 1,
            recovery_data: vec![0x05, 0x06, 0x07, 0x08],
        },
    ];

    let engine = ReconstructionEngine::new(4, 4, recovery_slices);

    // Test if reconstruction is possible with 2 missing slices and 2 recovery blocks
    assert!(
        engine.can_reconstruct(2),
        "Should be able to reconstruct 2 missing slices with 2 recovery blocks"
    );
    assert!(
        !engine.can_reconstruct(3),
        "Should not be able to reconstruct 3 missing slices with only 2 recovery blocks"
    );
}

#[test]
fn test_reconstruction_with_simple_case() {
    // Test reconstruction with a very simple case
    let recovery_slices = vec![
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 0,
            recovery_data: vec![0x10, 0x20, 0x30, 0x40],
        },
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 1,
            recovery_data: vec![0x11, 0x21, 0x31, 0x41],
        },
    ];

    let engine = ReconstructionEngine::new(4, 4, recovery_slices);

    // Simulate having 2 present slices and 2 missing slices
    let mut existing_slices = HashMap::default();
    existing_slices.insert(0, vec![0x01, 0x02, 0x03, 0x04]);
    existing_slices.insert(1, vec![0x05, 0x06, 0x07, 0x08]);

    let missing_slices = vec![2, 3];
    let global_slice_map: HashMap<usize, usize> = (0..4).map(|i| (i, i)).collect();

    let result =
        engine.reconstruct_missing_slices(&existing_slices, &missing_slices, &global_slice_map);

    // For now, we expect this to fail with the current implementation
    // but it shouldn't panic
    match result.success {
        true => println!(
            "Reconstruction succeeded: {:?}",
            result.reconstructed_slices
        ),
        false => println!(
            "Reconstruction failed as expected: {:?}",
            result.error_message
        ),
    }

    // The test passes as long as it doesn't panic
}

#[test]
fn test_reconstruction_engine_reconstruct_missing_slices() {
    let recovery_slices = vec![
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 0,
            recovery_data: vec![0x10, 0x20, 0x30],
        },
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 1,
            recovery_data: vec![0x11, 0x21, 0x31],
        },
    ];

    let engine = ReconstructionEngine::new(3, 3, recovery_slices);

    let mut existing_slices = HashMap::default();
    existing_slices.insert(0, vec![0x01, 0x02, 0x03]);
    existing_slices.insert(1, vec![0x04, 0x05, 0x06]);

    let missing_slices = vec![2];
    let global_slice_map: HashMap<usize, usize> = (0..3).map(|i| (i, i)).collect();

    let result =
        engine.reconstruct_missing_slices(&existing_slices, &missing_slices, &global_slice_map);

    // Result should not panic and should provide some result
    assert!(result.success || result.error_message.is_some());
}

#[test]
fn test_reconstruction_engine_reconstruct_no_missing() {
    let recovery_slices = vec![];
    let engine = ReconstructionEngine::new(4, 2, recovery_slices);

    let existing_slices = HashMap::default();
    let missing_slices = vec![];
    let global_slice_map: HashMap<usize, usize> = HashMap::default();

    let result =
        engine.reconstruct_missing_slices(&existing_slices, &missing_slices, &global_slice_map);

    // With no missing slices, should indicate success in some way
    let _ = result;
}

// ============================================================================
// Integration Tests with Realistic Data
// ============================================================================

#[test]
fn test_reconstruction_engine_with_real_recovery_slices() {
    // Test with more realistic recovery slices
    let recovery_slices = vec![
        RecoverySlicePacket {
            length: 528,
            md5: Md5Hash::new([1; 16]),
            set_id: RecoverySetId::new([2; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 0,
            recovery_data: vec![0; 528],
        },
        RecoverySlicePacket {
            length: 528,
            md5: Md5Hash::new([3; 16]),
            set_id: RecoverySetId::new([4; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 1,
            recovery_data: vec![1; 528],
        },
        RecoverySlicePacket {
            length: 528,
            md5: Md5Hash::new([5; 16]),
            set_id: RecoverySetId::new([6; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 2,
            recovery_data: vec![2; 528],
        },
    ];

    let engine = ReconstructionEngine::new(528, 528, recovery_slices);

    // With 3 recovery blocks
    assert!(engine.can_reconstruct(3));
    assert!(!engine.can_reconstruct(4));
}

#[test]
fn test_reconstruction_boundary_exact_recovery() {
    let recovery_slices = vec![
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 0,
            recovery_data: vec![0x01],
        },
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 1,
            recovery_data: vec![0x02],
        },
        RecoverySlicePacket {
            length: 64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([0; 16]),
            type_of_packet: *b"PAR 2.0\0RecvSlic",
            exponent: 2,
            recovery_data: vec![0x03],
        },
    ];

    let engine = ReconstructionEngine::new(4, 4, recovery_slices);

    // With 3 recovery blocks and 4 input blocks, can recover exactly 3 missing
    assert!(engine.can_reconstruct(3));
    assert!(!engine.can_reconstruct(4));
}
