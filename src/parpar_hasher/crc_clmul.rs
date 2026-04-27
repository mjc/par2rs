// Copyright (C) Anomaly Industries Inc. and par2rs contributors.
// SPDX-License-Identifier: GPL-2.0-or-later
//
// PCLMULQDQ-based CRC32 (IEEE / zlib polynomial 0xEDB88320) using the
// 4-fold parallel folding approach from Intel's white paper:
//   "Fast CRC Computation for Generic Polynomials Using PCLMULQDQ"
//   http://www.intel.com/content/dam/www/public/us/en/documents/white-papers/fast-crc-computation-generic-polynomials-pclmulqdq-paper.pdf
//
// This is a direct Rust port of:
//   parpar/hasher/crc_clmul.h
// from par2cmdline-turbo / ParPar (https://github.com/animetosho/ParPar),
// both GPL-2.0-or-later. Upstream took it from zlib-ng / Intel's zlib
// patch; the magic constants (rk1/rk2/rk5/rk6/rk7/rk8 and the fold
// constant) come straight from that Intel reference implementation.
//
// The state is four xmm registers ([State; 4] = 64 bytes), processed
// 64 B per block — exactly matching the MD5x2 cadence in the create
// path's HasherInput so we can fuse them at one cache-line read.
//
// Initial state: state[0] = 0x9db42487 packed into the low 32 bits of an
// xmm; this constant is the precomputed fold of the 0xFFFFFFFF IEEE
// initial XOR through the four-fold pipeline so the very first data
// block can be folded in raw.
//
// Final output: standard IEEE CRC32 (already XOR'd with 0xFFFFFFFF
// inside the finish reduction).

use core::arch::x86_64::{
    __m128i, _mm_and_si128, _mm_clmulepi64_si128, _mm_cvtsi32_si128, _mm_extract_epi32,
    _mm_load_si128, _mm_loadu_si128, _mm_or_si128, _mm_set1_epi8, _mm_set_epi32, _mm_setzero_si128,
    _mm_shuffle_epi8, _mm_slli_si128, _mm_srli_si128, _mm_xor_si128,
};

/// 64-byte CRC32 fold state.
///
/// Mirrors `__m128i state[4]` from upstream `crc_clmul.h`.
#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct State {
    pub xmm: [u128; 4],
}

impl State {
    /// Standard IEEE CRC32 initial fold state (matches `crc_init_clmul`).
    #[inline]
    pub fn new() -> Self {
        let mut s = Self { xmm: [0; 4] };
        // SAFETY: requires pclmul/sse2 only for the store; these are
        // baseline x86_64 features for the load/store, but we keep
        // initialisation in plain integer code to avoid pulling in any
        // SIMD requirement just to construct a State.
        s.xmm[0] = 0x9db42487u128; // low 32 bits of xmm0
        s
    }
}

