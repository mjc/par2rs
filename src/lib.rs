//! par2rs - High-performance PAR2 file repair and verification
//!
//! ## Performance
//!
//! Parallel Reed-Solomon reconstruction and SIMD-optimized operations (PSHUFB on x86_64,
//! NEON on ARM64, portable_simd cross-platform) achieve significant speedups over par2cmdline.
//!
//! See `docs/BENCHMARK_RESULTS.md` for cross-platform performance results and
//! `docs/SIMD_OPTIMIZATION.md` for technical implementation details.
//!
//! ## Reed-Solomon Implementation
//!
//! Uses Vandermonde polynomial 0x1100B (x¹⁶ + x¹² + x³ + x + 1) for GF(2^16) operations,
//! as mandated by the PAR2 specification for cross-compatibility with other PAR2 clients.

#![feature(portable_simd)]

pub mod analysis;
pub mod args;
pub mod domain;
pub mod file_ops;
pub mod file_verification;
pub mod packets;
pub mod recovery_loader;
pub mod reed_solomon;
pub mod repair;
pub mod slice_provider;
pub mod validation;
pub mod verify;

pub use args::parse_args;
pub use packets::*; // Add this line to import all public items from packets module
pub use recovery_loader::{FileSystemLoader, RecoveryDataLoader};
