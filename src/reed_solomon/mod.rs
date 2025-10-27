//! Reed-Solomon Error Correction Module
//!
//! ## Overview
//!
//! This module provides Reed-Solomon error correction functionality for PAR2 repair operations
//! using the Vandermonde polynomial 0x1100B (x¹⁶ + x¹² + x³ + x + 1) for GF(2^16).
//!
//! ## Reed-Solomon Implementations
//!
//! ### Modern Type-Safe API (Recommended)
//! - `ReedSolomon<DATA_BLOCKS, RECOVERY_BLOCKS>` - Const generic dimensions, compile-time safety
//! - `ReedSolomonBuilder` - Enforces required fields at compile time
//! - `Matrix<ROWS, COLS>` - Type-safe matrices with bounds checking
//! - Zero runtime overhead, maximum compile-time safety
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

pub mod codec;
pub mod galois;
#[doc(hidden)]
pub mod simd; // Public for tests/benchmarks, but hidden from docs

pub mod builder;
pub mod matrix; // Type-safe matrices with const generic dimensions // Compile-time validated Reed-Solomon builder

pub use galois::*;

// Re-export type-safe matrix operations
pub use matrix::{
    AlignedChunkSize, ColIndex, Matrix, NonZeroGalois16, RecoveryConfig, RowIndex, SliceLength,
};

// Modern type-safe Reed-Solomon (preferred for new code)
pub use builder::{HighRedundancy, MinimalConfig, ReedSolomon, ReedSolomonBuilder, StandardPar2};

// Re-export core error types and internal components needed by other modules
pub use codec::{build_split_mul_table, ReconstructionEngine, RsError, RsResult, SplitMulTable};

// Don't re-export simd - users shouldn't need direct SIMD access
