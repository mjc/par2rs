//! MD5x2 NEON backend for ARM64 (AArch64).
//!
//! This module provides a two-lane MD5 block compressor using ARM NEON
//! SIMD intrinsics. It ports the algorithm from par2cmdline-turbo/ParPar:
//!   - parpar/hasher/md5x2-neon.h (intrinsic macro definitions)
//!   - parpar/hasher/md5-base.h (64-round expansion)
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
//! ## Round macro
//! `rx!(TYPE, a, b, c, d, msg_word, K_const, R, 32-R)` computes one MD5
//! step: `a = rotate_left(a + msg + K + TYPE(b,c,d), R) + b`.
//! The rotation is split into `vsli_n_u32::<R>(vshr_n_u32::<32-R>(x), x)`
//! using literal const generics so the compiler emits a single SLI/SHR pair.
//!
//! ## Message loading
//! 16 words from `data1` and `data2` are interleaved into `msg[0..16]`
//! via four `vzipq_u32` calls: msg[i] = uint32x2_t{data1[i], data2[i]}.
//! On LE aarch64, `vld1q_u8 + vreinterpretq_u32_u8` gives native-endian
//! u32 words, matching MD5's LE word convention.

#![cfg(target_arch = "aarch64")]

use super::md5x2::Md5x2;
use core::arch::aarch64::{
    uint32x2_t, vadd_u32, vbsl_u32, vdup_n_u32, veor_u32, vget_high_u32, vget_lane_u32,
    vget_low_u32, vld1q_u8, vorn_u32, vreinterpretq_u32_u8, vset_lane_u32, vshr_n_u32, vsli_n_u32,
    vzipq_u32,
};