impl Default for State {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// `pshufb` shuffle/shift table, mirrors `parpar/hasher/tables.cpp`'s
/// `pshufb_shf_table[60]`. Indexed by `(len - 1)` for `1 <= len <= 15`.
///
/// Each row is 16 bytes (one xmm). The high bit (`0x80`) flagged bytes
/// in `pshufb` produce a zero, which is how upstream implements the
/// combined "shift-left by `len`" / "shift-right by `16 - len`" trick.
#[repr(C, align(16))]
struct ShfTable([u32; 60]);

static PSHUFB_SHF_TABLE: ShfTable = ShfTable([
    0x84838281, 0x88878685, 0x8c8b8a89, 0x008f8e8d, // shl 15 / shr 1
    0x85848382, 0x89888786, 0x8d8c8b8a, 0x01008f8e, // shl 14 / shr 2
    0x86858483, 0x8a898887, 0x8e8d8c8b, 0x0201008f, // shl 13 / shr 3
    0x87868584, 0x8b8a8988, 0x8f8e8d8c, 0x03020100, // shl 12 / shr 4
    0x88878685, 0x8c8b8a89, 0x008f8e8d, 0x04030201, // shl 11 / shr 5
    0x89888786, 0x8d8c8b8a, 0x01008f8e, 0x05040302, // shl 10 / shr 6
    0x8a898887, 0x8e8d8c8b, 0x0201008f, 0x06050403, // shl  9 / shr 7
    0x8b8a8988, 0x8f8e8d8c, 0x03020100, 0x07060504, // shl  8 / shr 8
    0x8c8b8a89, 0x008f8e8d, 0x04030201, 0x08070605, // shl  7 / shr 9
    0x8d8c8b8a, 0x01008f8e, 0x05040302, 0x09080706, // shl  6 / shr 10
    0x8e8d8c8b, 0x0201008f, 0x06050403, 0x0a090807, // shl  5 / shr 11
    0x8f8e8d8c, 0x03020100, 0x07060504, 0x0b0a0908, // shl  4 / shr 12
    0x008f8e8d, 0x04030201, 0x08070605, 0x0c0b0a09, // shl  3 / shr 13
    0x01008f8e, 0x05040302, 0x09080706, 0x0d0c0b0a, // shl  2 / shr 14
    0x0201008f, 0x06050403, 0x0a090807, 0x0e0d0c0b, // shl  1 / shr 15
]);

#[inline]
#[target_feature(enable = "sse2,pclmulqdq")]
unsafe fn double_xor(a: __m128i, b: __m128i, c: __m128i) -> __m128i {
    _mm_xor_si128(_mm_xor_si128(a, b), c)
}

#[inline]
#[target_feature(enable = "sse2,pclmulqdq")]
unsafe fn do_one_fold_merge(src: __m128i, data: __m128i) -> __m128i {
    // `xmm_fold4` packs rk3 (0xc6e41596) in the low 64 bits and rk4
    // (0x54442bd4) in the high 64 bits. These come from upstream's
    // `_mm_set_epi32(0x00000001, 0x54442bd4, 0x00000001, 0xc6e41596)`.
    let xmm_fold4 = _mm_set_epi32(
        0x00000001,
        0x54442bd4u32 as i32,
        0x00000001,
        0xc6e41596u32 as i32,
    );
    double_xor(
        _mm_clmulepi64_si128(src, xmm_fold4, 0x01),
        data,
        _mm_clmulepi64_si128(src, xmm_fold4, 0x10),
    )
}

/// Reset state to the standard IEEE CRC32 initial fold.
///
/// # Safety
/// Caller must have verified `is_x86_feature_detected!("sse2")` (a
/// baseline x86_64 feature) before invocation. `state` must be a valid
/// mutable reference; nothing else is read.
#[inline]
#[target_feature(enable = "sse2")]
pub unsafe fn init(state: &mut State) {
    let xmm0 = _mm_cvtsi32_si128(0x9db42487u32 as i32);
    let zero = _mm_setzero_si128();
    let p = state.xmm.as_mut_ptr() as *mut __m128i;
    core::arch::x86_64::_mm_store_si128(p, xmm0);
    core::arch::x86_64::_mm_store_si128(p.add(1), zero);
    core::arch::x86_64::_mm_store_si128(p.add(2), zero);
    core::arch::x86_64::_mm_store_si128(p.add(3), zero);
}

/// Fold one 64-byte block into the state.
///
/// `src` must point to at least 64 readable bytes. Mirrors
/// `crc_process_block_clmul`.
///
/// # Safety
/// `src` must be valid for a 64-byte read. Caller must have checked
/// `is_x86_feature_detected!("pclmulqdq")` and `"sse4.1"` (for the
/// finish path's `_mm_shuffle_epi8`; the block path itself only needs
/// `pclmulqdq` + `sse2`, but we group features together).
#[inline]
#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
pub unsafe fn process_block(state: &mut State, src: *const u8) {
    let s = state.xmm.as_mut_ptr() as *mut __m128i;
    let p = src as *const __m128i;

    let xmm_t0 = _mm_loadu_si128(p);
    let xmm_t1 = _mm_loadu_si128(p.add(1));
    let xmm_t2 = _mm_loadu_si128(p.add(2));
    let xmm_t3 = _mm_loadu_si128(p.add(3));

    let c0 = _mm_load_si128(s);
    let c1 = _mm_load_si128(s.add(1));
    let c2 = _mm_load_si128(s.add(2));
    let c3 = _mm_load_si128(s.add(3));

    core::arch::x86_64::_mm_store_si128(s, do_one_fold_merge(c0, xmm_t0));
    core::arch::x86_64::_mm_store_si128(s.add(1), do_one_fold_merge(c1, xmm_t1));
    core::arch::x86_64::_mm_store_si128(s.add(2), do_one_fold_merge(c2, xmm_t2));
    core::arch::x86_64::_mm_store_si128(s.add(3), do_one_fold_merge(c3, xmm_t3));
}

/// Finish: absorb `len` (≤ 63) trailing bytes and reduce the four-fold
/// state to a single 32-bit IEEE CRC32. Mirrors `crc_finish_clmul`.
///
/// # Safety
/// `src` must be valid for `len` readable bytes. `len` must be ≤ 63.
#[inline]
#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
pub unsafe fn finish(state: &mut State, src: *const u8, len: usize) -> u32 {
    debug_assert!(len <= 63);
    let s = state.xmm.as_mut_ptr() as *mut __m128i;
    let mut src = src;
    let mut len = len;

    // Stage 1: absorb full 16-byte chunks 48 / 32 / 16 of the tail.
    if len >= 48 {
        let xmm_t0 = _mm_loadu_si128(src as *const __m128i);
        let xmm_t1 = _mm_loadu_si128((src as *const __m128i).add(1));
        let xmm_t2 = _mm_loadu_si128((src as *const __m128i).add(2));

        let c0 = _mm_load_si128(s);
        let c1 = _mm_load_si128(s.add(1));
        let c2 = _mm_load_si128(s.add(2));
        let c3 = _mm_load_si128(s.add(3));

        let new3 = do_one_fold_merge(c2, xmm_t2);
        let new2 = do_one_fold_merge(c1, xmm_t1);
        let new1 = do_one_fold_merge(c0, xmm_t0);

        core::arch::x86_64::_mm_store_si128(s, c3);
        core::arch::x86_64::_mm_store_si128(s.add(1), new1);
        core::arch::x86_64::_mm_store_si128(s.add(2), new2);
        core::arch::x86_64::_mm_store_si128(s.add(3), new3);
    } else if len >= 32 {
        let xmm_t0 = _mm_loadu_si128(src as *const __m128i);
        let xmm_t1 = _mm_loadu_si128((src as *const __m128i).add(1));

        let c0 = _mm_load_si128(s);
        let c1 = _mm_load_si128(s.add(1));
        let c2 = _mm_load_si128(s.add(2));
        let c3 = _mm_load_si128(s.add(3));

        let new3 = do_one_fold_merge(c1, xmm_t1);
        let new2 = do_one_fold_merge(c0, xmm_t0);

        core::arch::x86_64::_mm_store_si128(s, c2);
        core::arch::x86_64::_mm_store_si128(s.add(1), c3);
        core::arch::x86_64::_mm_store_si128(s.add(2), new2);
        core::arch::x86_64::_mm_store_si128(s.add(3), new3);
    } else if len >= 16 {
        let xmm_t0 = _mm_loadu_si128(src as *const __m128i);

        let c0 = _mm_load_si128(s);
        let c1 = _mm_load_si128(s.add(1));
        let c2 = _mm_load_si128(s.add(2));
        let c3 = _mm_load_si128(s.add(3));

        let new3 = do_one_fold_merge(c0, xmm_t0);

        core::arch::x86_64::_mm_store_si128(s, c1);
        core::arch::x86_64::_mm_store_si128(s.add(1), c2);
        core::arch::x86_64::_mm_store_si128(s.add(2), c3);
        core::arch::x86_64::_mm_store_si128(s.add(3), new3);
    }
    src = src.add(len & 48);
    len &= 15;

    // Stage 2: absorb the final 1..=15 byte fragment.
    if len > 0 {
        // Load the (len-1)th row of the shuffle table (16 bytes).
        let shf_row = (PSHUFB_SHF_TABLE.0.as_ptr() as *const __m128i).add(len - 1);
        let xmm_shl = _mm_load_si128(shf_row);
        let xmm_shr = _mm_xor_si128(xmm_shl, _mm_set1_epi8(-128));

        // Zero-padded load of the tail.
        let mut tail_bytes = [0u8; 16];
        core::ptr::copy_nonoverlapping(src, tail_bytes.as_mut_ptr(), len);
        let xmm_t0 = _mm_loadu_si128(tail_bytes.as_ptr() as *const __m128i);

        let c0 = _mm_load_si128(s);
        let c1 = _mm_load_si128(s.add(1));
        let c2 = _mm_load_si128(s.add(2));
        let c3 = _mm_load_si128(s.add(3));

        let xmm_t1 = _mm_shuffle_epi8(c0, xmm_shl);

        let new0 = _mm_or_si128(_mm_shuffle_epi8(c0, xmm_shr), _mm_shuffle_epi8(c1, xmm_shl));
        let new1 = _mm_or_si128(_mm_shuffle_epi8(c1, xmm_shr), _mm_shuffle_epi8(c2, xmm_shl));
        let new2 = _mm_or_si128(_mm_shuffle_epi8(c2, xmm_shr), _mm_shuffle_epi8(c3, xmm_shl));
        let new3_intermediate = _mm_or_si128(
            _mm_shuffle_epi8(c3, xmm_shr),
            _mm_shuffle_epi8(xmm_t0, xmm_shl),
        );
        let new3 = do_one_fold_merge(xmm_t1, new3_intermediate);

        core::arch::x86_64::_mm_store_si128(s, new0);
        core::arch::x86_64::_mm_store_si128(s.add(1), new1);
        core::arch::x86_64::_mm_store_si128(s.add(2), new2);
        core::arch::x86_64::_mm_store_si128(s.add(3), new3);
    }

    // Stage 3: reduce four xmm to one (rk1/rk2 fold).
    let crc_fold_rk12 = _mm_set_epi32(
        0x00000001,
        0x751997d0u32 as i32, // rk2
        0x00000000,
        0xccaa009eu32 as i32, // rk1
    );

    let c0 = _mm_load_si128(s);
    let c1 = _mm_load_si128(s.add(1));
    let c2 = _mm_load_si128(s.add(2));
    let c3 = _mm_load_si128(s.add(3));

    let mut t = double_xor(
        c1,
        _mm_clmulepi64_si128(c0, crc_fold_rk12, 0x10),
        _mm_clmulepi64_si128(c0, crc_fold_rk12, 0x01),
    );
    t = double_xor(
        c2,
        _mm_clmulepi64_si128(t, crc_fold_rk12, 0x10),
        _mm_clmulepi64_si128(t, crc_fold_rk12, 0x01),
    );
    t = double_xor(
        c3,
        _mm_clmulepi64_si128(t, crc_fold_rk12, 0x10),
        _mm_clmulepi64_si128(t, crc_fold_rk12, 0x01),
    );

    // Stage 4: 128 -> 64 fold (rk5/rk6).
    let crc_fold_rk56 = _mm_set_epi32(
        0x00000001,
        0x63cd6124u32 as i32, // rk6
        0x00000000,
        0xccaa009eu32 as i32, // rk5
    );

    let mut xmm_t1 = _mm_xor_si128(
        _mm_clmulepi64_si128(t, crc_fold_rk56, 0),
        _mm_srli_si128(t, 8),
    );

    let mut xmm_t0 = _mm_slli_si128(xmm_t1, 4);
    xmm_t0 = _mm_clmulepi64_si128(xmm_t0, crc_fold_rk56, 0x10);
    let mask = _mm_set_epi32(0, -1, -1, 0);
    xmm_t1 = _mm_and_si128(xmm_t1, mask);
    xmm_t0 = _mm_xor_si128(xmm_t0, xmm_t1);

    // Stage 5: Barrett reduction (rk7/rk8) for the final 32-bit CRC.
    let crc_fold_rk78 = _mm_set_epi32(
        0x00000001,
        0xdb710640u32 as i32, // rk8
        0x00000000,
        0xf7011641u32 as i32, // rk7
    );

    let xmm_q1 = _mm_clmulepi64_si128(xmm_t0, crc_fold_rk78, 0);
    let xmm_q2 = _mm_clmulepi64_si128(xmm_q1, crc_fold_rk78, 0x10);
    // result = NOT(q2 XOR t0) folded with the 0xFFFFFFFF init absorption mask.
    let xmm_xor_mask = _mm_set_epi32(0, -1, -1, 0);
    let xmm_t0_xor = _mm_xor_si128(xmm_t0, xmm_xor_mask);
    let xmm_final = _mm_xor_si128(xmm_q2, xmm_t0_xor);
    _mm_extract_epi32(xmm_final, 2) as u32
}

/// Convenience: detect once at startup, then call this on each block.
#[inline]
pub fn is_supported() -> bool {
    is_x86_feature_detected!("sse4.1") && is_x86_feature_detected!("pclmulqdq")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive the CLMul CRC32 over a buffer the same way the create path
    /// will: 64 B blocks, then a tail.
    fn clmul_crc(data: &[u8]) -> u32 {
        assert!(is_supported(), "test host must have sse4.1 + pclmulqdq");
        let mut state = State::new();
        unsafe {
            init(&mut state);
            let mut p = data.as_ptr();
            let mut remaining = data.len();
            while remaining >= 64 {
                process_block(&mut state, p);
                p = p.add(64);
                remaining -= 64;
            }
            finish(&mut state, p, remaining)
        }
    }

    fn reference_crc(data: &[u8]) -> u32 {
        let mut h = crc32fast::Hasher::new();
        h.update(data);
        h.finalize()
    }

    #[test]
    fn matches_crc32fast_empty() {
        assert_eq!(clmul_crc(&[]), reference_crc(&[]));
    }

    #[test]
    fn matches_crc32fast_short_lengths() {
        // Cover every length 1..=63 so every branch in finish() is hit
        // (1..=15 fragment, 16..=31 + fragment, 32..=47 + fragment,
        // 48..=63 + fragment) plus the no-tail case.
        let pattern: Vec<u8> = (0..256u16)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(7))
            .collect();
        for len in 0..=63 {
            let buf = &pattern[..len];
            assert_eq!(clmul_crc(buf), reference_crc(buf), "mismatch at len={len}");
        }
    }

