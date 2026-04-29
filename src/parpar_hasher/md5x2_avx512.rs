// Copyright (C) Anomaly Industries Inc. and par2rs contributors.
// SPDX-License-Identifier: GPL-2.0-or-later
//
// AVX-512VL two-lane MD5 block compression for x86_64.
//
// 1:1 Rust port of the `__AVX512VL__` specialisation produced by:
//   parpar/hasher/md5x2-sse.h     lines ~109-134 (`#ifdef __AVX512VL__` block)
//   parpar/hasher/md5x2-base.h    (round expansion machinery — unchanged)
//   parpar/hasher/md5-base.h      (the actual 64-round body — unchanged)
// from par2cmdline-turbo (https://github.com/animetosho/par2cmdline-turbo)
// and upstream ParPar (https://github.com/animetosho/ParPar), both
// GPL-2.0-or-later.
//
// ## What's different vs the SSE2 backend
//
// **State layout: identical.** Upstream aliases
//   #define md5_extract_x2_avx512 md5_extract_x2_sse
// so the lane layout in xmm registers (`[lane0_word, GARBAGE,
// lane1_word, GARBAGE]`) is shared verbatim. We re-use `State`,
// `from_flat`, `to_flat`, and `load4` from `md5x2_sse2` so any future
// layout fix lands in both backends at once.
//
// **Bool functions: vpternlogd.** AVX-512VL's `vpternlogd` collapses
// any 3-input boolean expression into one instruction. Upstream
// (md5x2-sse.h:122-128 in the `__AVX512VL__` block) redefines:
//
//     #define F(b,c,d) _mm_ternarylogic_epi32(d,c,b,0xD8)
//     #define G(b,c,d) _mm_ternarylogic_epi32(d,c,b,0xAC)
//     #define H(b,c,d) _mm_ternarylogic_epi32(d,c,b,0x96)
//     #define I(b,c,d) _mm_ternarylogic_epi32(d,c,b,0x63)
//
// Note ADDF is *not* defined in this block, so md5-base.h's fallback
// `_ADDF(f,a,b,c,d) = ADD(a, f(b,c,d))` is used — i.e. round step is
//   a = a + ik
//   a = a + f(b,c,d)        // single vpternlogd then vpaddd
//   a = ROL(a, r)
//   a = a + b
//
// Truth-table sanity check (vpternlogd indexes by `(d_bit<<2)|(c_bit<<1)|b_bit`
// and the 8-bit IMM is the lookup table indexed MSB→LSB):
//
//   F = ((c ^ d) & b) ^ d → indices 011,100,110,111 are 1 → 0xD8 ✓
//   H = b ^ c ^ d         → indices 001,010,100,111 are 1 → 0x96 ✓
//   G = (~d & c) | (d & b) (eq. (~d & c) + (d & b) since disjoint)
//                          → indices 010,011,101,111 are 1 → 0xAC ✓
//   I = (~d | b) ^ c       → indices 000,001,011,101,110 are 1 → 0x63 ✓
//
// **Rotate: vprold.** `_mm_rol_epi32::<R>(a)` is a real per-dword
// 32-bit rotate, so the SSE2 shuffle+srli_epi64 trick (and its 32-r
// pre-subtraction) goes away. The macro callers pass `r` directly.
//
// ## Why we don't widen to YMM/ZMM
//
// Upstream's `HasherInput_AVX512` keeps the 128-bit footprint — the
// AVX-512 block is purely about replacing 2-3 ops with 1, not about
// processing 2× or 4× the data. Widening would require reworking the
// fused driver in `hasher_input.rs` and is explicitly out of scope.

#![cfg(target_arch = "x86_64")]
#![allow(non_snake_case)] // mirror upstream variable names A, B, C, D, XX0..XX15

use core::arch::x86_64::{
    __m128i, _mm_add_epi32, _mm_rol_epi32, _mm_set1_epi32, _mm_ternarylogic_epi32,
};

use crate::parpar_hasher::md5x2_sse2::{load4 as load4_xmm, State};