// One MD5 round step. Each arm computes:
//   a = rotate_left(a + msg_word + K_const + ROUND_FUNC(b, c, d), R) + b
// Round functions (Reference: par2cmdline-turbo/parpar/hasher/md5x2-neon.h):
//   F(b,c,d) = (b & c) | (~b & d) = vbsl_u32(b, c, d)
//   G(b,c,d) = (d & b) | (~d & c) = vbsl_u32(d, b, c)
//   H(b,c,d) = b ^ c ^ d          = veor_u32(veor_u32(b,c),d)
//   I(b,c,d) = c ^ (b | ~d)       = veor_u32(c, vorn_u32(b, d))
//
// The rotation `rotate_left(x, R)` expands to:
//   vsli_n_u32::<R>(vshr_n_u32::<{32-R}>(x), x)
// Both shift amounts must be compile-time literals; callers pass both
// R and 32-R explicitly since generic_const_exprs is not stable.
macro_rules! rx {
    (F, $a:ident, $b:ident, $c:ident, $d:ident, $m:expr, $k:literal, $r:literal, $rr:literal) => {{
        let mk = vadd_u32($m, vdup_n_u32($k));
        let sum = vadd_u32(vadd_u32($a, mk), vbsl_u32($b, $c, $d));
        $a = vadd_u32(vsli_n_u32::<$r>(vshr_n_u32::<$rr>(sum), sum), $b);
    }};
    (G, $a:ident, $b:ident, $c:ident, $d:ident, $m:expr, $k:literal, $r:literal, $rr:literal) => {{
        let mk = vadd_u32($m, vdup_n_u32($k));
        let sum = vadd_u32(vadd_u32($a, mk), vbsl_u32($d, $b, $c));
        $a = vadd_u32(vsli_n_u32::<$r>(vshr_n_u32::<$rr>(sum), sum), $b);
    }};
    (H, $a:ident, $b:ident, $c:ident, $d:ident, $m:expr, $k:literal, $r:literal, $rr:literal) => {{
        let mk = vadd_u32($m, vdup_n_u32($k));
        let sum = vadd_u32(vadd_u32($a, mk), veor_u32(veor_u32($b, $c), $d));
        $a = vadd_u32(vsli_n_u32::<$r>(vshr_n_u32::<$rr>(sum), sum), $b);
    }};
    (I, $a:ident, $b:ident, $c:ident, $d:ident, $m:expr, $k:literal, $r:literal, $rr:literal) => {{
        let mk = vadd_u32($m, vdup_n_u32($k));
        let sum = vadd_u32(vadd_u32($a, mk), veor_u32($c, vorn_u32($b, $d)));
        $a = vadd_u32(vsli_n_u32::<$r>(vshr_n_u32::<$rr>(sum), sum), $b);
    }};
}

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
        // Load 16 message words from both buffers, interleaving lane0 (data1)
        // and lane1 (data2) so that msg[i] = {data1_word_i, data2_word_i}.
        //
        // vzipq_u32([a0,a1,a2,a3], [b0,b1,b2,b3]) → ([a0,b0,a1,b1], [a2,b2,a3,b3])
        // vget_low/high extract the two uint32x2_t halves.
        // Reference: par2cmdline-turbo/parpar/hasher/md5x2-neon.h LOAD4 macro.
        let mut msg = [vdup_n_u32(0u32); 16];
        for i in 0..4usize {
            let in0 = vreinterpretq_u32_u8(vld1q_u8(data1.add(i * 16)));
            let in1 = vreinterpretq_u32_u8(vld1q_u8(data2.add(i * 16)));
            let zipped = vzipq_u32(in0, in1);
            msg[i * 4] = vget_low_u32(zipped.0); // [data1[4i+0], data2[4i+0]]
            msg[i * 4 + 1] = vget_high_u32(zipped.0); // [data1[4i+1], data2[4i+1]]
            msg[i * 4 + 2] = vget_low_u32(zipped.1); // [data1[4i+2], data2[4i+2]]
            msg[i * 4 + 3] = vget_high_u32(zipped.1); // [data1[4i+3], data2[4i+3]]
        }

        // Save initial state for the Davies-Meyer feed-forward at the end.
        let oa = state.regs[0];
        let ob = state.regs[1];
        let oc = state.regs[2];
        let od = state.regs[3];
        let mut a = oa;
        let mut b = ob;
        let mut c = oc;
        let mut d = od;

        // 64 MD5 rounds, fully unrolled.
        // Reference: par2cmdline-turbo/parpar/hasher/md5-base.h (64 RX() calls).
        // K constants from RFC 1321 §3.4; rotation amounts per RFC 1321 §3.4.
        //
        // rx!(TYPE, accumulator, b, c, d, msg_word, K, R, 32-R)

        // Round 0-15 (F)
        rx!(F, a, b, c, d, msg[0], 0xd76aa478u32, 7, 25);
        rx!(F, d, a, b, c, msg[1], 0xe8c7b756u32, 12, 20);
        rx!(F, c, d, a, b, msg[2], 0x242070dbu32, 17, 15);
        rx!(F, b, c, d, a, msg[3], 0xc1bdceeeu32, 22, 10);
        rx!(F, a, b, c, d, msg[4], 0xf57c0fafu32, 7, 25);
        rx!(F, d, a, b, c, msg[5], 0x4787c62au32, 12, 20);
        rx!(F, c, d, a, b, msg[6], 0xa8304613u32, 17, 15);
        rx!(F, b, c, d, a, msg[7], 0xfd469501u32, 22, 10);
        rx!(F, a, b, c, d, msg[8], 0x698098d8u32, 7, 25);
        rx!(F, d, a, b, c, msg[9], 0x8b44f7afu32, 12, 20);
        rx!(F, c, d, a, b, msg[10], 0xffff5bb1u32, 17, 15);
        rx!(F, b, c, d, a, msg[11], 0x895cd7beu32, 22, 10);
        rx!(F, a, b, c, d, msg[12], 0x6b901122u32, 7, 25);
        rx!(F, d, a, b, c, msg[13], 0xfd987193u32, 12, 20);
        rx!(F, c, d, a, b, msg[14], 0xa679438eu32, 17, 15);
        rx!(F, b, c, d, a, msg[15], 0x49b40821u32, 22, 10);

        // Round 16-31 (G)
        rx!(G, a, b, c, d, msg[1], 0xf61e2562u32, 5, 27);
        rx!(G, d, a, b, c, msg[6], 0xc040b340u32, 9, 23);
        rx!(G, c, d, a, b, msg[11], 0x265e5a51u32, 14, 18);
        rx!(G, b, c, d, a, msg[0], 0xe9b6c7aau32, 20, 12);
        rx!(G, a, b, c, d, msg[5], 0xd62f105du32, 5, 27);
        rx!(G, d, a, b, c, msg[10], 0x02441453u32, 9, 23);
        rx!(G, c, d, a, b, msg[15], 0xd8a1e681u32, 14, 18);
        rx!(G, b, c, d, a, msg[4], 0xe7d3fbc8u32, 20, 12);
        rx!(G, a, b, c, d, msg[9], 0x21e1cde6u32, 5, 27);
        rx!(G, d, a, b, c, msg[14], 0xc33707d6u32, 9, 23);
        rx!(G, c, d, a, b, msg[3], 0xf4d50d87u32, 14, 18);
        rx!(G, b, c, d, a, msg[8], 0x455a14edu32, 20, 12);
        rx!(G, a, b, c, d, msg[13], 0xa9e3e905u32, 5, 27);
        rx!(G, d, a, b, c, msg[2], 0xfcefa3f8u32, 9, 23);
        rx!(G, c, d, a, b, msg[7], 0x676f02d9u32, 14, 18);
        rx!(G, b, c, d, a, msg[12], 0x8d2a4c8au32, 20, 12);

        // Round 32-47 (H)
        rx!(H, a, b, c, d, msg[5], 0xfffa3942u32, 4, 28);
        rx!(H, d, a, b, c, msg[8], 0x8771f681u32, 11, 21);
        rx!(H, c, d, a, b, msg[11], 0x6d9d6122u32, 16, 16);
        rx!(H, b, c, d, a, msg[14], 0xfde5380cu32, 23, 9);
        rx!(H, a, b, c, d, msg[1], 0xa4beea44u32, 4, 28);
        rx!(H, d, a, b, c, msg[4], 0x4bdecfa9u32, 11, 21);
        rx!(H, c, d, a, b, msg[7], 0xf6bb4b60u32, 16, 16);
        rx!(H, b, c, d, a, msg[10], 0xbebfbc70u32, 23, 9);
        rx!(H, a, b, c, d, msg[13], 0x289b7ec6u32, 4, 28);
        rx!(H, d, a, b, c, msg[0], 0xeaa127fau32, 11, 21);
        rx!(H, c, d, a, b, msg[3], 0xd4ef3085u32, 16, 16);
        rx!(H, b, c, d, a, msg[6], 0x04881d05u32, 23, 9);
        rx!(H, a, b, c, d, msg[9], 0xd9d4d039u32, 4, 28);
        rx!(H, d, a, b, c, msg[12], 0xe6db99e5u32, 11, 21);
        rx!(H, c, d, a, b, msg[15], 0x1fa27cf8u32, 16, 16);
        rx!(H, b, c, d, a, msg[2], 0xc4ac5665u32, 23, 9);

        // Round 48-63 (I)
        rx!(I, a, b, c, d, msg[0], 0xf4292244u32, 6, 26);
        rx!(I, d, a, b, c, msg[7], 0x432aff97u32, 10, 22);
        rx!(I, c, d, a, b, msg[14], 0xab9423a7u32, 15, 17);
        rx!(I, b, c, d, a, msg[5], 0xfc93a039u32, 21, 11);
        rx!(I, a, b, c, d, msg[12], 0x655b59c3u32, 6, 26);
        rx!(I, d, a, b, c, msg[3], 0x8f0ccc92u32, 10, 22);
        rx!(I, c, d, a, b, msg[10], 0xffeff47du32, 15, 17);
        rx!(I, b, c, d, a, msg[1], 0x85845dd1u32, 21, 11);
        rx!(I, a, b, c, d, msg[8], 0x6fa87e4fu32, 6, 26);
        rx!(I, d, a, b, c, msg[15], 0xfe2ce6e0u32, 10, 22);
        rx!(I, c, d, a, b, msg[6], 0xa3014314u32, 15, 17);
        rx!(I, b, c, d, a, msg[13], 0x4e0811a1u32, 21, 11);
        rx!(I, a, b, c, d, msg[4], 0xf7537e82u32, 6, 26);
        rx!(I, d, a, b, c, msg[11], 0xbd3af235u32, 10, 22);
        rx!(I, c, d, a, b, msg[2], 0x2ad7d2bbu32, 15, 17);
        rx!(I, b, c, d, a, msg[9], 0xeb86d391u32, 21, 11);

        // Davies-Meyer feed-forward: add initial state to compressed state.
        state.regs[0] = vadd_u32(oa, a);
        state.regs[1] = vadd_u32(ob, b);
        state.regs[2] = vadd_u32(oc, c);
        state.regs[3] = vadd_u32(od, d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parpar_hasher::md5x2::Md5x2;
    use crate::parpar_hasher::md5x2_scalar::Scalar;

    /// Verify NEON process_block produces the same digest as the scalar oracle
    /// for an arbitrary pair of distinct 64-byte blocks.
    ///
    /// This test can only run on aarch64 hardware (or QEMU aarch64).
    #[test]
    fn neon_matches_scalar_oracle() {
        // Two distinct 64-byte blocks.
        let block1: [u8; 64] = core::array::from_fn(|i| i as u8);
        let block2: [u8; 64] = core::array::from_fn(|i| (i as u8).wrapping_mul(3).wrapping_add(7));

        // Compute with NEON backend.
        let neon_digest = unsafe {
            let mut state = State::init_state();
            State::init_lane(&mut state, 0);
            State::init_lane(&mut state, 1);
            State::process_block(&mut state, block1.as_ptr(), block2.as_ptr());
            (
                State::extract_lane(&state, 0),
                State::extract_lane(&state, 1),
            )
        };

        // Compute with scalar oracle (two independent MD5 states).
        let scalar_lane0 = unsafe {
            let mut state = Scalar::init_state();
            Scalar::init_lane(&mut state, 0);
            Scalar::init_lane(&mut state, 1);
            Scalar::process_block(&mut state, block1.as_ptr(), block1.as_ptr());
            Scalar::extract_lane(&state, 0)
        };
        let scalar_lane1 = unsafe {
            let mut state = Scalar::init_state();
            Scalar::init_lane(&mut state, 0);
            Scalar::init_lane(&mut state, 1);
            Scalar::process_block(&mut state, block2.as_ptr(), block2.as_ptr());
            Scalar::extract_lane(&state, 0)
        };

        assert_eq!(neon_digest.0, scalar_lane0, "NEON lane0 mismatch vs scalar");
        assert_eq!(neon_digest.1, scalar_lane1, "NEON lane1 mismatch vs scalar");
    }
}
