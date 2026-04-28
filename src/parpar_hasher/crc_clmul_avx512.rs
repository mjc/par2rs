// Copyright (C) Anomaly Industries Inc. and par2rs contributors.
// SPDX-License-Identifier: GPL-2.0-or-later
//
// AVX-512VL flavour of the 4-fold PCLMULQDQ CRC32 driver. Mirrors the
// `_CRC_USE_AVX512_` branch in `parpar/hasher/crc_clmul.h`: the only
// substantive change vs the SSE4.1 baseline is `double_xor` collapsing
// from two `pxor` instructions into a single `vpternlogd` with truth
// table 0x96 (`a ^ b ^ c`). Everything else — the rk1..rk8 constants,
// the four-fold pipeline shape, the pshufb tail trick — is identical,
// so we reuse [`super::crc_clmul::State`], `init`, and the
// `PSHUFB_SHF_TABLE` from the SSE module.
//
// `HasherInput` selects between this and the SSE baseline at compile
// time via the `Md5x2::USE_AVX512_CRC` associated const, monomorphised
// per backend. There is no separate CRC backend trait.

#![cfg(target_arch = "x86_64")]

use core::arch::x86_64::{
    __m128i, _mm_and_si128, _mm_clmulepi64_si128, _mm_extract_epi32, _mm_load_si128,
    _mm_loadu_si128, _mm_or_si128, _mm_set1_epi8, _mm_set_epi32, _mm_shuffle_epi8, _mm_slli_si128,
    _mm_srli_si128, _mm_ternarylogic_epi32, _mm_xor_si128,
};

use super::crc_clmul::{State, PSHUFB_SHF_TABLE};

/// `vpternlogd` with truth table 0x96 (`a ^ b ^ c`) — replaces the two
/// SSE2 xor instructions with a single AVX-512VL op. See upstream
/// `crc_clmul.h:24`.
#[inline]
#[target_feature(enable = "avx512f,avx512vl")]
unsafe fn double_xor_avx512(a: __m128i, b: __m128i, c: __m128i) -> __m128i {
    _mm_ternarylogic_epi32::<0x96>(a, b, c)
}

#[inline]
#[target_feature(enable = "avx512f,avx512vl,pclmulqdq")]
unsafe fn do_one_fold_merge_avx512(src: __m128i, data: __m128i) -> __m128i {
    let xmm_fold4 = _mm_set_epi32(
        0x00000001,
        0x54442bd4u32 as i32,
        0x00000001,
        0xc6e41596u32 as i32,
    );
    double_xor_avx512(
        _mm_clmulepi64_si128(src, xmm_fold4, 0x01),
        data,
        _mm_clmulepi64_si128(src, xmm_fold4, 0x10),
    )
}

/// Fold one 64-byte block. AVX-512VL flavour of `crc_clmul::process_block`.
///
/// # Safety
/// Caller must have verified `is_x86_feature_detected!("avx512f")`,
/// `"avx512vl"`, and `"pclmulqdq"`. `src` must be valid for 64 readable bytes.
#[inline]
#[target_feature(enable = "avx512f,avx512vl,sse4.1,pclmulqdq")]
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

    core::arch::x86_64::_mm_store_si128(s, do_one_fold_merge_avx512(c0, xmm_t0));
    core::arch::x86_64::_mm_store_si128(s.add(1), do_one_fold_merge_avx512(c1, xmm_t1));
    core::arch::x86_64::_mm_store_si128(s.add(2), do_one_fold_merge_avx512(c2, xmm_t2));
    core::arch::x86_64::_mm_store_si128(s.add(3), do_one_fold_merge_avx512(c3, xmm_t3));
}

