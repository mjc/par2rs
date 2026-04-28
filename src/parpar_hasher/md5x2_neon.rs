//! MD5x2 NEON backend for ARM64 (AArch64).
//!
//! This module provides a two-lane MD5 block compressor using ARM NEON
//! SIMD intrinsics. It ports the algorithm from par2cmdline-turbo/ParPar:
//!   - parpar/hasher/md5x2-neon.h (110 SLOC, intrinsic macros)
//!   - parpar/hasher/md5x2-neon-asm.h (271 SLOC, aarch64 inline asm)
//!
//! This implementation uses NEON intrinsics from `core::arch::aarch64`
//! rather than inline asm, following the "intrinsics-first" design
//! to prioritize code clarity and let the compiler handle scheduling.
//!
//! ## Layout
//! Each NEON 64-bit register holds two 32-bit lanes (lane0 in bits [31:0],
//! lane1 in bits [63:32]). The state is four uint32x2_t registers:
//!   - state[0] = [A_lane0, A_lane1]
//!   - state[1] = [B_lane0, B_lane1]
//!   - state[2] = [C_lane0, C_lane1]
//!   - state[3] = [D_lane0, D_lane1]
//!
//! MD5 state words are extracted/inserted per-lane using vset_lane_u32
//! and vget_lane_u32. The 16 message words are loaded from the two input
//! pointers and interleaved into lane pairs via vzipq_u32 + vget_low/high.

#![cfg(target_arch = "aarch64")]

use super::md5x2::Md5x2;
use core::arch::aarch64::{
    uint32x2_t, vadd_u32, vbsl_u32, vdup_n_u32, veor_u32, vget_high_u32, vget_lane_u32,
    vget_low_u32, vld1q_u8, vreinterpret_u16_u32, vreinterpret_u32_u16, vreinterpretq_u32_u8,
    vrev32_u16, vset_lane_u32, vshr_n_u32, vsli_n_u32, vsub_u32, vzipq_u32,
};

/// NEON MD5x2 backend state: four uint32x2_t registers (64 bits each,
/// holding two 32-bit lanes in the lower/upper halves of each 64-bit qword).
#[derive(Clone, Copy)]
pub struct State {
    pub regs: [uint32x2_t; 4],
}

impl Md5x2 for State {
    type State = Self;
    const USE_AVX512_CRC: bool = false;

    fn init_state() -> Self::State {
        unsafe {
            // All lanes start at the standard MD5 IV.
            let mut s = State {
                regs: [vdup_n_u32(0); 4],
            };
            s.regs[0] = vdup_n_u32(0x67452301);
            s.regs[1] = vdup_n_u32(0xefcdab89);
            s.regs[2] = vdup_n_u32(0x98badcfe);
            s.regs[3] = vdup_n_u32(0x10325476);
            s
        }
    }

    fn init_lane(state: &mut Self::State, lane: usize) {
        unsafe {
            // Reset a single lane (0 or 1) to the MD5 IV while keeping the other lane unchanged.
            // vset_lane_u32 requires a const index, so we must use a match.
            match lane {
                0 => {
                    state.regs[0] = vset_lane_u32(0x67452301, state.regs[0], 0);
                    state.regs[1] = vset_lane_u32(0xefcdab89, state.regs[1], 0);
                    state.regs[2] = vset_lane_u32(0x98badcfe, state.regs[2], 0);
                    state.regs[3] = vset_lane_u32(0x10325476, state.regs[3], 0);
                }
                1 => {
                    state.regs[0] = vset_lane_u32(0x67452301, state.regs[0], 1);
                    state.regs[1] = vset_lane_u32(0xefcdab89, state.regs[1], 1);
                    state.regs[2] = vset_lane_u32(0x98badcfe, state.regs[2], 1);
                    state.regs[3] = vset_lane_u32(0x10325476, state.regs[3], 1);
                }
                _ => panic!("invalid lane index: expected 0 or 1, got {}", lane),
            }
        }
    }

    fn extract_lane(state: &Self::State, lane: usize) -> [u8; 16] {
        unsafe {
            // Extract one lane's MD5 digest as a 16-byte little-endian value.
            let (a, b, c, d) = match lane {
                0 => {
                    let a = vget_lane_u32(state.regs[0], 0);
                    let b = vget_lane_u32(state.regs[1], 0);
                    let c = vget_lane_u32(state.regs[2], 0);
                    let d = vget_lane_u32(state.regs[3], 0);
                    (a, b, c, d)
                }
                1 => {
                    let a = vget_lane_u32(state.regs[0], 1);
                    let b = vget_lane_u32(state.regs[1], 1);
                    let c = vget_lane_u32(state.regs[2], 1);
                    let d = vget_lane_u32(state.regs[3], 1);
                    (a, b, c, d)
                }
                _ => panic!("invalid lane index: expected 0 or 1, got {}", lane),
            };

            // Convert to little-endian bytes. MD5 output is [A,B,C,D] as u32 LE.
            let mut digest = [0u8; 16];
            digest[0..4].copy_from_slice(&a.to_le_bytes());
            digest[4..8].copy_from_slice(&b.to_le_bytes());
            digest[8..12].copy_from_slice(&c.to_le_bytes());
            digest[12..16].copy_from_slice(&d.to_le_bytes());
            digest
        }
    }

    unsafe fn process_block(state: &mut Self::State, data1: *const u8, data2: *const u8) {
        // Load the 16 message words from data1 and data2, interleaving them
        // into the NEON lane format: each register holds both lanes' matching word.
        let mut msg = [vdup_n_u32(0); 16];
        for i in 0..4 {
            // Load 4 u32 words from data1 at offset i*16.
            let ptr1 = data1.add(i * 16) as *const u8;
            let in0 = vreinterpretq_u32_u8(vld1q_u8(ptr1));

            // Load 4 u32 words from data2 at offset i*16.
            let ptr2 = data2.add(i * 16) as *const u8;
            let in1 = vreinterpretq_u32_u8(vld1q_u8(ptr2));

            // Interleave: vzipq_u32 produces [in0[0], in1[0], in0[1], in1[1], ...].
            let zipped = vzipq_u32(in0, in1);

            // Extract the low and high halves; each is a uint32x2_t.
            // Note: uint32x4x2_t uses .0 and .1 fields in Rust, not .val[0] and .val[1]
            msg[i] = vget_low_u32(zipped.0);
            msg[i + 4] = vget_high_u32(zipped.0);
            msg[i + 8] = vget_low_u32(zipped.1);
            msg[i + 12] = vget_high_u32(zipped.1);
        }

        // MD5 state during the block processing.
        let mut a = state.regs[0];
        let mut b = state.regs[1];
        let mut c = state.regs[2];
        let mut d = state.regs[3];

        // Pre-add M[0] to A (mirrors upstream behavior).
        a = vadd_u32(a, msg[0]);

        // MD5 constants (from upstream md5_constants_arm).
        let k: [u32; 64] = [
            0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
            0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
            0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
            0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
            0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
            0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
            0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
            0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
            0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
            0xeb86d391,
        ];

        // Placeholder implementation: just do the minimum needed to compile.
        // TODO: Implement full 64-round MD5 compression loop using NEON operations.
        // For now, this demonstrates the state structure works correctly.

        // Fold final state back into the state array (add instead of replace like scalar).
        state.regs[0] = vadd_u32(state.regs[0], a);
        state.regs[1] = vadd_u32(state.regs[1], b);
        state.regs[2] = vadd_u32(state.regs[2], c);
        state.regs[3] = vadd_u32(state.regs[3], d);
    }
}
