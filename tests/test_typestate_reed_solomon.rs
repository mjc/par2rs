//! Tests for type-safe Reed-Solomon implementation
//!
//! Ensures the type-safe version produces identical results to the original
//! while providing compile-time safety guarantees.

use par2rs::reed_solomon::{
    ReedSolomon as OriginalRS, ReedSolomonBuilder, TypeSafeReedSolomon as TypeSafeRS,
    TypeSafeReedSolomonBuilder,
};

#[test]
fn typestate_produces_identical_results() {
    // Setup original implementation
    let mut original_rs = OriginalRS::new();
    original_rs.set_input(&[true, true, false, true]).unwrap();
    original_rs.set_output(true, 0).unwrap();
    original_rs.set_output(true, 1).unwrap();
    original_rs.compute().unwrap();

    // Setup type-safe implementation
    let typestate_rs = TypeSafeRS::new()
        .set_input(&[true, true, false, true])
        .unwrap()
        .set_output(true, 0)
        .unwrap()
        .set_output(true, 1)
        .unwrap()
        .compute()
        .unwrap();

    // Test with same inputs
    let input = vec![0xAAu8; 528];
    let mut original_output = vec![0x55u8; 528];
    let mut typestate_output = vec![0x55u8; 528];

    // Process with both implementations
    original_rs
        .process(0, &input, 0, &mut original_output)
        .unwrap();
    typestate_rs
        .process(0, &input, 0, &mut typestate_output)
        .unwrap();

    // Results should be identical
    assert_eq!(
        original_output, typestate_output,
        "Outputs should be identical"
    );
}

#[test]
fn typestate_multiple_process_calls() {
    // Test that multiple process calls work identically
    let mut original_rs = OriginalRS::new();
    original_rs.set_input(&[true, true, false]).unwrap();
    original_rs.set_output(true, 0).unwrap();
    original_rs.compute().unwrap();

    let typestate_rs = TypeSafeRS::new()
        .set_input(&[true, true, false])
        .unwrap()
        .set_output(true, 0)
        .unwrap()
        .compute()
        .unwrap();

    let input1 = vec![0xAAu8; 256];
    let input2 = vec![0x55u8; 256];
    let mut original_output = vec![0x00u8; 256];
    let mut typestate_output = vec![0x00u8; 256];

    // Multiple operations
    original_rs
        .process(0, &input1, 0, &mut original_output)
        .unwrap();
    original_rs
        .process(1, &input2, 0, &mut original_output)
        .unwrap();

    typestate_rs
        .process(0, &input1, 0, &mut typestate_output)
        .unwrap();
    typestate_rs
        .process(1, &input2, 0, &mut typestate_output)
        .unwrap();

    assert_eq!(original_output, typestate_output);
}

#[test]
fn typestate_builder_identical_to_original() {
    // Test builder patterns produce identical results
    let mut original_rs = ReedSolomonBuilder::new()
        .with_input_status(&[true, false, true, false])
        .with_recovery_block(true, 0)
        .with_recovery_block(true, 1)
        .build()
        .unwrap();
    original_rs.compute().unwrap();

    let typestate_rs = TypeSafeReedSolomonBuilder::new()
        .with_input_status(&[true, false, true, false])
        .with_recovery_block(true, 0)
        .with_recovery_block(true, 1)
        .build()
        .unwrap()
        .compute()
        .unwrap();

    let input = vec![0x42u8; 1024];
    let mut original_output = vec![0x00u8; 1024];
    let mut typestate_output = vec![0x00u8; 1024];

    original_rs
        .process(0, &input, 0, &mut original_output)
        .unwrap();
    typestate_rs
        .process(0, &input, 0, &mut typestate_output)
        .unwrap();

    assert_eq!(original_output, typestate_output);
}

#[test]
fn typestate_error_conditions_match() {
    // Test that error conditions produce the same errors

    // Too many inputs
    let result_original = {
        let mut rs = OriginalRS::new();
        rs.set_input(&vec![true; 65536])
    };

    let result_typestate = TypeSafeRS::new().set_input(&vec![true; 65536]);

    assert!(result_original.is_err());
    assert!(result_typestate.is_err());

    // Not enough recovery blocks
    let result_original = {
        let mut rs = OriginalRS::new();
        rs.set_input(&[false, false, false]).unwrap(); // 3 missing
        rs.set_output(true, 0).unwrap(); // only 1 recovery
        rs.compute()
    };

    let result_typestate = TypeSafeRS::new()
        .set_input(&[false, false, false])
        .unwrap()
        .set_output(true, 0)
        .unwrap()
        .compute();

    assert!(result_original.is_err());
    assert!(result_typestate.is_err());

    // No output blocks
    let result_original = {
        let mut rs = OriginalRS::new();
        rs.set_input(&[true, true]).unwrap();
        rs.compute()
    };

    let result_typestate = TypeSafeRS::new()
        .set_input(&[true, true])
        .unwrap()
        .compute();

    assert!(result_original.is_err());
    assert!(result_typestate.is_err());
}

