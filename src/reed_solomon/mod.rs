//! Reed-Solomon Error Correction Module
//!
//! ## Overview
//!
//! This module provides Reed-Solomon error correction functionality for PAR2 repair operations
//! using the Vandermonde polynomial 0x1100B (x¹⁶ + x¹² + x³ + x + 1) for GF(2^16).
//!
//! ## Performance
//!
//! Parallel reconstruction with AVX2 PSHUFB-optimized multiply-add achieves:
//! - **1.93x faster** than par2cmdline for 100MB files (0.506s vs 0.980s)
//! - **2.90x faster** for 1GB files (4.704s vs 13.679s)
//! - **2.00x faster** for 10GB files (57.243s vs 114.526s)
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
pub mod simd_pshufb;

pub use galois::*;
pub use reedsolomon::*;
pub use simd::*;
