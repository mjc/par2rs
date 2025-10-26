//! Galois Field GF(2^16) arithmetic for PAR2 Reed-Solomon operations
//!
//! ## Vandermonde Polynomials
//!
//! This module implements 16-bit Galois Field arithmetic using the PAR2 standard
//! **Vandermonde polynomial** (primitive irreducible polynomial):
//!
//! - **GF(2^16)**: 0x1100B (x¹⁶ + x¹² + x³ + x + 1) - for Reed-Solomon encoding/decoding
//!
//! This polynomial is used as the field generator to construct the Vandermonde matrix
//! for Reed-Solomon encoding/decoding. The specific polynomial 0x1100B is mandated by
//! the PAR2 specification and cannot be changed without breaking compatibility.
//!
//! ## Performance
//!
//! Parallel reconstruction with SIMD-optimized multiply-add operations achieve significant
//! speedups over par2cmdline. See `docs/BENCHMARK_RESULTS.md` for cross-platform results.
//!
//! ## Implementation Notes
//!
//! Ported from par2cmdline galois.h implementation with AVX2 SIMD enhancements.
//! Only GF(2^16) is implemented as PAR2 doesn't use other Galois fields.

use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};

/// PAR2 GF(2^16) Vandermonde polynomial: 0x1100B (x¹⁶ + x¹² + x³ + x + 1)
/// Primitive irreducible polynomial used as field generator for Reed-Solomon codes
const GF16_GENERATOR: u32 = 0x1100B;
const BITS: usize = 16;
const COUNT: usize = 1 << BITS;
const LIMIT: usize = COUNT - 1;

/// Galois Field lookup tables for fast arithmetic
pub struct GaloisTable {
    pub log: Vec<u16>,
    pub antilog: Vec<u16>,
}

impl Default for GaloisTable {
    fn default() -> Self {
        Self::new()
    }
}

impl GaloisTable {
    pub fn new() -> Self {
        let mut table = GaloisTable {
            log: vec![0; COUNT],
            antilog: vec![0; COUNT],
        };
        table.build_tables();
        table
    }

    fn build_tables(&mut self) {
        let mut b = 1u32;

        for l in 0..LIMIT {
            self.log[b as usize] = l as u16;
            self.antilog[l] = b as u16;

            b <<= 1;
            if b & COUNT as u32 != 0 {
                b ^= GF16_GENERATOR;
            }
        }

        self.log[0] = LIMIT as u16;
        self.antilog[LIMIT] = 0;
    }
}

/// Galois Field GF(2^16) element
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Galois16 {
    value: u16,
}

impl Galois16 {
    /// Zero constant for compile-time usage
    pub const ZERO: Self = Self { value: 0 };

    /// One constant for compile-time usage  
    pub const ONE: Self = Self { value: 1 };

    /// Create a new Galois16 element
    #[inline]
    pub const fn new(value: u16) -> Self {
        Self { value }
    }

    /// Check if this element is zero
    #[inline]
    pub const fn is_zero(self) -> bool {
        self.value == 0
    }

    /// Checked division that returns None for division by zero
    /// Use this in matrix operations where singular matrices should be detected
    #[inline]
    pub fn checked_div(self, rhs: Self) -> Option<Self> {
        if rhs.value == 0 {
            return None;
        }
        if self.value == 0 {
            return Some(Self::new(0));
        }

        let table = Self::get_table();
        let log_diff = (table.log[self.value as usize] as i32
            - table.log[rhs.value as usize] as i32
            + LIMIT as i32)
            % LIMIT as i32;
        Some(Self::new(table.antilog[log_diff as usize]))
    }

    pub fn value(&self) -> u16 {
        self.value
    }

    /// Power operation
    pub fn pow(&self, exponent: u16) -> Self {
        if self.value == 0 {
            return Self::new(0);
        }

        let table = Self::get_table();
        let log_val = table.log[self.value as usize] as u32;
        let result_log = (log_val * exponent as u32) % LIMIT as u32;
        Self::new(table.antilog[result_log as usize])
    }

    /// Get logarithm value
    pub fn log(&self) -> u16 {
        let table = Self::get_table();
        table.log[self.value as usize]
    }

