// Copyright (C) Anomaly Industries Inc. and par2rs contributors.
// SPDX-License-Identifier: GPL-2.0-or-later
//
// SSE2 two-lane MD5 block compression for x86_64.
//
// 1:1 Rust port of the SSE2 specialisation produced by:
//   parpar/hasher/md5x2-sse.h     (provides ROTATE, ADDF, init_lane, extract macros)
//   parpar/hasher/md5x2-base.h    (provides the round expansion machinery)
//   parpar/hasher/md5-base.h      (provides the actual 64-round body)
// from par2cmdline-turbo (https://github.com/animetosho/par2cmdline-turbo)
// and upstream ParPar (https://github.com/animetosho/ParPar), both
// GPL-2.0-or-later.
//
// State layout (matches upstream `state_[i]` where `state_` is `__m128i*`):
//
//   state[0]   xmm holding A: [A_lane0 (32b), GARBAGE (32b), A_lane1 (32b), GARBAGE (32b)]
//   state[1]   xmm holding B: [B_lane0,       GARBAGE,       B_lane1,       GARBAGE]
//   state[2]   xmm holding C: [C_lane0,       GARBAGE,       C_lane1,       GARBAGE]
//   state[3]   xmm holding D: [D_lane0,       GARBAGE,       D_lane1,       GARBAGE]
//
// Each MD5 word lives in 32 bits; SSE2 holds two of them per xmm in
// 32-bit lanes 0 and 2 (the qword-low halves of each qword), with the
// other two 32-bit lanes containing don't-care bits. The "rotate by r"
// trick from upstream:
//
//     ROTATE(a, r) = _mm_srli_epi64(_mm_shuffle_epi32(a, _MM_SHUFFLE(2,2,0,0)), 32-r)
//
// The shuffle duplicates each valid 32-bit word into both 32-bit halves
// of the 64-bit qword that holds it ([X, ?, Y, ?] -> [X, X, Y, Y]).
// `srli_epi64(.., 32-r)` then logically shifts each 64-bit qword right
// by `32-r`, producing `[ROL(X, r), 0, ROL(Y, r), 0]` (the low 32 bits
// of each qword are the desired rotated result; the high 32 bits become
// "GARBAGE" for the next round, which is fine — only 32-bit lanes 0
// and 2 ever carry signal).
//
// The four MD5 bool functions (F, G, H, I) are spelled exactly as
// upstream's `ADDF` macro:
//
//   F(b, c, d) = ((c ^ d) & b) ^ d
//   G(b, c, d) = (~d & c) + (d & b)         /* note: ADD, not OR */
//   H(b, c, d) = d ^ c ^ b
//   I(b, c, d) = (~d | b) ^ c
//
// Per-round update body (`_RX` from md5-base.h, with `ADDF` from
// md5x2-sse.h's SSE2 block):
//
//   a = a + ik         /* ik = INPUT(k, ...) = m[i] + K[i] (constant + data word) */
//   a = a + f(b,c,d)   /* via ADDF, which folds in the f directly */
//   a = ROTATE(a, r)
//   a = a + b
//
// Round 0 (F) uses pre-loaded data words from `LOAD4`; rounds 1-3 use
// the same `XX0..XX15` state, since MD5 reuses the same 16 message
// words across rounds in shuffled orders.

#![cfg(target_arch = "x86_64")]
#![allow(non_snake_case)] // mirror upstream variable names A, B, C, D, XX0..XX15

use core::arch::x86_64::{
    __m128i, _mm_add_epi32, _mm_and_si128, _mm_andnot_si128, _mm_loadu_si128, _mm_set1_epi32,
    _mm_set_epi32, _mm_shuffle_epi32, _mm_srli_epi64, _mm_storeu_si128, _mm_unpackhi_epi64,
    _mm_unpacklo_epi64, _mm_xor_si128,
};

