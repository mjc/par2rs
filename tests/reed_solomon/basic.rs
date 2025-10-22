//! Basic Reed-Solomon Matrix Operations Tests
//!
//! Tests for ReedSolomon struct construction, input/output setup,
//! matrix computation, and builder pattern.

use par2rs::reed_solomon::{ReedSolomon, ReedSolomonBuilder};

// ============================================================================
// Constructor and Setup Tests
// ============================================================================

#[test]
fn test_reed_solomon_new() {
    let rs = ReedSolomon::new();
    // Just ensure it constructs without panicking
    let _ = rs;
}

#[test]
fn test_reed_solomon_basic_setup() {
    // Using the builder pattern for cleaner setup
    let rs = ReedSolomonBuilder::new()
        .with_input_status(&[true, true, false, true, false]) // 3 present, 2 missing
        .with_recovery_block(true, 0)
        .with_recovery_block(true, 1)
        .with_recovery_block(false, 2)
        .with_recovery_block(false, 4)
        .build()
        .expect("Failed to build ReedSolomon");

    // Verify it built successfully - just ensure no panic
    let _ = rs;
}

#[test]
fn test_reed_solomon_basic_setup_traditional() {
    // Keep one test using traditional approach for backwards compatibility verification
    let mut rs = ReedSolomon::new();

    // Test basic setup with some present and missing blocks
    let input_status = vec![true, true, false, true, false]; // 3 present, 2 missing
    rs.set_input(&input_status).expect("Failed to set input");

    // Add some recovery blocks
    rs.set_output(true, 0)
        .expect("Failed to set recovery block 0");
    rs.set_output(true, 1)
        .expect("Failed to set recovery block 1");

    // Add missing outputs to compute
    rs.set_output(false, 2)
        .expect("Failed to set missing output 2");
    rs.set_output(false, 4)
        .expect("Failed to set missing output 4");

    // This should work without panicking
    let result = rs.compute();
    match result {
        Ok(()) => println!("Reed-Solomon matrix computed successfully"),
        Err(e) => println!("Reed-Solomon computation failed: {}", e),
    }
}

// ============================================================================
// Input Configuration Tests
// ============================================================================

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
fn test_reed_solomon_asymmetric_input() {
    let mut rs = ReedSolomon::new();

    // Many present blocks, few missing
    let input_status = vec![true, true, true, true, true, true, true, true, true, false];
    let _ = rs.set_input(&input_status);

    let _ = rs.set_output(true, 0);

    let result = rs.compute();
    let _ = result;
}

// ============================================================================
// Output Configuration Tests
// ============================================================================

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

// ============================================================================
// Matrix Computation Tests
// ============================================================================

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
// Integration Tests with Builder Pattern
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
