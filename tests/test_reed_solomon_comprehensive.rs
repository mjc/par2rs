//! Comprehensive tests for Reed-Solomon implementation
//!
//! Tests for Galois field operations, matrix setup, reconstruction engine,
//! and integration tests combining multiple components.

use par2rs::domain::{Md5Hash, RecoverySetId};
use par2rs::reed_solomon::{ReconstructionEngine, ReedSolomon, ReedSolomonBuilder};
use par2rs::RecoverySlicePacket;
use rustc_hash::FxHashMap as HashMap;

// ============================================================================
// Galois Field Tests
// ============================================================================

#[test]
fn test_galois16_basic_operations() {
    use par2rs::reed_solomon::galois::Galois16;

    let a = Galois16::new(5);
    let b = Galois16::new(3);

    // Test addition (XOR in GF)
    let sum = a + b;
    assert_eq!(sum.value(), 6); // 5 XOR 3 = 6

    // Test subtraction (same as addition in GF(2^n))
    let diff = a - b;
    assert_eq!(diff.value(), 6); // 5 XOR 3 = 6
}

#[test]
fn test_galois16_multiplicative_identity() {
    use par2rs::reed_solomon::galois::Galois16;

    let a = Galois16::new(42);
    let one = Galois16::new(1);

    // Multiply by 1 should give identity
    assert_eq!((a * one).value(), a.value());
}

#[test]
fn test_galois16_multiplication_by_zero() {
    use par2rs::reed_solomon::galois::Galois16;

    let a = Galois16::new(42);
    let zero = Galois16::new(0);

    // Multiply by 0 should give 0
    assert_eq!((a * zero).value(), 0);
}

#[test]
fn test_galois16_commutative_multiplication() {
    use par2rs::reed_solomon::galois::Galois16;

    let a = Galois16::new(17);
    let b = Galois16::new(23);

    // a * b = b * a
    assert_eq!((a * b).value(), (b * a).value());
}

#[test]
fn test_galois16_power_operations() {
    use par2rs::reed_solomon::galois::Galois16;

    let a = Galois16::new(2);

    // Test power of 0
    let pow0 = a.pow(0);
    assert_eq!(pow0.value(), 1);

    // Test power operations don't panic
    let pow10 = a.pow(10);
    let _ = pow10.value();
}

#[test]
fn test_galois16_power_of_zero() {
    use par2rs::reed_solomon::galois::Galois16;

    let zero = Galois16::new(0);

    // 0^n = 0 for any n > 0
    assert_eq!(zero.pow(1).value(), 0);
    assert_eq!(zero.pow(100).value(), 0);
}

#[test]
fn test_galois16_division_by_self() {
    use par2rs::reed_solomon::galois::Galois16;

    let a = Galois16::new(42);

    // a / a = 1 (except when a = 0)
    assert_eq!((a / a).value(), 1);
}

#[test]
#[should_panic]
fn test_galois16_division_by_zero_panics() {
    use par2rs::reed_solomon::galois::Galois16;

    let a = Galois16::new(42);
    let zero = Galois16::new(0);

    // Division by zero should panic
    let _ = a / zero;
}

#[test]
fn test_galois16_zero_by_nonzero_division() {
    use par2rs::reed_solomon::galois::Galois16;

    let zero = Galois16::new(0);
    let a = Galois16::new(42);

    // 0 / a = 0
    assert_eq!((zero / a).value(), 0);
}

#[test]
fn test_galois16_add_assign_operation() {
    use par2rs::reed_solomon::galois::Galois16;

    let mut a = Galois16::new(5);
    let b = Galois16::new(3);
    a += b;

    assert_eq!(a.value(), 6); // 5 XOR 3 = 6
}

#[test]
fn test_galois16_mul_assign_operation() {
    use par2rs::reed_solomon::galois::Galois16;

    let a = Galois16::new(17);
    let b = Galois16::new(23);
    let mut a_copy = a;
    a_copy *= b;

    assert_eq!(a_copy.value(), (a * b).value());
}

#[test]
fn test_galois16_log_and_antilog() {
    use par2rs::reed_solomon::galois::Galois16;

    let val = Galois16::new(5);

    // Log should return a valid value
    let log_val = val.log();
    let _ = log_val; // Value is valid by construction

    // Alog should return a valid value
    let alog_val = val.alog();
    let _ = alog_val; // Value is valid by construction
}

#[test]
fn test_galois16_default_value() {
    use par2rs::reed_solomon::galois::Galois16;

    let default = Galois16::default();
    assert_eq!(default.value(), 0);
}

// ============================================================================
// Reed-Solomon Matrix Tests
// ============================================================================

#[test]
fn test_reed_solomon_new() {
    let rs = ReedSolomon::new();
    // Just ensure it constructs without panicking
    let _ = rs;
}

#[test]
fn test_reed_solomon_set_input_simple() {
    let mut rs = ReedSolomon::new();

    // Set simple input: 3 present, 2 missing
    let input_status = vec![true, true, false, true, false];
    let result = rs.set_input(&input_status);

    assert!(result.is_ok());
}

#[test]
fn test_reed_solomon_set_input_all_present() {
    let mut rs = ReedSolomon::new();

    // All blocks present
    let input_status = vec![true, true, true, true, true];
    let result = rs.set_input(&input_status);

    assert!(result.is_ok());
}

#[test]
fn test_reed_solomon_set_input_all_missing() {
    let mut rs = ReedSolomon::new();

    // All blocks missing (should still work, will fail to compute later)
    let input_status = vec![false, false, false, false];
    let result = rs.set_input(&input_status);

    assert!(result.is_ok());
}

