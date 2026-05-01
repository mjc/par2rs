//! ARM CRC32 backend for aarch64 (placeholder for future porting).
//!
//! This module will port the ARMv8 CRC32 implementation from
//! par2cmdline-turbo/ParPar when resources allow. For now, it's a stub.
//!
//! Upstream source: parpar/hasher/crc_arm.h (61 SLOC)
//!
//! ## Plan
//! Uses ARMv8 CRC32 instructions:
//! - `__crc32d` — 64-bit CRC update (aarch64)
//! - `__crc32w` — 32-bit CRC update (both architectures)
//! - `__crc32h` — 16-bit CRC update
//! - `__crc32b` — 8-bit CRC update
//!
//! These are available via `std::arch::aarch64` intrinsics and require
//! the ARMv8 CRC feature (`__ARM_FEATURE_CRC32`).

#![cfg(target_arch = "aarch64")]

/// CRC32 backend using ARMv8 CRC instructions (placeholder).
pub struct ArmCrc;

// TODO: Implement when porting is ready
//
// pub struct State {
//     crc: u32,
// }
//
// impl ArmCrc {
//     pub fn init() -> State { ... }
//     pub fn process_block(&mut self, src: &[u8; 64]) { ... }
//     pub fn finish(&self, tail: &[u8]) -> u32 { ... }
// }
