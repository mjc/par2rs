//! par2rs - High-performance PAR2 file repair and verification
//!
//! This library provides the core functionality for PAR2 (Parity Archive) operations.
//! It is primarily used by the `par2verify` and `par2repair` command-line tools.
//!
//! ## Performance
//!
//! Parallel Reed-Solomon reconstruction and SIMD-optimized operations (PSHUFB on x86_64,
//! NEON on ARM64, portable_simd cross-platform) achieve significant speedups over par2cmdline.
//!
//! See `docs/BENCHMARK_RESULTS.md` and `docs/SIMD_OPTIMIZATION.md` for details.

#![feature(portable_simd)]
#![allow(clippy::double_parens)] // binrw macro generates double parens

// Core modules used by binaries
pub mod analysis;
pub mod args;
pub mod par2_files;
pub mod repair;
pub mod verify;

// Internal modules (exposed but not typically used directly by binaries)
pub mod checksum;
pub mod domain;
pub mod packets;
pub mod reporters;

// Reed-Solomon is internal implementation detail, but exposed for advanced use
pub mod reed_solomon;

// Re-export commonly used types for convenience (used internally and by binaries)
pub use args::parse_args;
pub use packets::{
    parse_packets, InputFileSliceChecksumPacket, Packet, RecoverySliceMetadata, RecoverySlicePacket,
};