/// Internal SSE2 state: four 128-bit registers, each carrying both
/// lanes' matching MD5 word in 32-bit positions 0 and 2.
#[derive(Clone, Copy)]
#[repr(C, align(16))]
pub struct State(pub [__m128i; 4]);

impl State {
    /// Build the SSE2 state from a flat `[u32; 8]` (`[a0,b0,c0,d0,a1,b1,c1,d1]`).
    /// Used at backend boundaries: init / lane reset / extract.
    ///
    /// Made `pub(crate)` so the AVX-512VL backend can share the exact
    /// same XMM lane layout (`md5_extract_x2_avx512 = md5_extract_x2_sse`
    /// in upstream `md5x2-sse.h`).
    #[inline(always)]
    pub(crate) unsafe fn from_flat(flat: &[u32; 8]) -> Self {
        // Each xmm holds [word_lane0, 0, word_lane1, 0].
        // _mm_set_epi32 takes args in (e3, e2, e1, e0) order — i.e.
        // bits 96..128, 64..96, 32..64, 0..32 — so to get
        // [lane0, 0, lane1, 0] in positions (e0, e1, e2, e3) we pass
        // _mm_set_epi32(0, lane1, 0, lane0).
        let a = _mm_set_epi32(0, flat[4] as i32, 0, flat[0] as i32);
        let b = _mm_set_epi32(0, flat[5] as i32, 0, flat[1] as i32);
        let c = _mm_set_epi32(0, flat[6] as i32, 0, flat[2] as i32);
        let d = _mm_set_epi32(0, flat[7] as i32, 0, flat[3] as i32);
        State([a, b, c, d])
    }

    /// Convert back to flat `[u32; 8]` form by reading 32-bit lanes 0
    /// and 2 of each register. The "GARBAGE" lanes 1 and 3 are discarded.
    ///
    /// `pub(crate)` for the same reason as `from_flat` — the AVX-512VL
    /// backend reuses this layout verbatim.
    #[inline(always)]
    pub(crate) unsafe fn to_flat(&self) -> [u32; 8] {
        let mut tmp = [0u32; 4];
        let mut out = [0u32; 8];
        for (idx, reg) in self.0.iter().enumerate() {
            _mm_storeu_si128(tmp.as_mut_ptr() as *mut __m128i, *reg);
            out[idx] = tmp[0]; // lane 0 -> [a0,b0,c0,d0]
            out[idx + 4] = tmp[2]; // lane 1 -> [a1,b1,c1,d1]
        }
        out
    }
}

/// 32-bit constant broadcast into both valid (and both garbage) slots.
/// Direct port of upstream `VAL(k) = _mm_set1_epi32(k)`.
#[inline(always)]
#[allow(unused_unsafe)]
unsafe fn val(k: u32) -> __m128i {
    _mm_set1_epi32(k as i32)
}

/// Upstream `ROTATE(a, r) = _mm_srli_epi64(_mm_shuffle_epi32(a, 2200), 32-r)`.
///
/// `_mm_srli_epi64` requires its shift count as an immediate, and stable
/// Rust's const-generic arithmetic (`{ 32 - R }`) isn't allowed in const
/// position, so the macro callers pass the *already-subtracted* value
/// `S = 32 - r` directly.
#[inline(always)]
#[allow(unused_unsafe)]
unsafe fn rotate<const S: i32>(a: __m128i) -> __m128i {
    // _MM_SHUFFLE(2,2,0,0) = (2<<6)|(2<<4)|(0<<2)|0 = 0xa0
    let dup = _mm_shuffle_epi32(a, 0xa0);
    _mm_srli_epi64::<S>(dup)
}