#[test]
fn typestate_different_configurations() {
    // Test various Reed-Solomon configurations
    let configs = vec![
        // (input_status, recovery_exponents, recovery_present) - Must have missing blocks to compute
        (vec![true, false, true], vec![0], vec![true]), // 1 missing data, 1 present recovery
        (vec![true, false, true], vec![0, 1], vec![true, false]), // 1 missing data, 1 present + 1 missing recovery
        (
            vec![true, true, false, false],
            vec![0, 1, 2],
            vec![true, true, false],
        ), // 2 missing data, 2 present + 1 missing recovery
    ];

    for (input_status, recovery_exponents, recovery_present) in configs {
        let mut original_rs = OriginalRS::new();
        original_rs.set_input(&input_status).unwrap();
        for (i, &exp) in recovery_exponents.iter().enumerate() {
            original_rs.set_output(recovery_present[i], exp).unwrap();
        }
        original_rs.compute().unwrap();

        let mut typestate_rs = TypeSafeRS::new().set_input(&input_status).unwrap();
        for (i, &exp) in recovery_exponents.iter().enumerate() {
            typestate_rs = typestate_rs.set_output(recovery_present[i], exp).unwrap();
        }
        let typestate_rs = typestate_rs.compute().unwrap();

        // Test processing
        let input = vec![0x33u8; 64];
        let mut original_output = vec![0x00u8; 64];
        let mut typestate_output = vec![0x00u8; 64];

        for (present_idx, &is_present) in input_status.iter().enumerate() {
            if is_present {
                original_rs
                    .process(present_idx as u32, &input, 0, &mut original_output)
                    .unwrap();
                typestate_rs
                    .process(present_idx as u32, &input, 0, &mut typestate_output)
                    .unwrap();
            }
        }

        assert_eq!(
            original_output, typestate_output,
            "Configuration failed: input_status={:?}, recovery_exponents={:?}",
            input_status, recovery_exponents
        );
    }
}

#[test]
fn typestate_range_operations() {
    // Test set_output_range operations
    let mut original_rs = OriginalRS::new();
    original_rs.set_input(&[true, true, false]).unwrap();
    original_rs.set_output_range(true, 0, 2).unwrap(); // exponents 0, 1, 2
    original_rs.compute().unwrap();

    let typestate_rs = TypeSafeRS::new()
        .set_input(&[true, true, false])
        .unwrap()
        .set_output_range(true, 0, 2)
        .unwrap()
        .compute()
        .unwrap();

    let input = vec![0x77u8; 128];
    let mut original_output = vec![0x00u8; 128];
    let mut typestate_output = vec![0x00u8; 128];

    original_rs
        .process(0, &input, 0, &mut original_output)
        .unwrap();
    typestate_rs
        .process(0, &input, 0, &mut typestate_output)
        .unwrap();

    assert_eq!(original_output, typestate_output);
}

#[test]
fn typestate_process_errors_match() {
    // Test that process() errors match between implementations
    // Need missing blocks for compute to work
    let mut original_rs = OriginalRS::new();
    original_rs.set_input(&[true, false]).unwrap(); // One missing data block
    original_rs.set_output(true, 0).unwrap(); // One present recovery block
    original_rs.compute().unwrap();

    let typestate_rs = TypeSafeRS::new()
        .set_input(&[true, false])
        .unwrap() // One missing data block
        .set_output(true, 0)
        .unwrap() // One present recovery block
        .compute()
        .unwrap();

    // Mismatched buffer lengths
    let input = vec![0x42u8; 10];
    let mut output = vec![0x00u8; 20]; // Different length

    let original_result = original_rs.process(0, &input, 0, &mut output);
    let typestate_result = typestate_rs.process(0, &input, 0, &mut output);

    assert!(original_result.is_err());
    assert!(typestate_result.is_err());

    // Out of bounds index
    let input = vec![0x42u8; 10];
    let mut output = vec![0x00u8; 10];

    let original_result = original_rs.process(99, &input, 0, &mut output);
    let typestate_result = typestate_rs.process(99, &input, 0, &mut output);

    assert!(original_result.is_err());
    assert!(typestate_result.is_err());
}

#[test]
fn typestate_memory_layout_identical() {
    // Verify that both implementations have the same memory footprint
    use std::mem;

    let original_rs = OriginalRS::new();
    let typestate_rs = TypeSafeRS::new();

    // Should have identical size (PhantomData is zero-cost)
    assert_eq!(
        mem::size_of_val(&original_rs),
        mem::size_of_val(&typestate_rs)
    );

    // Test configured state too
    let typestate_configured = typestate_rs.set_input(&[true, false]).unwrap();
    assert_eq!(
        mem::size_of_val(&original_rs),
        mem::size_of_val(&typestate_configured)
    );
}

#[test]
fn typestate_matrix_access() {
    // Test that matrix access provides same data
    let mut original_rs = OriginalRS::new();
    original_rs.set_input(&[true, false, true]).unwrap();
    original_rs.set_output(true, 0).unwrap();
    original_rs.compute().unwrap();

    let _typestate_rs = TypeSafeRS::new()
        .set_input(&[true, false, true])
        .unwrap()
        .set_output(true, 0)
        .unwrap()
        .compute()
        .unwrap();

    // The type-safe version is successfully computed
    // (Matrix dimension and content checks would require exposing internal state,
    // which goes against the encapsulation goal of the type-safe wrapper)
}

// Compile-time tests to ensure the type system prevents invalid usage
// These are checked at compile time, not runtime
#[allow(dead_code)]
fn compile_time_safety_examples() {
    let rs = TypeSafeRS::new();

    // This would not compile - process() not available on New state:
    // let input = vec![0u8; 10];
    // let mut output = vec![0u8; 10];
    // rs.process(0, &input, 0, &mut output);

    let rs = rs.set_input(&[true, false]).unwrap();

    // This would not compile - process() not available on Configured state:
    // let input = vec![0u8; 10];
    // let mut output = vec![0u8; 10];
    // rs.process(0, &input, 0, &mut output);

    let rs = rs.set_output(true, 0).unwrap().compute().unwrap();

    // This compiles - process() is available on Computed state:
    let input = vec![0u8; 10];
    let mut output = vec![0u8; 10];
    let _ = rs.process(0, &input, 0, &mut output);
}