    /// Get antilogarithm value  
    pub fn antilog(&self) -> u16 {
        let table = Self::get_table();
        table.antilog[self.value as usize]
    }

    /// ALog operation - antilogarithm for base value generation
    /// This is used in par2cmdline for generating database base values
    pub fn alog(&self) -> u16 {
        let table = Self::get_table();
        table.antilog[self.value as usize]
    }

    /// Get the global table (no unsafe needed - direct static initialization)
    fn get_table() -> &'static GaloisTable {
        use std::sync::OnceLock;
        static TABLE: OnceLock<GaloisTable> = OnceLock::new();
        TABLE.get_or_init(GaloisTable::new)
    }
}

// Addition (XOR in Galois fields)
impl Add for Galois16 {
    type Output = Self;

    #[allow(clippy::suspicious_arithmetic_impl)] // XOR is addition in Galois fields
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.value ^ rhs.value)
    }
}

impl AddAssign for Galois16 {
    #[allow(clippy::suspicious_op_assign_impl)] // XOR is addition in Galois fields
    fn add_assign(&mut self, rhs: Self) {
        self.value ^= rhs.value;
    }
}

// Subtraction (same as addition in GF(2^n))
impl Sub for Galois16 {
    type Output = Self;

    #[allow(clippy::suspicious_arithmetic_impl)] // XOR is subtraction in Galois fields
    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.value ^ rhs.value)
    }
}

impl SubAssign for Galois16 {
    #[allow(clippy::suspicious_op_assign_impl)] // XOR is subtraction in Galois fields
    fn sub_assign(&mut self, rhs: Self) {
        self.value ^= rhs.value;
    }
}

// Multiplication using log tables
impl Mul for Galois16 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        if self.value == 0 || rhs.value == 0 {
            return Self::new(0);
        }

        let table = Self::get_table();
        let log_sum = (table.log[self.value as usize] as usize
            + table.log[rhs.value as usize] as usize)
            % LIMIT;
        Self::new(table.antilog[log_sum])
    }
}

impl MulAssign for Galois16 {
    fn mul_assign(&mut self, rhs: Self) {
        *self = *self * rhs;
    }
}

// Division using log tables
impl Div for Galois16 {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        if rhs.value == 0 {
            panic!("Division by zero in Galois field");
        }
        if self.value == 0 {
            return Self::new(0);
        }

        let table = Self::get_table();
        let log_diff = (table.log[self.value as usize] as i32
            - table.log[rhs.value as usize] as i32
            + LIMIT as i32)
            % LIMIT as i32;
        Self::new(table.antilog[log_diff as usize])
    }
}

impl DivAssign for Galois16 {
    fn div_assign(&mut self, rhs: Self) {
        *self = *self / rhs;
    }
}

// Conversion traits
impl From<u16> for Galois16 {
    fn from(value: u16) -> Self {
        Self::new(value)
    }
}

impl From<Galois16> for u16 {
    fn from(val: Galois16) -> Self {
        val.value
    }
}

// Display traits
impl std::fmt::Display for Galois16 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

