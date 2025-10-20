//! par2rs - High-performance PAR2 file repair and verification
//!
//! ## Performance
//!
//! Parallel Reed-Solomon reconstruction and SIMD-optimized operations achieve:
//! - **1.93x faster** than par2cmdline (0.506s vs 0.980s for 100MB file repair)
//! - **2.90x faster** for 1GB files (4.704s vs 13.679s)
//! - **2.00x faster** for 10GB files (57.243s vs 114.526s)
//!
//! See `docs/SIMD_OPTIMIZATION.md` for detailed benchmarks and implementation notes.
//!
//! ## Reed-Solomon Implementation
//!
//! Uses Vandermonde polynomial 0x1100B (x¹⁶ + x¹² + x³ + x + 1) for GF(2^16) operations,
//! as mandated by the PAR2 specification for cross-compatibility with other PAR2 clients.

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