/// LOAD4 for one source pointer: load 16 contiguous bytes (= 4 input words),
/// returning four xmm regs each shaped `[word_X, GARBAGE, word_X', GARBAGE]`
/// where `_X` and `_X'` are the corresponding words from data1 and data2.
///
/// Direct port of `LOAD4` macro (md5x2-sse.h:13-20). Loads 4 input words
/// from each of `ptr0` and `ptr1` at byte offset `idx*4`, interleaves
/// them into MD5x2 layout, and returns (XX_idx, XX_idx+1, XX_idx+2, XX_idx+3).
// `pub(crate)` so the AVX-512VL backend can reuse the same load+shuffle
// path: upstream's `LOAD4` is shared between SSE and AVX-512 codepaths
// (`md5x2-sse.h` defines `LOAD4` once at file scope and the AVX-512VL
// block doesn't redefine it).
#[inline(always)]
#[allow(unused_unsafe)]
pub(crate) unsafe fn load4(
    ptr0: *const u8,
    ptr1: *const u8,
    idx: usize,
) -> (__m128i, __m128i, __m128i, __m128i) {
    let in0 = _mm_loadu_si128(ptr0.add(idx * 4) as *const __m128i);
    let in1 = _mm_loadu_si128(ptr1.add(idx * 4) as *const __m128i);
    // var0 = unpacklo(in0, in1) = [in0_w0, in1_w0, in0_w1, in1_w1]
    //   -> 32-bit lanes (0,1,2,3) where (0,2) carry the two lanes' word0/word1 alternately.
    // Wait — that's not the upstream layout. Re-read md5x2-sse.h:14-19:
    //
    //   in0 = loadu(ptr0+idx*4)              [in0_w0, in0_w1, in0_w2, in0_w3]
    //   in1 = loadu(ptr1+idx*4)              [in1_w0, in1_w1, in1_w2, in1_w3]
    //   var0 = unpacklo_epi64(in0, in1)      [in0_w0, in0_w1, in1_w0, in1_w1]
    //   var1 = shuffle_epi32(var0, 2,3,0,1)  [in0_w1, in0_w0, in1_w1, in1_w0]
    //   var2 = unpackhi_epi64(in0, in1)      [in0_w2, in0_w3, in1_w2, in1_w3]
    //   var3 = shuffle_epi32(var2, 2,3,0,1)  [in0_w3, in0_w2, in1_w3, in1_w2]
    //
    // So var0 has shape [w0_lane0, GARBAGE_lane0, w0_lane1, GARBAGE_lane1]?
    // Position 0 = in0_w0 (lane0 word0). Position 1 = in0_w1 (this is the
    // "garbage" slot for word0 — it holds the *next* word, but at round
    // time it's masked out by ROTATE / state updates only writing the
    // qword-low halves). Position 2 = in1_w0 (lane1 word0). Position 3 =
    // in1_w1 (lane1's garbage for word0).
    //
    // So var0 = XX_idx (word i for both lanes, with their word i+1 in
    // the garbage slots). Similarly:
    //
    //   var1 = XX_idx+1: position 0 = in0_w1 (lane0 word i+1), position 2 = in1_w1 (lane1 word i+1).
    //   var2 = XX_idx+2: position 0 = in0_w2, position 2 = in1_w2.
    //   var3 = XX_idx+3: position 0 = in0_w3, position 2 = in1_w3.
    //
    // The "garbage" 32-bit slots are *deterministic but unused* — only
    // the qword-low halves (32-bit lanes 0 and 2) carry the active state.
    let var0 = _mm_unpacklo_epi64(in0, in1);
    // _MM_SHUFFLE(2,3,0,1) = (2<<6)|(3<<4)|(0<<2)|1 = 0xb1
    let var1 = _mm_shuffle_epi32(var0, 0xb1);
    let var2 = _mm_unpackhi_epi64(in0, in1);
    let var3 = _mm_shuffle_epi32(var2, 0xb1);
    (var0, var1, var2, var3)
}

// ----- Bool functions ADDF, expanded per-round from md5x2-sse.h:43-50 -----
//
// Each `addf_*` returns ADD(a, f(b,c,d)) so the round body can be:
//
//   a = a + ik
//   a = addf_X(a, b, c, d)
//   a = rotate(a, r)
//   a = a + b