    #[test]
    fn matches_crc32fast_block_aligned() {
        // Exact multiples of 64.
        let pattern: Vec<u8> = (0..4096u32)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(7))
            .collect();
        for blocks in [1usize, 2, 3, 4, 16, 64] {
            let len = blocks * 64;
            let buf = &pattern[..len];
            assert_eq!(clmul_crc(buf), reference_crc(buf), "mismatch at len={len}");
        }
    }

    #[test]
    fn matches_crc32fast_block_plus_tail() {
        let pattern: Vec<u8> = (0..8192u32)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(7))
            .collect();
        // Lots of (full-blocks, tail) combinations.
        for &full in &[0usize, 1, 2, 5, 16, 100] {
            for tail in [1usize, 7, 15, 16, 17, 31, 32, 33, 47, 48, 49, 63] {
                let len = full * 64 + tail;
                if len > pattern.len() {
                    continue;
                }
                let buf = &pattern[..len];
                assert_eq!(
                    clmul_crc(buf),
                    reference_crc(buf),
                    "mismatch at full={full} tail={tail} (len={len})"
                );
            }
        }
    }

    #[test]
    fn matches_crc32fast_known_vectors() {
        // Sanity vs published CRC32 test vectors.
        assert_eq!(clmul_crc(b""), 0x00000000);
        assert_eq!(clmul_crc(b"a"), 0xe8b7be43);
        assert_eq!(clmul_crc(b"abc"), 0x352441c2);
        assert_eq!(clmul_crc(b"123456789"), 0xcbf43926);
        assert_eq!(
            clmul_crc(b"The quick brown fox jumps over the lazy dog"),
            0x414fa339
        );
    }
}