/// 32-bit constant broadcast into all four 32-bit lanes. Direct port
/// of upstream `VAL(k) = _mm_set1_epi32(k)`.
#[inline(always)]
#[allow(unused_unsafe)]
unsafe fn val(k: u32) -> __m128i {
    _mm_set1_epi32(k as i32)
}

// ----- Bool functions, one vpternlogd each -----
//
// `_mm_ternarylogic_epi32::<IMM>(a, b, c)` evaluates the boolean
// function whose 8-entry truth table is `IMM`, indexed MSB-first by
// `(a_bit << 2) | (b_bit << 1) | c_bit`. Upstream calls these as
// `_mm_ternarylogic_epi32(d, c, b, IMM)`, so we mirror the `(d, c, b)`
// argument order verbatim.

#[inline(always)]
#[allow(unused_unsafe)]
unsafe fn addf_f(a: __m128i, b: __m128i, c: __m128i, d: __m128i) -> __m128i {
    let f = _mm_ternarylogic_epi32::<0xD8>(d, c, b);
    _mm_add_epi32(a, f)
}

#[inline(always)]
#[allow(unused_unsafe)]
unsafe fn addf_g(a: __m128i, b: __m128i, c: __m128i, d: __m128i) -> __m128i {
    let g = _mm_ternarylogic_epi32::<0xAC>(d, c, b);
    _mm_add_epi32(a, g)
}

#[inline(always)]
#[allow(unused_unsafe)]
unsafe fn addf_h(a: __m128i, b: __m128i, c: __m128i, d: __m128i) -> __m128i {
    let h = _mm_ternarylogic_epi32::<0x96>(d, c, b);
    _mm_add_epi32(a, h)
}

#[inline(always)]
#[allow(unused_unsafe)]
unsafe fn addf_i(a: __m128i, b: __m128i, c: __m128i, d: __m128i) -> __m128i {
    let i = _mm_ternarylogic_epi32::<0x63>(d, c, b);
    _mm_add_epi32(a, i)
}

// ----------------------------------------------------------------------
// Round body. Same shape as the SSE2 macro, but `vprold` takes the
// rotate amount directly (no 32-r pre-subtraction).
// ----------------------------------------------------------------------

macro_rules! rx {
    ($f_helper:ident, $a:ident, $b:ident, $c:ident, $d:ident, $xx:expr, $r:literal, $k:expr) => {{
        $a = _mm_add_epi32($a, _mm_add_epi32($xx, val($k)));
        $a = $f_helper($a, $b, $c, $d);
        $a = _mm_rol_epi32::<$r>($a);
        $a = _mm_add_epi32($a, $b);
    }};
}