#[inline(always)]
#[allow(unused_unsafe)]
unsafe fn addf_f(a: __m128i, b: __m128i, c: __m128i, d: __m128i) -> __m128i {
    // F: ((c ^ d) & b) ^ d
    let cd = _mm_xor_si128(c, d);
    let cdb = _mm_and_si128(cd, b);
    let f = _mm_xor_si128(cdb, d);
    _mm_add_epi32(a, f)
}

#[inline(always)]
#[allow(unused_unsafe)]
unsafe fn addf_g(a: __m128i, b: __m128i, c: __m128i, d: __m128i) -> __m128i {
    // G special form (upstream md5x2-sse.h:44):
    //   ADD(ADD(andnot(d, c), a), and(d, b))
    // i.e. a + (~d & c) + (d & b)
    let andn = _mm_andnot_si128(d, c);
    let andd = _mm_and_si128(d, b);
    _mm_add_epi32(_mm_add_epi32(andn, a), andd)
}

#[inline(always)]
#[allow(unused_unsafe)]
unsafe fn addf_h(a: __m128i, b: __m128i, c: __m128i, d: __m128i) -> __m128i {
    // H: d ^ c ^ b
    let h = _mm_xor_si128(_mm_xor_si128(d, c), b);
    _mm_add_epi32(a, h)
}

#[inline(always)]
#[allow(unused_unsafe)]
unsafe fn addf_i(a: __m128i, b: __m128i, c: __m128i, d: __m128i) -> __m128i {
    // I (SSE2 form, md5x2-sse.h:47):
    //   xor(or(xor(d, set1_epi8(-1)), b), c)
    // i.e. (~d | b) ^ c
    let nd = _mm_xor_si128(d, _mm_set1_epi32(-1));
    let or_ = core::arch::x86_64::_mm_or_si128(nd, b);
    let xored = _mm_xor_si128(or_, c);
    _mm_add_epi32(a, xored)
}

// ----------------------------------------------------------------------
// Round body: one `_RX(f, a, b, c, d, ik, r)` step.
//
// Each round takes the current xmm A (called `a`), the current B/C/D,
// the pre-added input word `ik = m[i] + K_i` (or for round 0,
// loaded-from-data + K), and the rotation `r`. It updates `a` in place.
// ----------------------------------------------------------------------

macro_rules! rx {
    // Round-step. `$r` is the MD5 rotation amount (left rotate by r);
    // `$s` must be `32 - r` (precomputed by caller because stable Rust
    // forbids `{ 32 - R }` in const-generic position). The pair is kept
    // explicit so a typo desyncs the test, not the runtime.
    (F, $a:ident, $b:ident, $c:ident, $d:ident, $xx:expr, $r:literal, $s:literal, $k:expr) => {{
        $a = _mm_add_epi32($a, _mm_add_epi32($xx, val($k)));
        $a = addf_f($a, $b, $c, $d);
        $a = rotate::<$s>($a);
        $a = _mm_add_epi32($a, $b);
        let _ = $r; // keep r in source for grep/audit against md5-base.h
    }};
    (G, $a:ident, $b:ident, $c:ident, $d:ident, $xx:expr, $r:literal, $s:literal, $k:expr) => {{
        $a = _mm_add_epi32($a, _mm_add_epi32($xx, val($k)));
        $a = addf_g($a, $b, $c, $d);
        $a = rotate::<$s>($a);
        $a = _mm_add_epi32($a, $b);
        let _ = $r;
    }};
    (H, $a:ident, $b:ident, $c:ident, $d:ident, $xx:expr, $r:literal, $s:literal, $k:expr) => {{
        $a = _mm_add_epi32($a, _mm_add_epi32($xx, val($k)));
        $a = addf_h($a, $b, $c, $d);
        $a = rotate::<$s>($a);
        $a = _mm_add_epi32($a, $b);
        let _ = $r;
    }};
    (I, $a:ident, $b:ident, $c:ident, $d:ident, $xx:expr, $r:literal, $s:literal, $k:expr) => {{
        $a = _mm_add_epi32($a, _mm_add_epi32($xx, val($k)));
        $a = addf_i($a, $b, $c, $d);
        $a = rotate::<$s>($a);
        $a = _mm_add_epi32($a, $b);
        let _ = $r;
    }};
}

