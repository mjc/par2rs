//! Reed-Solomon Error Correction Module
//!
//! This module provides Reed-Solomon error correction functionality for PAR2 repair operations.
//! PAR2 uses Galois Field GF(2^16) for Reed-Solomon calculations.
//!
//! This implementation is ported from par2cmdline to ensure compatibility with the PAR2 specification.

pub mod galois;
pub mod reedsolomon;
pub mod simd;

pub use galois::*;
pub use reedsolomon::*;
pub use simd::*;