/// Process one 64-byte block per lane, MD5x2 AVX-512VL.
///
/// 1:1 expansion of `md5_process_block_x2_avx512` from the
/// `__AVX512VL__` block of upstream `md5x2-sse.h` (after re-substitution
/// via `md5x2-base.h` → `md5-base.h`).
///
/// # Safety
/// Caller must ensure CPU supports `avx512f` + `avx512vl`. Both
/// pointers must be valid for 64-byte reads. They may alias.
#[target_feature(enable = "avx512f,avx512vl")]
pub unsafe fn process_block_x2_avx512(state: &mut State, data1: *const u8, data2: *const u8) {
    let oA = state.0[0];
    let oB = state.0[1];
    let oC = state.0[2];
    let oD = state.0[3];

    let mut A = oA;
    let mut B = oB;
    let mut C = oC;
    let mut D = oD;

    // load4 reuses upstream LOAD4 — same in both AVX-512 and SSE2 paths.
    let (XX0, XX1, XX2, XX3) = load4_xmm(data1, data2, 0);
    let (XX4, XX5, XX6, XX7) = load4_xmm(data1, data2, 4);
    let (XX8, XX9, XX10, XX11) = load4_xmm(data1, data2, 8);
    let (XX12, XX13, XX14, XX15) = load4_xmm(data1, data2, 12);

    // ----- Round 0 (F) — md5-base.h:148-174 -----
    rx!(addf_f, A, B, C, D, XX0, 7, 0xd76aa478);
    rx!(addf_f, D, A, B, C, XX1, 12, 0xe8c7b756);
    rx!(addf_f, C, D, A, B, XX2, 17, 0x242070db);
    rx!(addf_f, B, C, D, A, XX3, 22, 0xc1bdceee);
    rx!(addf_f, A, B, C, D, XX4, 7, 0xf57c0faf);
    rx!(addf_f, D, A, B, C, XX5, 12, 0x4787c62a);
    rx!(addf_f, C, D, A, B, XX6, 17, 0xa8304613);
    rx!(addf_f, B, C, D, A, XX7, 22, 0xfd469501);
    rx!(addf_f, A, B, C, D, XX8, 7, 0x698098d8);
    rx!(addf_f, D, A, B, C, XX9, 12, 0x8b44f7af);
    rx!(addf_f, C, D, A, B, XX10, 17, 0xffff5bb1);
    rx!(addf_f, B, C, D, A, XX11, 22, 0x895cd7be);
    rx!(addf_f, A, B, C, D, XX12, 7, 0x6b901122);
    rx!(addf_f, D, A, B, C, XX13, 12, 0xfd987193);
    rx!(addf_f, C, D, A, B, XX14, 17, 0xa679438e);
    rx!(addf_f, B, C, D, A, XX15, 22, 0x49b40821);

    // ----- Round 1 (G) — md5-base.h:192-207 -----
    rx!(addf_g, A, B, C, D, XX1, 5, 0xf61e2562);
    rx!(addf_g, D, A, B, C, XX6, 9, 0xc040b340);
    rx!(addf_g, C, D, A, B, XX11, 14, 0x265e5a51);
    rx!(addf_g, B, C, D, A, XX0, 20, 0xe9b6c7aa);
    rx!(addf_g, A, B, C, D, XX5, 5, 0xd62f105d);
    rx!(addf_g, D, A, B, C, XX10, 9, 0x02441453);
    rx!(addf_g, C, D, A, B, XX15, 14, 0xd8a1e681);
    rx!(addf_g, B, C, D, A, XX4, 20, 0xe7d3fbc8);
    rx!(addf_g, A, B, C, D, XX9, 5, 0x21e1cde6);
    rx!(addf_g, D, A, B, C, XX14, 9, 0xc33707d6);
    rx!(addf_g, C, D, A, B, XX3, 14, 0xf4d50d87);
    rx!(addf_g, B, C, D, A, XX8, 20, 0x455a14ed);
    rx!(addf_g, A, B, C, D, XX13, 5, 0xa9e3e905);
    rx!(addf_g, D, A, B, C, XX2, 9, 0xfcefa3f8);
    rx!(addf_g, C, D, A, B, XX7, 14, 0x676f02d9);
    rx!(addf_g, B, C, D, A, XX12, 20, 0x8d2a4c8a);

    // ----- Round 2 (H) — md5-base.h:225-240 -----
    rx!(addf_h, A, B, C, D, XX5, 4, 0xfffa3942);
    rx!(addf_h, D, A, B, C, XX8, 11, 0x8771f681);
    rx!(addf_h, C, D, A, B, XX11, 16, 0x6d9d6122);
    rx!(addf_h, B, C, D, A, XX14, 23, 0xfde5380c);
    rx!(addf_h, A, B, C, D, XX1, 4, 0xa4beea44);
    rx!(addf_h, D, A, B, C, XX4, 11, 0x4bdecfa9);
    rx!(addf_h, C, D, A, B, XX7, 16, 0xf6bb4b60);
    rx!(addf_h, B, C, D, A, XX10, 23, 0xbebfbc70);
    rx!(addf_h, A, B, C, D, XX13, 4, 0x289b7ec6);
    rx!(addf_h, D, A, B, C, XX0, 11, 0xeaa127fa);
    rx!(addf_h, C, D, A, B, XX3, 16, 0xd4ef3085);
    rx!(addf_h, B, C, D, A, XX6, 23, 0x04881d05);
    rx!(addf_h, A, B, C, D, XX9, 4, 0xd9d4d039);
    rx!(addf_h, D, A, B, C, XX12, 11, 0xe6db99e5);
    rx!(addf_h, C, D, A, B, XX15, 16, 0x1fa27cf8);
    rx!(addf_h, B, C, D, A, XX2, 23, 0xc4ac5665);

    // ----- Round 3 (I) — md5-base.h:263-278 -----
    //
    // IMPORTANT — IOFFSET handling: upstream's AVX (and only AVX)
    // `__AVX__` block sets `IOFFSET = -1` because its `ADDF` redefines
    // I as `_mm_sub_epi32(a, c ^ andnot(b, d))`, and the K constants
    // need a -1 compensation. The `__AVX512VL__` block does NOT set
    // IOFFSET (md5x2-sse.h:128-130 — `# ifdef IOFFSET / # undef
    // IOFFSET / # endif` clears it before include). So our AVX-512
    // path uses the SSE2-style I (= `((~d|b) ^ c)`) and the
    // unmodified K constants. Same constants as the SSE2 backend.
    rx!(addf_i, A, B, C, D, XX0, 6, 0xf4292244);
    rx!(addf_i, D, A, B, C, XX7, 10, 0x432aff97);
    rx!(addf_i, C, D, A, B, XX14, 15, 0xab9423a7);
    rx!(addf_i, B, C, D, A, XX5, 21, 0xfc93a039);
    rx!(addf_i, A, B, C, D, XX12, 6, 0x655b59c3);
    rx!(addf_i, D, A, B, C, XX3, 10, 0x8f0ccc92);
    rx!(addf_i, C, D, A, B, XX10, 15, 0xffeff47d);
    rx!(addf_i, B, C, D, A, XX1, 21, 0x85845dd1);
    rx!(addf_i, A, B, C, D, XX8, 6, 0x6fa87e4f);
    rx!(addf_i, D, A, B, C, XX15, 10, 0xfe2ce6e0);
    rx!(addf_i, C, D, A, B, XX6, 15, 0xa3014314);
    rx!(addf_i, B, C, D, A, XX13, 21, 0x4e0811a1);
    rx!(addf_i, A, B, C, D, XX4, 6, 0xf7537e82);
    rx!(addf_i, D, A, B, C, XX11, 10, 0xbd3af235);
    rx!(addf_i, C, D, A, B, XX2, 15, 0x2ad7d2bb);
    rx!(addf_i, B, C, D, A, XX9, 21, 0xeb86d391);

    state.0[0] = _mm_add_epi32(oA, A);
    state.0[1] = _mm_add_epi32(oB, B);
    state.0[2] = _mm_add_epi32(oC, C);
    state.0[3] = _mm_add_epi32(oD, D);
}