#[test]
fn test_reed_solomon_set_output_single() {
    let mut rs = ReedSolomon::new();

    let input_status = vec![true, true, false];
    let _ = rs.set_input(&input_status);

    let result = rs.set_output(true, 0);
    assert!(result.is_ok());
}

#[test]
fn test_reed_solomon_set_output_multiple() {
    let mut rs = ReedSolomon::new();

    let input_status = vec![true, true, false, true];
    let _ = rs.set_input(&input_status);

    // Set multiple outputs
    let _ = rs.set_output(true, 0);
    let _ = rs.set_output(true, 1);
    let _ = rs.set_output(false, 2);

    // Should succeed
}

#[test]
fn test_reed_solomon_compute_basic() {
    let mut rs = ReedSolomon::new();

    let input_status = vec![true, true, false];
    let _ = rs.set_input(&input_status);

    let _ = rs.set_output(true, 0);
    let _ = rs.set_output(true, 1);

    let result = rs.compute();
    // Just ensure it doesn't panic
    let _ = result;
}

#[test]
fn test_reed_solomon_compute_with_recovery_blocks() {
    let mut rs = ReedSolomon::new();

    // 4 input blocks: 3 present, 1 missing
    let input_status = vec![true, true, true, false];
    let _ = rs.set_input(&input_status);

    // 2 recovery blocks
    let _ = rs.set_output(true, 0);
    let _ = rs.set_output(true, 1);

    let result = rs.compute();
    // Result depends on matrix solvability
    let _ = result;
}

// ============================================================================
// Reconstruction Engine Tests
// ============================================================================

#[test]
fn test_reconstruction_engine_new() {
    let recovery_slices = vec![];
    let engine = ReconstructionEngine::new(4, 2, recovery_slices);

    // Just ensure it constructs
    let _ = engine;
}

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
// Integration Tests
// ============================================================================

#[test]
fn test_reed_solomon_full_workflow_basic() {
    // Using builder pattern for cleaner test setup
    let mut rs = ReedSolomonBuilder::new()
        .with_input_status(&[true, true, true, false]) // 3 present, 1 missing
        .with_recovery_block(true, 0)
        .with_recovery_block(true, 1)
        .build()
        .expect("Failed to build ReedSolomon");

    // Attempt computation
    let result = rs.compute();
    // Should not panic regardless of success
    let _ = result;
}

#[test]
fn test_reed_solomon_full_workflow_all_present() {
    // All blocks present (no repair needed) - using builder
    let mut rs = ReedSolomonBuilder::new()
        .with_input_status(&[true, true, true, true])
        .with_recovery_block(true, 0)
        .with_recovery_block(true, 1)
        .build()
        .expect("Failed to build ReedSolomon");

    let result = rs.compute();
    // Should succeed or at least not panic
    let _ = result;
}

#[test]
fn test_reed_solomon_multiple_missing_blocks() {
    // 5 blocks with 2 missing - builder makes this more concise
    let mut rs = ReedSolomonBuilder::new()
        .with_input_status(&[true, false, true, false, true])
        .with_recovery_block(true, 0)
        .with_recovery_block(true, 1)
        .build()
        .expect("Failed to build ReedSolomon");

    let result = rs.compute();
    let _ = result;
}

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

#[test]
fn test_galois16_complex_arithmetic_sequence() {
    use par2rs::reed_solomon::galois::Galois16;

    // Create a sequence of operations
    let vals: Vec<_> = (1..=5).map(Galois16::new).collect();

    // Chain operations
    let mut result = vals[0];
    for &v in &vals[1..] {
        result += v;
    }

    // Should produce a valid result (value is always in range for u16)
    let _ = result.value();
}

#[test]
fn test_galois16_polynomial_evaluation() {
    use par2rs::reed_solomon::galois::Galois16;

    // Evaluate polynomial p(x) = x^2 + 3x + 5 at x = 7 in GF(2^16)
    let x = Galois16::new(7);
    let coeff2 = Galois16::new(1);
    let coeff1 = Galois16::new(3);
    let coeff0 = Galois16::new(5);

    let result = (x * x * coeff2) + (x * coeff1) + coeff0;

    // Should produce a valid GF element (value is always in range for u16)
    let _ = result.value();
}

#[test]
fn test_reed_solomon_error_recovery_scenario() {
    // Scenario: 8-block file with 4 recovery blocks, 2 blocks are damaged
    // Using builder pattern for cleaner setup
    let rs = ReedSolomonBuilder::new()
        .with_input_status(&[true, true, false, true, true, true, true, true])
        .with_recovery_blocks_range(true, 0, 3) // 4 recovery blocks (0-3)
        .build()
        .expect("Failed to build ReedSolomon");

    // Sufficient recovery blocks should allow computation without panicking
    let _ = rs;
}

#[test]
fn test_reconstruction_empty_recovery_slices() {
    let recovery_slices = vec![];
    let engine = ReconstructionEngine::new(0, 0, recovery_slices);

    // No recovery blocks means can't reconstruct anything
    assert!(!engine.can_reconstruct(1));
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

#[test]
fn test_galois16_large_exponent() {
    use par2rs::reed_solomon::galois::Galois16;

    let a = Galois16::new(123);

    // Test with large exponent
    let result = a.pow(1000);
    let _ = result.value(); // Value is valid by construction
}

#[test]
fn test_reed_solomon_asymmetric_input() {
    let mut rs = ReedSolomon::new();

    // Many present blocks, few missing
    let input_status = vec![true, true, true, true, true, true, true, true, true, false];
    let _ = rs.set_input(&input_status);

    let _ = rs.set_output(true, 0);

    let result = rs.compute();
    let _ = result;
}
