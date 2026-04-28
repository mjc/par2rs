//! MD5x2 NEON backend for ARM64 (placeholder for future porting).
//!
//! This module will port the NEON MD5x2 implementation from
//! par2cmdline-turbo/ParPar when resources allow. For now, it's a stub
//! that documents the plan and reserves module space.
//!
//! Upstream sources:
//! - parpar/hasher/md5x2-neon.h (110 SLOC)
//! - parpar/hasher/md5x2-neon-asm.h (271 SLOC, AArch64 inline asm)
//!
//! ## Plan
//! 1. Port using NEON intrinsics from `core::arch::aarch64` (not asm!)
//! 2. Implement `Md5x2` trait with parallel lane tracking
//! 3. Add comprehensive correctness tests vs Scalar backend
//! 4. Benchmark on real ARM64 hardware (GitHub Actions or AWS Graviton)
//! 5. Only switch to `asm!` blocks if benchmarks show intrinsics are slow

#![cfg(target_arch = "aarch64")]

use super::md5x2::Md5x2;

/// NEON MD5x2 backend for ARM64 (placeholder).
pub struct Neon;

impl Md5x2 for Neon {
    type State = [u32; 8];
    const USE_AVX512_CRC: bool = false; // NEON doesn't have vpternlogd; uses ARM CRC instructions instead

    fn init_lane(state: &mut Self::State, idx: usize) {
        // TODO: Port from upstream md5_init_lane_x2_neon macro
        unimplemented!("md5x2_neon not yet ported")
    }

    unsafe fn process_block(state: &mut Self::State, data: [*const u8; 2]) {
        // TODO: Port from upstream md5_process_block_x2_neon asm
        let _ = (state, data);
        unimplemented!("md5x2_neon not yet ported")
    }

    fn extract(state: &Self::State, idx: usize) -> [u8; 16] {
        // TODO: Port from upstream md5_extract_x2_neon macro
        let _ = (state, idx);
        unimplemented!("md5x2_neon not yet ported")
    }
}