/// Process one 64-byte block per lane, MD5x2 SSE2.
///
/// 1:1 expansion of `md5_process_block_x2_sse` (md5x2-base.h ->
/// md5-base.h with the SSE2 macros from md5x2-sse.h substituted).
///
/// # Safety
/// `data1` and `data2` must each be valid for 64-byte reads. They may
/// alias.
#[target_feature(enable = "sse2")]
pub unsafe fn process_block_x2_sse2(state: &mut State, data1: *const u8, data2: *const u8) {
    // Save initial state — added back at the end.
    let oA = state.0[0];
    let oB = state.0[1];
    let oC = state.0[2];
    let oD = state.0[3];

    // Working state.
    let mut A = oA;
    let mut B = oB;
    let mut C = oC;
    let mut D = oD;

    // Load all 16 message words at once via four LOAD4 calls.
    // XX[i] holds message word i for both lanes in 32-bit positions 0 and 2.
    let (XX0, XX1, XX2, XX3) = load4(data1, data2, 0);
    let (XX4, XX5, XX6, XX7) = load4(data1, data2, 4);
    let (XX8, XX9, XX10, XX11) = load4(data1, data2, 8);
    let (XX12, XX13, XX14, XX15) = load4(data1, data2, 12);

    // ----- Round 0 (F) — uses L (load + add k) -----
    // Sequence and constants from md5-base.h:148-174.
    rx!(F, A, B, C, D, XX0, 7, 25, 0xd76aa478);
    rx!(F, D, A, B, C, XX1, 12, 20, 0xe8c7b756);
    rx!(F, C, D, A, B, XX2, 17, 15, 0x242070db);
    rx!(F, B, C, D, A, XX3, 22, 10, 0xc1bdceee);
    rx!(F, A, B, C, D, XX4, 7, 25, 0xf57c0faf);
    rx!(F, D, A, B, C, XX5, 12, 20, 0x4787c62a);
    rx!(F, C, D, A, B, XX6, 17, 15, 0xa8304613);
    rx!(F, B, C, D, A, XX7, 22, 10, 0xfd469501);
    rx!(F, A, B, C, D, XX8, 7, 25, 0x698098d8);
    rx!(F, D, A, B, C, XX9, 12, 20, 0x8b44f7af);
    rx!(F, C, D, A, B, XX10, 17, 15, 0xffff5bb1);
    rx!(F, B, C, D, A, XX11, 22, 10, 0x895cd7be);
    rx!(F, A, B, C, D, XX12, 7, 25, 0x6b901122);
    rx!(F, D, A, B, C, XX13, 12, 20, 0xfd987193);
    rx!(F, C, D, A, B, XX14, 17, 15, 0xa679438e);
    rx!(F, B, C, D, A, XX15, 22, 10, 0x49b40821);

    // ----- Round 1 (G) — md5-base.h:192-207 -----
    rx!(G, A, B, C, D, XX1, 5, 27, 0xf61e2562);
    rx!(G, D, A, B, C, XX6, 9, 23, 0xc040b340);
    rx!(G, C, D, A, B, XX11, 14, 18, 0x265e5a51);
    rx!(G, B, C, D, A, XX0, 20, 12, 0xe9b6c7aa);
    rx!(G, A, B, C, D, XX5, 5, 27, 0xd62f105d);
    rx!(G, D, A, B, C, XX10, 9, 23, 0x02441453);
    rx!(G, C, D, A, B, XX15, 14, 18, 0xd8a1e681);
    rx!(G, B, C, D, A, XX4, 20, 12, 0xe7d3fbc8);
    rx!(G, A, B, C, D, XX9, 5, 27, 0x21e1cde6);
    rx!(G, D, A, B, C, XX14, 9, 23, 0xc33707d6);
    rx!(G, C, D, A, B, XX3, 14, 18, 0xf4d50d87);
    rx!(G, B, C, D, A, XX8, 20, 12, 0x455a14ed);
    rx!(G, A, B, C, D, XX13, 5, 27, 0xa9e3e905);
    rx!(G, D, A, B, C, XX2, 9, 23, 0xfcefa3f8);
    rx!(G, C, D, A, B, XX7, 14, 18, 0x676f02d9);
    rx!(G, B, C, D, A, XX12, 20, 12, 0x8d2a4c8a);

    // ----- Round 2 (H) — md5-base.h:225-240 -----
    rx!(H, A, B, C, D, XX5, 4, 28, 0xfffa3942);
    rx!(H, D, A, B, C, XX8, 11, 21, 0x8771f681);
    rx!(H, C, D, A, B, XX11, 16, 16, 0x6d9d6122);
    rx!(H, B, C, D, A, XX14, 23, 9, 0xfde5380c);
    rx!(H, A, B, C, D, XX1, 4, 28, 0xa4beea44);
    rx!(H, D, A, B, C, XX4, 11, 21, 0x4bdecfa9);
    rx!(H, C, D, A, B, XX7, 16, 16, 0xf6bb4b60);
    rx!(H, B, C, D, A, XX10, 23, 9, 0xbebfbc70);
    rx!(H, A, B, C, D, XX13, 4, 28, 0x289b7ec6);
    rx!(H, D, A, B, C, XX0, 11, 21, 0xeaa127fa);
    rx!(H, C, D, A, B, XX3, 16, 16, 0xd4ef3085);
    rx!(H, B, C, D, A, XX6, 23, 9, 0x04881d05);
    rx!(H, A, B, C, D, XX9, 4, 28, 0xd9d4d039);
    rx!(H, D, A, B, C, XX12, 11, 21, 0xe6db99e5);
    rx!(H, C, D, A, B, XX15, 16, 16, 0x1fa27cf8);
    rx!(H, B, C, D, A, XX2, 23, 9, 0xc4ac5665);

    // ----- Round 3 (I) — md5-base.h:263-278 -----
    // SSE2 path uses IOFFSET = 0 (only AVX/AVX512 set IOFFSET = -1).
    rx!(I, A, B, C, D, XX0, 6, 26, 0xf4292244);
    rx!(I, D, A, B, C, XX7, 10, 22, 0x432aff97);
    rx!(I, C, D, A, B, XX14, 15, 17, 0xab9423a7);
    rx!(I, B, C, D, A, XX5, 21, 11, 0xfc93a039);
    rx!(I, A, B, C, D, XX12, 6, 26, 0x655b59c3);
    rx!(I, D, A, B, C, XX3, 10, 22, 0x8f0ccc92);
    rx!(I, C, D, A, B, XX10, 15, 17, 0xffeff47d);
    rx!(I, B, C, D, A, XX1, 21, 11, 0x85845dd1);
    rx!(I, A, B, C, D, XX8, 6, 26, 0x6fa87e4f);
    rx!(I, D, A, B, C, XX15, 10, 22, 0xfe2ce6e0);
    rx!(I, C, D, A, B, XX6, 15, 17, 0xa3014314);
    rx!(I, B, C, D, A, XX13, 21, 11, 0x4e0811a1);
    rx!(I, A, B, C, D, XX4, 6, 26, 0xf7537e82);
    rx!(I, D, A, B, C, XX11, 10, 22, 0xbd3af235);
    rx!(I, C, D, A, B, XX2, 15, 17, 0x2ad7d2bb);
    rx!(I, B, C, D, A, XX9, 21, 11, 0xeb86d391);

    // Fold the round-final state back onto the saved initial state.
    state.0[0] = _mm_add_epi32(oA, A);
    state.0[1] = _mm_add_epi32(oB, B);
    state.0[2] = _mm_add_epi32(oC, C);
    state.0[3] = _mm_add_epi32(oD, D);
}

