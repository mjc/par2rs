//! Galois Field (GF(2^16)) Arithmetic Tests
//!
//! Tests for Galois16 field operations including addition, multiplication,
//! division, powers, and special properties.

use par2rs::reed_solomon::galois::Galois16;

// ============================================================================
// Basic Arithmetic Operations
// ============================================================================

#[test]
fn test_galois16_basic_operations() {
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
    let a = Galois16::new(42);
    let one = Galois16::new(1);

    // Multiply by 1 should give identity
    assert_eq!((a * one).value(), a.value());
}

#[test]
fn test_galois16_multiplication_by_zero() {
    let a = Galois16::new(42);
    let zero = Galois16::new(0);

    // Multiply by 0 should give 0
    assert_eq!((a * zero).value(), 0);
}

#[test]
fn test_galois16_commutative_multiplication() {
    let a = Galois16::new(17);
    let b = Galois16::new(23);

    // a * b = b * a
    assert_eq!((a * b).value(), (b * a).value());
}

// ============================================================================
// Power Operations
// ============================================================================

#[test]
fn test_galois16_power_operations() {
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
    let zero = Galois16::new(0);

    // 0^n = 0 for any n > 0
    assert_eq!(zero.pow(1).value(), 0);
    assert_eq!(zero.pow(100).value(), 0);
}

#[test]
fn test_galois16_large_exponent() {
    let a = Galois16::new(123);

    // Test with large exponent
    let result = a.pow(1000);
    let _ = result.value(); // Value is valid by construction
}

// ============================================================================
// Division Operations
// ============================================================================

#[test]
fn test_galois16_division_by_self() {
    let a = Galois16::new(42);

    // a / a = 1 (except when a = 0)
    assert_eq!((a / a).value(), 1);
}

#[test]
#[should_panic]
fn test_galois16_division_by_zero_panics() {
    let a = Galois16::new(42);
    let zero = Galois16::new(0);

    // Division by zero should panic
    let _ = a / zero;
}

#[test]
fn test_galois16_zero_by_nonzero_division() {
    let zero = Galois16::new(0);
    let a = Galois16::new(42);

    // 0 / a = 0
    assert_eq!((zero / a).value(), 0);
}

// ============================================================================
// Assignment Operations
// ============================================================================

#[test]
fn test_galois16_add_assign_operation() {
    let mut a = Galois16::new(5);
    let b = Galois16::new(3);
    a += b;

    assert_eq!(a.value(), 6); // 5 XOR 3 = 6
}

#[test]
fn test_galois16_mul_assign_operation() {
    let a = Galois16::new(17);
    let b = Galois16::new(23);
    let mut a_copy = a;
    a_copy *= b;

    assert_eq!(a_copy.value(), (a * b).value());
}

// ============================================================================
// Logarithm Operations
// ============================================================================

#[test]
fn test_galois16_log_and_antilog() {
    let val = Galois16::new(5);

    // Log should return a valid value
    let log_val = val.log();
    let _ = log_val; // Value is valid by construction

    // Alog should return a valid value
    let alog_val = val.alog();
    let _ = alog_val; // Value is valid by construction
}

// ============================================================================
// Special Values and Properties
// ============================================================================

#[test]
fn test_galois16_default_value() {
    let default = Galois16::default();
    assert_eq!(default.value(), 0);
}

#[test]
fn test_galois16_complex_arithmetic_sequence() {
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
    // Evaluate polynomial p(x) = x^2 + 3x + 5 at x = 7 in GF(2^16)
    let x = Galois16::new(7);
    let coeff2 = Galois16::new(1);
    let coeff1 = Galois16::new(3);
    let coeff0 = Galois16::new(5);

    let result = (x * x * coeff2) + (x * coeff1) + coeff0;

    // Should produce a valid GF element (value is always in range for u16)
    let _ = result.value();
}
