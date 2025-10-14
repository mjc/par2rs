//! Reed-Solomon Error Correction Module
//!
//! ## Overview
//!
//! This module provides Reed-Solomon error correction functionality for PAR2 repair operations
//! using the Vandermonde polynomial 0x1100B (x¹⁶ + x¹² + x³ + x + 1) for GF(2^16).
//!
//! ## Performance
//!
//! AVX2 PSHUFB-optimized multiply-add achieves:
//! - **2.76x speedup** over scalar code (54.7ns vs 150.9ns per block)
//! - **1.66x faster** than par2cmdline in real-world repair (0.607s vs 1.008s for 100MB)
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