/// `Md5x2` backend wrapper for the SSE2 implementation.
pub struct Sse2;

impl crate::parpar_hasher::md5x2::Md5x2 for Sse2 {
    type State = State;

    #[inline(always)]
    fn init_state() -> Self::State {
        // Both lanes initialised to MD5 IV.
        let flat: [u32; 8] = [
            0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0x67452301, 0xefcdab89, 0x98badcfe,
            0x10325476,
        ];
        unsafe { State::from_flat(&flat) }
    }

    #[inline(always)]
    fn init_lane(state: &mut Self::State, lane: usize) {
        // Round-trip through flat form to reset just one lane. Cheap —
        // happens once per ~block_size bytes.
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
        process_block_x2_sse2(state, data1, data2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parpar_hasher::md5x2::Md5x2;
    use crate::parpar_hasher::md5x2_scalar::Scalar;

    /// Hash an N-block message via both backends and assert the resulting
    /// (lane0, lane1) digests match exactly.
    fn cross_check(blocks: &[u8]) {
        assert_eq!(blocks.len() % 64, 0);
        let n = blocks.len() / 64;

        let mut s_state = Scalar::init_state();
        let mut v_state = Sse2::init_state();

        // Feed lane 0 from a forward walk, lane 1 from a reverse walk
        // — using the same 64-byte block on both is too easy a test
        // because the two lanes carry identical state and bugs that
        // mix lanes 0/1 are hidden. Use distinct inputs per lane.
        for i in 0..n {
            let d1 = &blocks[i * 64..i * 64 + 64];
            let d2 = &blocks[(n - 1 - i) * 64..(n - 1 - i) * 64 + 64];
            unsafe {
                Scalar::process_block(&mut s_state, d1.as_ptr(), d2.as_ptr());
                Sse2::process_block(&mut v_state, d1.as_ptr(), d2.as_ptr());
            }
        }

        let s0 = Scalar::extract_lane(&s_state, 0);
        let s1 = Scalar::extract_lane(&s_state, 1);
        let v0 = Sse2::extract_lane(&v_state, 0);
        let v1 = Sse2::extract_lane(&v_state, 1);

        assert_eq!(v0, s0, "lane 0 mismatch after {n} blocks");
        assert_eq!(v1, s1, "lane 1 mismatch after {n} blocks");
    }

    fn synth(len: usize, seed: u64) -> Vec<u8> {
        // xorshift64 — same generator the hasher_input tests use.
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
    fn one_block() {
        cross_check(&synth(64, 1));
    }

    #[test]
    fn two_blocks() {
        cross_check(&synth(128, 2));
    }

    #[test]
    fn many_blocks() {
        cross_check(&synth(64 * 17, 3));
    }

    #[test]
    fn lane_reset_round_trip() {
        let mut state = Sse2::init_state();
        let block = synth(64, 4);
        unsafe {
            Sse2::process_block(&mut state, block.as_ptr(), block.as_ptr());
        }
        // Reset lane 0; lane 1 should be unchanged.
        let lane1_before = Sse2::extract_lane(&state, 1);
        Sse2::init_lane(&mut state, 0);
        let lane1_after = Sse2::extract_lane(&state, 1);
        let lane0_after = Sse2::extract_lane(&state, 0);
        assert_eq!(lane1_before, lane1_after, "init_lane(0) clobbered lane 1");
        let iv: [u8; 16] = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
            0x32, 0x10,
        ];
        assert_eq!(lane0_after, iv, "init_lane(0) didn't restore IV");
    }
}