/// `Md5x2` backend wrapper for the AVX-512VL implementation.
///
/// Caller is responsible for runtime feature detection — typically
/// done via `HasherInputDyn::new()` which checks `is_x86_feature_detected!`
/// before instantiating this backend.
pub struct Avx512;

impl crate::parpar_hasher::md5x2::Md5x2 for Avx512 {
    type State = State;

    const USE_AVX512_CRC: bool = true;

    #[inline(always)]
    fn init_state() -> Self::State {
        // Same XMM lane layout as SSE2 — `from_flat` is shared verbatim.
        let flat: [u32; 8] = [
            0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0x67452301, 0xefcdab89, 0x98badcfe,
            0x10325476,
        ];
        unsafe { State::from_flat(&flat) }
    }

    #[inline(always)]
    fn init_lane(state: &mut Self::State, lane: usize) {
        debug_assert!(lane < 2);
        let mut flat = unsafe { state.to_flat() };
        let off = lane * 4;
        flat[off] = 0x67452301;
        flat[off + 1] = 0xefcdab89;
        flat[off + 2] = 0x98badcfe;
        flat[off + 3] = 0x10325476;
        *state = unsafe { State::from_flat(&flat) };
    }

    #[inline(always)]
    fn extract_lane(state: &Self::State, lane: usize) -> [u8; 16] {
        debug_assert!(lane < 2);
        let flat = unsafe { state.to_flat() };
        let off = lane * 4;
        let mut out = [0u8; 16];
        for i in 0..4 {
            out[i * 4..i * 4 + 4].copy_from_slice(&flat[off + i].to_le_bytes());
        }
        out
    }

