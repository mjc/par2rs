//! Reed-Solomon Error Correction Module
//!
//! This module provides Reed-Solomon error correction functionality for PAR2 repair operations.
//! PAR2 uses Galois Field GF(2^16) for Reed-Solomon calculations.

pub mod par2_reed_solomon;
pub mod reconstruction;
pub mod types;

pub use par2_reed_solomon::*;
pub use reconstruction::*;
pub use types::*;
