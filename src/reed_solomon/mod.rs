//! Reed-Solomon Error Correction Module
//!
//! ## Overview
//!
//! This module provides Reed-Solomon error correction functionality for PAR2 repair operations
//! using the Vandermonde polynomial 0x1100B (x¹⁶ + x¹² + x³ + x + 1) for GF(2^16).
//!
//! ## Performance
//!
//! Parallel reconstruction with SIMD-optimized multiply-add operations (PSHUFB on x86_64,
//! NEON on ARM64, portable_simd cross-platform) achieves significant speedups over par2cmdline.
//!
//! See `docs/SIMD_OPTIMIZATION.md` for detailed benchmarks and implementation notes.
//!
//! ## Compatibility
//!
//! Implementation ported from par2cmdline to ensure compatibility with the PAR2 specification.
//! The specific Vandermonde polynomial is mandated by the PAR2 spec and cannot be changed.

pub mod galois;
pub mod reedsolomon;
pub mod simd;

pub use galois::*;
pub use reedsolomon::*;
pub use simd::*;