    #[inline(always)]
    unsafe fn process_block(state: &mut Self::State, data1: *const u8, data2: *const u8) {
        process_block_x2_avx512(state, data1, data2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parpar_hasher::md5x2::Md5x2;
    use crate::parpar_hasher::md5x2_scalar::Scalar;

    fn avx512_supported() -> bool {
        is_x86_feature_detected!("avx512f") && is_x86_feature_detected!("avx512vl")
    }

    /// Hash an N-block message via scalar and AVX-512 backends and
    /// assert lane digests match exactly. Mirrors `md5x2_sse2::tests`.
    ///
    /// Panics if AVX-512 is unsupported on this CPU — callers are
    /// expected to be `#[ignore]`-marked tests so non-AVX-512 hosts
    /// don't run them at all (rather than silently passing).
    fn cross_check(blocks: &[u8]) {
        assert!(
            avx512_supported(),
            "AVX-512 cross-check invoked on CPU without avx512f+vl — \
             this test is #[ignore]'d; run with --ignored on AVX-512 hardware"
        );
        assert_eq!(blocks.len() % 64, 0);
        let n = blocks.len() / 64;

        let mut s_state = Scalar::init_state();
        let mut v_state = Avx512::init_state();

        for i in 0..n {
            let d1 = &blocks[i * 64..i * 64 + 64];
            let d2 = &blocks[(n - 1 - i) * 64..(n - 1 - i) * 64 + 64];
            unsafe {
                Scalar::process_block(&mut s_state, d1.as_ptr(), d2.as_ptr());
                Avx512::process_block(&mut v_state, d1.as_ptr(), d2.as_ptr());
            }
        }

        let s0 = Scalar::extract_lane(&s_state, 0);
        let s1 = Scalar::extract_lane(&s_state, 1);
        let v0 = Avx512::extract_lane(&v_state, 0);
        let v1 = Avx512::extract_lane(&v_state, 1);

        assert_eq!(v0, s0, "lane 0 mismatch after {n} blocks");
        assert_eq!(v1, s1, "lane 1 mismatch after {n} blocks");
    }

    fn synth(len: usize, seed: u64) -> Vec<u8> {
        let mut s = seed;
        let mut out = vec![0u8; len];
        for byte in out.iter_mut() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            *byte = s as u8;
        }
        out
    }

    #[test]
    #[ignore = "requires AVX-512VL hardware"]
    fn one_block() {
        cross_check(&synth(64, 1));
    }

    #[test]
    #[ignore = "requires AVX-512VL hardware"]
    fn two_blocks() {
        cross_check(&synth(128, 2));
    }

    #[test]
    #[ignore = "requires AVX-512VL hardware"]
    fn many_blocks() {
        cross_check(&synth(64 * 17, 3));
    }

    #[test]
    #[ignore = "requires AVX-512VL hardware"]
    fn lane_reset_round_trip() {
        assert!(
            avx512_supported(),
            "lane_reset_round_trip invoked on non-AVX-512 host"
        );
        let mut state = Avx512::init_state();
        let block = synth(64, 4);
        unsafe {
            Avx512::process_block(&mut state, block.as_ptr(), block.as_ptr());
        }
        let lane1_before = Avx512::extract_lane(&state, 1);
        Avx512::init_lane(&mut state, 0);
        let lane1_after = Avx512::extract_lane(&state, 1);
        let lane0_after = Avx512::extract_lane(&state, 0);
        assert_eq!(lane1_before, lane1_after, "init_lane(0) clobbered lane 1");
        let iv: [u8; 16] = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
            0x32, 0x10,
        ];
        assert_eq!(lane0_after, iv, "init_lane(0) didn't restore IV");
    }
}