/// GCD function as used in par2cmdline
pub fn gcd(mut a: u32, mut b: u32) -> u32 {
    if a != 0 && b != 0 {
        while a != 0 && b != 0 {
            if a > b {
                a %= b;
            } else {
                b %= a;
            }
        }
        a + b
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================
    // GaloisTable Tests
    // ========================

    #[test]
    fn galois_table_log_antilog_consistency() {
        let table = GaloisTable::new();

        // For any value x (except 0), antilog[log[x]] should equal x
        for i in 1..COUNT {
            let log_val = table.log[i];
            let recovered = table.antilog[log_val as usize];
            assert_eq!(recovered, i as u16, "Inconsistency at i={}", i);
        }
    }

    #[test]
    fn galois_table_zero_special_case() {
        let table = GaloisTable::new();
        assert_eq!(table.log[0], LIMIT as u16);
        assert_eq!(table.antilog[LIMIT], 0);
    }

    #[test]
    fn galois_table_generator_property() {
        let table = GaloisTable::new();
        // First antilog value should be 1 (identity element)
        assert_eq!(table.antilog[0], 1);
    }

    // ========================
    // Galois16 Basic Operations
    // ========================

    #[test]
    fn galois16_zero_identity() {
        let zero = Galois16::new(0);
        let a = Galois16::new(0x1234);

        // 0 + a = a
        assert_eq!(zero + a, a);
        // a + 0 = a
        assert_eq!(a + zero, a);
        // 0 * a = 0
        assert_eq!(zero * a, zero);
        // a * 0 = 0
        assert_eq!(a * zero, zero);
    }

    #[test]
    fn galois16_one_multiplicative_identity() {
        let one = Galois16::new(1);
        let a = Galois16::new(0x5678);

        // 1 * a = a
        assert_eq!(one * a, a);
        // a * 1 = a
        assert_eq!(a * one, a);
    }

    #[test]
    fn galois16_addition_is_xor() {
        let a = Galois16::new(0x1234);
        let b = Galois16::new(0x5678);

        let sum = a + b;
        assert_eq!(sum.value(), 0x1234 ^ 0x5678);
    }

    #[test]
    fn galois16_addition_commutative() {
        let a = Galois16::new(0xABCD);
        let b = Galois16::new(0x1234);

        assert_eq!(a + b, b + a);
    }

    #[test]
    fn galois16_addition_self_is_zero() {
        let a = Galois16::new(0x1234);

        // a + a = 0 in GF(2^n)
        assert_eq!(a + a, Galois16::new(0));
    }

    #[test]
    fn galois16_subtraction_equals_addition() {
        let a = Galois16::new(0x1234);
        let b = Galois16::new(0x5678);

        // Subtraction and addition are the same in GF(2^n)
        assert_eq!(a + b, a - b);
    }

    #[test]
    fn galois16_multiplication_commutative() {
        let a = Galois16::new(7);
        let b = Galois16::new(13);

        assert_eq!(a * b, b * a);
    }

    #[test]
    fn galois16_multiplication_associative() {
        let a = Galois16::new(3);
        let b = Galois16::new(5);
        let c = Galois16::new(7);

        assert_eq!((a * b) * c, a * (b * c));
    }

    #[test]
    fn galois16_distributive_property() {
        let a = Galois16::new(3);
        let b = Galois16::new(5);
        let c = Galois16::new(7);

        // a * (b + c) = (a * b) + (a * c)
        assert_eq!(a * (b + c), (a * b) + (a * c));
    }

    #[test]
    fn galois16_division_inverse_of_multiplication() {
        let a = Galois16::new(2);
        let b = Galois16::new(3);
        let product = a * b;

        // (a * b) / a == b
        assert_eq!(product / a, b);
        // (a * b) / b == a
        assert_eq!(product / b, a);
    }

    #[test]
    fn galois16_division_by_self_is_one() {
        let a = Galois16::new(0x1234);

        assert_eq!(a / a, Galois16::new(1));
    }

    #[test]
    #[should_panic(expected = "Division by zero")]
    fn galois16_division_by_zero_panics() {
        let a = Galois16::new(5);
        let zero = Galois16::new(0);

        let _ = a / zero;
    }

    #[test]
    fn galois16_checked_division_by_zero_returns_none() {
        let a = Galois16::new(5);
        let zero = Galois16::new(0);
        assert_eq!(a.checked_div(zero), None);
    }

    #[test]
    fn galois16_checked_division_zero_by_nonzero_is_zero() {
        let zero = Galois16::new(0);
        let a = Galois16::new(5);
        assert_eq!(zero.checked_div(a), Some(Galois16::new(0)));
    }

    #[test]
    fn galois16_checked_division_normal_cases() {
        let a = Galois16::new(100);
        let b = Galois16::new(25);
        let result = a.checked_div(b).unwrap();

        // Verify a / b * b == a
        assert_eq!(result * b, a);

        // Test division by self
        assert_eq!(a.checked_div(a), Some(Galois16::new(1)));
    }

    #[test]
    fn galois16_zero_divided_by_nonzero_is_zero() {
        let zero = Galois16::new(0);
        let a = Galois16::new(5);

        assert_eq!(zero / a, zero);
    }

    // ========================
    // Power Tests
    // ========================

    #[test]
    fn galois16_power_zero_exponent() {
        let a = Galois16::new(5);

        // Any number to power 0 should be 1
        assert_eq!(a.pow(0), Galois16::new(1));
    }

    #[test]
    fn galois16_power_one_exponent() {
        let a = Galois16::new(123);

        // Any number to power 1 is itself
        assert_eq!(a.pow(1), a);
    }

    #[test]
    fn galois16_power_two_equals_multiplication() {
        let base = Galois16::new(2);
        let squared = base.pow(2);

        assert_eq!(squared, base * base);
    }

    #[test]
    fn galois16_power_of_zero() {
        let zero = Galois16::new(0);

        // 0 to any power is 0
        assert_eq!(zero.pow(5), zero);
        assert_eq!(zero.pow(100), zero);
    }

    #[test]
    fn galois16_power_properties() {
        let a = Galois16::new(3);

        // a^2 * a^3 = a^5
        assert_eq!(a.pow(2) * a.pow(3), a.pow(5));
    }

    // ========================
    // Log/Antilog Tests
    // ========================

    #[test]
    fn galois16_log_antilog_roundtrip() {
        let a = Galois16::new(42);

        let log_val = a.log();
        let antilog_val = Galois16::new(log_val).antilog();

        assert_eq!(antilog_val, a.value());
    }

    #[test]
    fn galois16_alog_equals_antilog() {
        let a = Galois16::new(10);

        assert_eq!(a.alog(), a.antilog());
    }

    // ========================
    // Assignment Operators
    // ========================

    #[test]
    fn galois16_add_assign() {
        let mut a = Galois16::new(0x1234);
        let b = Galois16::new(0x5678);
        let expected = a + b;

        a += b;
        assert_eq!(a, expected);
    }

    #[test]
    fn galois16_sub_assign() {
        let mut a = Galois16::new(0x1234);
        let b = Galois16::new(0x5678);
        let expected = a - b;

        a -= b;
        assert_eq!(a, expected);
    }

    #[test]
    fn galois16_mul_assign() {
        let mut a = Galois16::new(7);
        let b = Galois16::new(13);
        let expected = a * b;

        a *= b;
        assert_eq!(a, expected);
    }

    #[test]
    fn galois16_div_assign() {
        let mut a = Galois16::new(42);
        let b = Galois16::new(7);
        let expected = a / b;

        a /= b;
        assert_eq!(a, expected);
    }

    // ========================
    // Conversion Traits
    // ========================

    #[test]
    fn galois16_from_u16() {
        let val: Galois16 = 0x1234u16.into();
        assert_eq!(val.value(), 0x1234);
    }

    #[test]
    fn galois16_into_u16() {
        let g = Galois16::new(0x5678);
        let val: u16 = g.into();
        assert_eq!(val, 0x5678);
    }

    #[test]
    fn galois16_display() {
        let g = Galois16::new(12345);
        assert_eq!(format!("{}", g), "12345");
    }

    // ========================
    // GCD Tests
    // ========================

    #[test]
    fn gcd_basic_cases() {
        assert_eq!(gcd(48, 18), 6);
        assert_eq!(gcd(65535, 7), 1);
        assert_eq!(gcd(100, 50), 50);
    }

    #[test]
    fn gcd_coprime_numbers() {
        // 17 and 19 are both prime
        assert_eq!(gcd(17, 19), 1);
        assert_eq!(gcd(65535, 2), 1);
    }

    #[test]
    fn gcd_with_zero() {
        assert_eq!(gcd(0, 5), 0);
        assert_eq!(gcd(5, 0), 0);
        assert_eq!(gcd(0, 0), 0);
    }

    #[test]
    fn gcd_identical_numbers() {
        assert_eq!(gcd(42, 42), 42);
        assert_eq!(gcd(1, 1), 1);
        assert_eq!(gcd(65535, 65535), 65535);
    }

    #[test]
    fn gcd_commutative() {
        assert_eq!(gcd(48, 18), gcd(18, 48));
        assert_eq!(gcd(100, 35), gcd(35, 100));
    }

    #[test]
    fn gcd_with_one() {
        assert_eq!(gcd(1, 100), 1);
        assert_eq!(gcd(12345, 1), 1);
    }

    #[test]
    fn gcd_powers_of_two() {
        assert_eq!(gcd(16, 64), 16);
        assert_eq!(gcd(128, 32), 32);
    }
}
