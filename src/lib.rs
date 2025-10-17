//! par2rs - High-performance PAR2 file repair and verification
//!
//! ## Performance
//!
//! SIMD-optimized Reed-Solomon operations using AVX2 PSHUFB achieve:
//! - **1.66x faster** than par2cmdline (0.607s vs 1.008s for 100MB file repair)
//! - **2.76x speedup** in GF(2^16) multiply-add operations
//!
//! See `docs/SIMD_OPTIMIZATION.md` for detailed benchmarks and implementation notes.
//!
//! ## Reed-Solomon Implementation
//!
//! Uses Vandermonde polynomial 0x1100B (x¹⁶ + x¹² + x³ + x + 1) for GF(2^16) operations,
//! as mandated by the PAR2 specification for cross-compatibility with other PAR2 clients.

pub mod analysis;
pub mod args;
pub mod file_ops;
pub mod file_verification;
pub mod packets;
pub mod recovery_loader;
pub mod reed_solomon;
pub mod repair;
pub mod slice_provider;
pub mod verify;

pub use args::parse_args;
pub use packets::*; // Add this line to import all public items from packets module
pub use recovery_loader::{RecoveryDataLoader, FileSystemLoader};