/// Finish: absorb `len` (≤ 63) trailing bytes; reduce 4×xmm → 32-bit IEEE CRC.
///
/// AVX-512VL flavour of `crc_clmul::finish`. Uses `vpternlogd`-backed
/// `double_xor` in stages 1 and 3 (rk1/rk2 fold). Stages 4 (rk5/rk6) and
/// 5 (Barrett rk7/rk8) only use single xors and are unchanged.
///
/// # Safety
/// Caller must have verified avx512f + avx512vl + sse4.1 + pclmulqdq.
/// `src` must be valid for `len` readable bytes; `len ≤ 63`.
#[inline]
#[target_feature(enable = "avx512f,avx512vl,sse4.1,pclmulqdq")]
pub unsafe fn finish(state: &mut State, src: *const u8, len: usize) -> u32 {
    debug_assert!(len <= 63);
    let s = state.xmm.as_mut_ptr() as *mut __m128i;
    let mut src = src;
    let mut len = len;

    if len >= 48 {
        let xmm_t0 = _mm_loadu_si128(src as *const __m128i);
        let xmm_t1 = _mm_loadu_si128((src as *const __m128i).add(1));
        let xmm_t2 = _mm_loadu_si128((src as *const __m128i).add(2));

        let c0 = _mm_load_si128(s);
        let c1 = _mm_load_si128(s.add(1));
        let c2 = _mm_load_si128(s.add(2));
        let c3 = _mm_load_si128(s.add(3));

        let new3 = do_one_fold_merge_avx512(c2, xmm_t2);
        let new2 = do_one_fold_merge_avx512(c1, xmm_t1);
        let new1 = do_one_fold_merge_avx512(c0, xmm_t0);

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

        let new3 = do_one_fold_merge_avx512(c1, xmm_t1);
        let new2 = do_one_fold_merge_avx512(c0, xmm_t0);

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

        let new3 = do_one_fold_merge_avx512(c0, xmm_t0);

        core::arch::x86_64::_mm_store_si128(s, c1);
        core::arch::x86_64::_mm_store_si128(s.add(1), c2);
        core::arch::x86_64::_mm_store_si128(s.add(2), c3);
        core::arch::x86_64::_mm_store_si128(s.add(3), new3);
    }
    src = src.add(len & 48);
    len &= 15;

    if len > 0 {
        let shf_row = (PSHUFB_SHF_TABLE.0.as_ptr() as *const __m128i).add(len - 1);
        let xmm_shl = _mm_load_si128(shf_row);
        let xmm_shr = _mm_xor_si128(xmm_shl, _mm_set1_epi8(-128));

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
        let new3 = do_one_fold_merge_avx512(xmm_t1, new3_intermediate);

        core::arch::x86_64::_mm_store_si128(s, new0);
        core::arch::x86_64::_mm_store_si128(s.add(1), new1);
        core::arch::x86_64::_mm_store_si128(s.add(2), new2);
        core::arch::x86_64::_mm_store_si128(s.add(3), new3);
    }

    // Stage 3: reduce four xmm to one (rk1/rk2 fold) — vpternlogd path.
    let crc_fold_rk12 = _mm_set_epi32(
        0x00000001,
        0x751997d0u32 as i32,
        0x00000000,
        0xccaa009eu32 as i32,
    );

    let c0 = _mm_load_si128(s);
    let c1 = _mm_load_si128(s.add(1));
    let c2 = _mm_load_si128(s.add(2));
    let c3 = _mm_load_si128(s.add(3));

    let mut t = double_xor_avx512(
        c1,
        _mm_clmulepi64_si128(c0, crc_fold_rk12, 0x10),
        _mm_clmulepi64_si128(c0, crc_fold_rk12, 0x01),
    );
    t = double_xor_avx512(
        c2,
        _mm_clmulepi64_si128(t, crc_fold_rk12, 0x10),
        _mm_clmulepi64_si128(t, crc_fold_rk12, 0x01),
    );
    t = double_xor_avx512(
        c3,
        _mm_clmulepi64_si128(t, crc_fold_rk12, 0x10),
        _mm_clmulepi64_si128(t, crc_fold_rk12, 0x01),
    );

    // Stage 4: 128 -> 64 fold (rk5/rk6).
    let crc_fold_rk56 = _mm_set_epi32(
        0x00000001,
        0x63cd6124u32 as i32,
        0x00000000,
        0xccaa009eu32 as i32,
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

    // Stage 5: Barrett reduction (rk7/rk8).
    let crc_fold_rk78 = _mm_set_epi32(
        0x00000001,
        0xdb710640u32 as i32,
        0x00000000,
        0xf7011641u32 as i32,
    );

    let xmm_q1 = _mm_clmulepi64_si128(xmm_t0, crc_fold_rk78, 0);
    let xmm_q2 = _mm_clmulepi64_si128(xmm_q1, crc_fold_rk78, 0x10);
    let xmm_xor_mask = _mm_set_epi32(0, -1, -1, 0);
    let xmm_t0_xor = _mm_xor_si128(xmm_t0, xmm_xor_mask);
    let xmm_final = _mm_xor_si128(xmm_q2, xmm_t0_xor);
    _mm_extract_epi32(xmm_final, 2) as u32
}

/// Convenience: detect support once at startup.
#[inline]
pub fn is_supported() -> bool {
    is_x86_feature_detected!("avx512f")
        && is_x86_feature_detected!("avx512vl")
        && is_x86_feature_detected!("pclmulqdq")
        && is_x86_feature_detected!("sse4.1")
}

#[cfg(test)]
mod tests {
    use super::super::crc_clmul;
    use super::*;

    fn avx512_crc(data: &[u8]) -> u32 {
        let mut state = State::new();
        unsafe {
            crc_clmul::init(&mut state);
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

    fn sse_crc(data: &[u8]) -> u32 {
        let mut state = State::new();
        unsafe {
            crc_clmul::init(&mut state);
            let mut p = data.as_ptr();
            let mut remaining = data.len();
            while remaining >= 64 {
                crc_clmul::process_block(&mut state, p);
                p = p.add(64);
                remaining -= 64;
            }
            crc_clmul::finish(&mut state, p, remaining)
        }
    }

    #[test]
    fn avx512_matches_sse_known_vectors() {
        if !is_supported() {
            eprintln!("skipping: host lacks avx512vl + pclmulqdq");
            return;
        }
        for s in [
            &b""[..],
            &b"a"[..],
            &b"abc"[..],
            &b"123456789"[..],
            &b"The quick brown fox jumps over the lazy dog"[..],
        ] {
            assert_eq!(avx512_crc(s), sse_crc(s), "mismatch on {:?}", s);
        }
    }

    #[test]
    fn avx512_matches_sse_short_lengths() {
        if !is_supported() {
            return;
        }
        let pat: Vec<u8> = (0..256u16)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(7))
            .collect();
        for len in 0..=63 {
            assert_eq!(avx512_crc(&pat[..len]), sse_crc(&pat[..len]));
        }
    }

    #[test]
    fn avx512_matches_sse_block_plus_tail() {
        if !is_supported() {
            return;
        }
        let pat: Vec<u8> = (0..8192u32)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(7))
            .collect();
        for &full in &[0usize, 1, 2, 5, 16, 100] {
            for tail in [0usize, 1, 7, 15, 16, 17, 31, 32, 33, 47, 48, 49, 63] {
                let len = full * 64 + tail;
                if len > pat.len() {
                    continue;
                }
                assert_eq!(
                    avx512_crc(&pat[..len]),
                    sse_crc(&pat[..len]),
                    "mismatch full={full} tail={tail}"
                );
            }
        }
    }
}
