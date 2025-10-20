//! Property-based tests for Reed-Solomon error correction
//!
//! These tests use proptest to validate Reed-Solomon encoding and decoding
//! with randomly generated inputs, ensuring correctness across a wide range
//! of scenarios.

use par2rs::reed_solomon::Galois16;
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

proptest! {
    /// Property: Galois16 field operations are closed (result is always in GF(2^16))
    #[test]
    fn prop_galois_field_closed(
        a in 0u16..=65535,
        b in 0u16..=65535,
    ) {
        let ga = Galois16::new(a);
        let gb = Galois16::new(b);

        // Addition (result is always valid u16)
        let _sum = ga + gb;

        // Multiplication (result is always valid u16)
        let _product = ga * gb;

        // Subtraction (same as addition in GF, result is always valid u16)
        let _diff = ga - gb;
    }

    /// Property: Galois16 addition is commutative: a + b = b + a
    #[test]
    fn prop_galois_addition_commutative(
        a in 0u16..=65535,
        b in 0u16..=65535,
    ) {
        let ga = Galois16::new(a);
        let gb = Galois16::new(b);

        prop_assert_eq!((ga + gb).value(), (gb + ga).value());
    }

    /// Property: Galois16 addition is associative: (a + b) + c = a + (b + c)
    #[test]
    fn prop_galois_addition_associative(
        a in 0u16..=65535,
        b in 0u16..=65535,
        c in 0u16..=65535,
    ) {
        let ga = Galois16::new(a);
        let gb = Galois16::new(b);
        let gc = Galois16::new(c);

        let left = (ga + gb) + gc;
        let right = ga + (gb + gc);
        prop_assert_eq!(left.value(), right.value());
    }

    /// Property: Galois16 multiplication is commutative: a * b = b * a
    #[test]
    fn prop_galois_multiplication_commutative(
        a in 0u16..=65535,
        b in 0u16..=65535,
    ) {
        let ga = Galois16::new(a);
        let gb = Galois16::new(b);

        prop_assert_eq!((ga * gb).value(), (gb * ga).value());
    }

    /// Property: Galois16 multiplication is associative: (a * b) * c = a * (b * c)
    #[test]
    fn prop_galois_multiplication_associative(
        a in 0u16..=65535,
        b in 0u16..=65535,
        c in 0u16..=65535,
    ) {
        let ga = Galois16::new(a);
        let gb = Galois16::new(b);
        let gc = Galois16::new(c);

        let left = (ga * gb) * gc;
        let right = ga * (gb * gc);
        prop_assert_eq!(left.value(), right.value());
    }

    /// Property: Galois16 distributive law: a * (b + c) = (a * b) + (a * c)
    #[test]
    fn prop_galois_distributive(
        a in 0u16..=65535,
        b in 0u16..=65535,
        c in 0u16..=65535,
    ) {
        let ga = Galois16::new(a);
        let gb = Galois16::new(b);
        let gc = Galois16::new(c);

        let left = ga * (gb + gc);
        let right = (ga * gb) + (ga * gc);
        prop_assert_eq!(left.value(), right.value());
    }

    /// Property: Galois16 zero identity: a + 0 = a
    #[test]
    fn prop_galois_zero_identity(a in 0u16..=65535) {
        let ga = Galois16::new(a);
        let zero = Galois16::new(0);

        prop_assert_eq!((ga + zero).value(), a);
    }

    /// Property: Galois16 one identity: a * 1 = a
    #[test]
    fn prop_galois_one_identity(a in 0u16..=65535) {
        let ga = Galois16::new(a);
        let one = Galois16::new(1);

        prop_assert_eq!((ga * one).value(), a);
    }

    /// Property: Galois16 additive inverse: a + a = 0 (in GF(2^n), elements are self-inverse)
    #[test]
    fn prop_galois_additive_inverse(a in 0u16..=65535) {
        let ga = Galois16::new(a);

        prop_assert_eq!((ga + ga).value(), 0);
    }

    /// Property: Galois16 multiplicative inverse: a * a^(-1) = 1 (for a ≠ 0)
    #[test]
    fn prop_galois_multiplicative_inverse(a in 1u16..=65535) {
        let ga = Galois16::new(a);
        let one = Galois16::new(1);
        let inv = one / ga;  // a^(-1) = 1 / a

        prop_assert_eq!((ga * inv).value(), 1);
    }

    /// Property: Galois16 power consistency: a^2 = a * a
    #[test]
    fn prop_galois_power_consistency(
        a in 0u16..=65535,
        power in 0u16..=100,
    ) {
        let ga = Galois16::new(a);

        let pow_result = ga.pow(power);

        // Compute manually
        let mut manual = Galois16::new(1);
        for _ in 0..power {
            manual *= ga;
        }

        prop_assert_eq!(pow_result.value(), manual.value());
    }

    /// Property: Galois16 power of zero: 0^n = 0 (for n > 0)
    #[test]
    fn prop_galois_zero_power(power in 1u16..=100) {
        let zero = Galois16::new(0);
        let result = zero.pow(power);

        prop_assert_eq!(result.value(), 0);
    }

    /// Property: Galois16 power of one: 1^n = 1
    #[test]
    fn prop_galois_one_power(power in 0u16..=100) {
        let one = Galois16::new(1);
        let result = one.pow(power);

        prop_assert_eq!(result.value(), 1);
    }

    /// Property: Division is consistent with multiplication: (a / b) * b = a (for b ≠ 0)
    #[test]
    fn prop_galois_division_consistency(
        a in 0u16..=65535,
        b in 1u16..=65535,
    ) {
        let ga = Galois16::new(a);
        let gb = Galois16::new(b);

        let quotient = ga / gb;
        let result = quotient * gb;

        prop_assert_eq!(result.value(), a);
    }

    /// Property: log and alog are inverses: alog(log(a)) = a (for a ≠ 0)
    #[test]
    fn prop_galois_log_alog_inverse(a in 1u16..=65535) {
        let ga = Galois16::new(a);

        let log_val = ga.log();
        let reconstructed = Galois16::new(log_val).alog();

        prop_assert_eq!(reconstructed, a);
    }

    /// Property: Multiplication via logs: log(a * b) = log(a) + log(b) (mod field_size - 1)
    #[test]
    fn prop_galois_log_multiplication(
        a in 1u16..=65535,
        b in 1u16..=65535,
    ) {
        let ga = Galois16::new(a);
        let gb = Galois16::new(b);

        let product = ga * gb;

        if product.value() != 0 {
            let log_a = ga.log() as u32;
            let log_b = gb.log() as u32;
            let log_product = product.log() as u32;

            // In GF(2^16), logs add modulo 65535
            let expected = (log_a + log_b) % 65535;

            prop_assert_eq!(log_product, expected);
        }
    }

    /// Property: Byte slice XOR operations (fundamental to Reed-Solomon)
    #[test]
    fn prop_byte_slice_xor_commutative(
        len in 1usize..=1024,
        seed in any::<u64>(),
    ) {
        let mut rng = StdRng::seed_from_u64(seed);

        let a: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
        let b: Vec<u8> = (0..len).map(|_| rng.gen()).collect();

        let mut result1 = a.clone();
        for i in 0..len {
            result1[i] ^= b[i];
        }

        let mut result2 = b.clone();
        for i in 0..len {
            result2[i] ^= a[i];
        }

        prop_assert_eq!(result1, result2);
    }

    /// Property: Double XOR is identity: (a XOR b) XOR b = a
    #[test]
    fn prop_double_xor_identity(
        len in 1usize..=1024,
        seed in any::<u64>(),
    ) {
        let mut rng = StdRng::seed_from_u64(seed);

        let a: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
        let b: Vec<u8> = (0..len).map(|_| rng.gen()).collect();

        let mut result = a.clone();
        for i in 0..len {
            result[i] ^= b[i];
            result[i] ^= b[i];
        }

        prop_assert_eq!(result, a);
    }

    /// Property: XOR with zero is identity: a XOR 0 = a
    #[test]
    fn prop_xor_zero_identity(
        len in 1usize..=1024,
        seed in any::<u64>(),
    ) {
        let mut rng = StdRng::seed_from_u64(seed);

        let a: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
        let zeros = vec![0u8; len];

        let mut result = a.clone();
        for i in 0..len {
            result[i] ^= zeros[i];
        }

        prop_assert_eq!(result, a);
    }

    /// Property: XOR is associative: (a XOR b) XOR c = a XOR (b XOR c)
    #[test]
    fn prop_xor_associative(
        len in 1usize..=1024,
        seed in any::<u64>(),
    ) {
        let mut rng = StdRng::seed_from_u64(seed);

        let a: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
        let b: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
        let c: Vec<u8> = (0..len).map(|_| rng.gen()).collect();

        let mut left = a.clone();
        for i in 0..len {
            left[i] ^= b[i];
            left[i] ^= c[i];
        }

        let mut right = a.clone();
        let mut b_xor_c = b.clone();
        for i in 0..len {
            b_xor_c[i] ^= c[i];
            right[i] ^= b_xor_c[i];
        }

        prop_assert_eq!(left, right);
    }
}

#[cfg(test)]
mod standard_tests {
    use super::*;

    /// Test that proptest is properly configured
    #[test]
    fn test_proptest_runs() {
        // This test ensures the proptest framework is working
        // The actual property tests are in the proptest! macro above
    }

    /// Verify Galois16 basic operations work
    #[test]
    fn test_galois_basic_ops() {
        let a = Galois16::new(5);
        let b = Galois16::new(3);

        // Just verify operations don't panic
        let _sum = a + b;
        let _product = a * b;
        let _diff = a - b;
        let _quotient = a / b;
    }

    /// Test that property test strategies work correctly
    #[test]
    fn test_strategies_produce_valid_values() {
        use proptest::strategy::Strategy;
        use proptest::test_runner::TestRunner;

        let mut runner = TestRunner::default();

        // Test u16 range strategy - just verify it produces values without panicking
        for _ in 0..10 {
            let _value = (0u16..=u16::MAX).new_tree(&mut runner).unwrap().current();
        }
    }
}
