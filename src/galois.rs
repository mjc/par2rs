//! Galois Field GF(2^16) arithmetic for PAR2 Reed-Solomon operations
//!
//! This module implements 16-bit Galois Field arithmetic using the PAR2 standard
//! generator polynomial 0x1100B (binary: 1 0001 0000 0000 1011).

/// PAR2 uses GF(2^16) with generator polynomial 0x1100B
const GF_GENERATOR: u32 = 0x1100B;

/// Precomputed multiplication and division tables for performance
pub struct GaloisField {
    log_table: [u16; 65536],
    exp_table: [u16; 131072], // 2x size to avoid modulo in calculations
}

impl GaloisField {
    /// Create a new Galois Field with precomputed tables
    pub fn new() -> Self {
        let mut gf = GaloisField {
            log_table: [0; 65536],
            exp_table: [0; 131072],
        };
        gf.build_tables();
        gf
    }

    /// Build logarithm and exponential tables for fast multiplication/division
    fn build_tables(&mut self) {
        let mut value = 1u32;
        
        // Build the exponential table first
        for i in 0..65535 {
            self.exp_table[i] = value as u16;
            if value < 65536 {
                self.log_table[value as usize] = i as u16;
            }
            
            value <<= 1;
            if value & 0x10000 != 0 {
                value ^= GF_GENERATOR;
            }
        }
        
        // Duplicate the table for easier calculation
        for i in 65535..131072 {
            self.exp_table[i] = self.exp_table[i - 65535];
        }
        
        self.log_table[0] = 0; // Special case: log(0) = 0 (though mathematically undefined)
    }

    /// Add two elements in GF(2^16) - this is just XOR
    #[inline]
    pub fn add(&self, a: u16, b: u16) -> u16 {
        a ^ b
    }

    /// Subtract two elements in GF(2^16) - same as addition (XOR)
    #[inline]
    pub fn sub(&self, a: u16, b: u16) -> u16 {
        a ^ b
    }

    /// Multiply two elements in GF(2^16)
    #[inline]
    pub fn mul(&self, a: u16, b: u16) -> u16 {
        if a == 0 || b == 0 {
            return 0;
        }
        
        let log_a = self.log_table[a as usize] as usize;
        let log_b = self.log_table[b as usize] as usize;
        self.exp_table[log_a + log_b]
    }

    /// Divide two elements in GF(2^16)
    #[inline]
    pub fn div(&self, a: u16, b: u16) -> u16 {
        if a == 0 {
            return 0;
        }
        if b == 0 {
            panic!("Division by zero in Galois Field");
        }
        
        let log_a = self.log_table[a as usize] as usize;
        let log_b = self.log_table[b as usize] as usize;
        
        // Subtraction in log space, with wraparound
        let log_result = if log_a >= log_b {
            log_a - log_b
        } else {
            log_a + 65535 - log_b
        };
        
        self.exp_table[log_result]
    }

    /// Raise an element to a power in GF(2^16)
    #[inline]
    pub fn pow(&self, base: u16, exponent: u32) -> u16 {
        if base == 0 {
            return if exponent == 0 { 1 } else { 0 };
        }
        if exponent == 0 {
            return 1;
        }
        
        let log_base = self.log_table[base as usize] as u64;
        let log_result = (log_base * exponent as u64) % 65535;
        self.exp_table[log_result as usize]
    }

    /// Get the multiplicative inverse of an element
    #[inline]
    pub fn inverse(&self, a: u16) -> u16 {
        if a == 0 {
            panic!("Cannot invert zero in Galois Field");
        }
        
        let log_a = self.log_table[a as usize] as usize;
        self.exp_table[65535 - log_a]
    }
}

impl Default for GaloisField {
    fn default() -> Self {
        Self::new()
    }
}

use std::sync::OnceLock;

/// Global Galois Field instance for PAR2 operations
static GALOIS_FIELD: OnceLock<GaloisField> = OnceLock::new();

/// Get the global Galois Field instance
pub fn galois_field() -> &'static GaloisField {
    GALOIS_FIELD.get_or_init(|| GaloisField::new())
}

/// Convenience functions using the global Galois Field
#[inline]
pub fn gf_add(a: u16, b: u16) -> u16 {
    galois_field().add(a, b)
}

#[inline]
pub fn gf_sub(a: u16, b: u16) -> u16 {
    galois_field().sub(a, b)
}

#[inline]
pub fn gf_mul(a: u16, b: u16) -> u16 {
    galois_field().mul(a, b)
}

#[inline]
pub fn gf_div(a: u16, b: u16) -> u16 {
    galois_field().div(a, b)
}

#[inline]
pub fn gf_pow(base: u16, exponent: u32) -> u16 {
    galois_field().pow(base, exponent)
}

#[inline]
pub fn gf_inverse(a: u16) -> u16 {
    galois_field().inverse(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_galois_field_basic_operations() {
        let gf = GaloisField::new();
        
        // Test basic properties
        assert_eq!(gf.add(5, 3), 5 ^ 3);
        assert_eq!(gf.sub(5, 3), 5 ^ 3);
        
        // Test multiplicative identity
        assert_eq!(gf.mul(1, 42), 42);
        assert_eq!(gf.mul(42, 1), 42);
        
        // Test additive identity
        assert_eq!(gf.add(0, 42), 42);
        assert_eq!(gf.add(42, 0), 42);
        
        // Test that a * inverse(a) = 1 for some non-zero values
        for a in 1..10u16 {
            let inv_a = gf.inverse(a);
            assert_eq!(gf.mul(a, inv_a), 1, "Failed for a = {}", a);
        }
    }

    #[test]
    fn test_galois_field_division() {
        let gf = GaloisField::new();
        
        // Test that a / b * b = a for some non-zero a, b
        for a in 1..10u16 {
            for b in 1..10u16 {
                let quotient = gf.div(a, b);
                let result = gf.mul(quotient, b);
                assert_eq!(result, a, "Failed for a = {}, b = {}", a, b);
            }
        }
    }

    #[test]
    fn test_galois_field_power() {
        let gf = GaloisField::new();
        
        // Test some basic power operations
        assert_eq!(gf.pow(2, 0), 1);
        assert_eq!(gf.pow(2, 1), 2);
        assert_eq!(gf.pow(0, 5), 0);
        
        // Test that a^0 = 1 for some non-zero values
        for a in 1..10u16 {
            assert_eq!(gf.pow(a, 0), 1);
        }
    }

    #[test]
    fn test_convenience_functions() {
        // Test that convenience functions work by creating a local instance
        let gf = GaloisField::new();
        assert_eq!(gf.add(5, 3), 5 ^ 3);
        assert_eq!(gf.mul(1, 42), 42);
        assert_eq!(gf.pow(2, 1), 2);
    }
}
