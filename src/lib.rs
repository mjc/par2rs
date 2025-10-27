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
//!
//! ## Type-Safe Reed-Solomon Usage
//!
//! The modern API provides compile-time safety and zero-cost abstractions:
//!
//! ```rust
//! use par2rs::reed_solomon::{ReedSolomonBuilder, AlignedChunkSize};
//!
//! // Compile-time validated configuration
//! let mut rs = ReedSolomonBuilder::new()
//!     .with_data_blocks::<10>()      // 10 data blocks
//!     .with_recovery_blocks::<5>()   // 5 recovery blocks  
//!     .finalize()
//!     .build();
//!
//! // Set block availability (const generic bounds checking)
//! rs.set_input_present(0, false)?;   // Missing data block 0
//! rs.set_recovery_present(0, true)?; // Recovery block 0 available
//!
//! // Compute generator matrix
//! rs.compute()?;
//!
//! // Process SIMD-aligned chunks (compile-time alignment validation)
//! let input = [0u8; 64];
//! let mut output = [0u8; 64];
//! let alignment = AlignedChunkSize::<64>::new();
//!
//! rs.process_aligned_chunk(0, &input, 0, &mut output, alignment)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![feature(portable_simd)]
#![allow(clippy::double_parens)] // binrw macro generates double parens

pub mod analysis;
pub mod args;
pub mod domain;
pub mod file_verification;
pub mod md5_writer;
pub mod packets;
pub mod par2_files;
pub mod recovery_loader;
pub mod reed_solomon;
pub mod repair;
pub mod slice_provider;
pub mod validation;
pub mod verify;

pub use args::parse_args;
pub use packets::*;
pub use recovery_loader::{FileSystemLoader, RecoveryDataLoader};
