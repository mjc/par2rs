// Copyright (C) Anomaly Industries Inc. and par2rs contributors.
// SPDX-License-Identifier: GPL-2.0-or-later
//
// Two-lane MD5 block compression for x86_64 using BMI1's `andn`.
//
// This is a Rust line-for-line port of:
//   parpar/hasher/md5x2-x86-asm.h (the `_MD5_USE_BMI1_` branches),
//   wired through parpar/hasher/hasher_bmi1.cpp
// from par2cmdline-turbo (https://github.com/animetosho/par2cmdline-turbo)
// and the upstream ParPar (https://github.com/animetosho/ParPar), both
// licensed GPL-2.0-or-later.
//
// Relative to `md5x2_scalar.rs`, this file differs in only the G and I
// (and ROUND_I_LAST) round bodies — F and H are bit-for-bit identical.
// The substitutions track upstream's `#ifdef _MD5_USE_BMI1_` branches:
//
// * ROUND_G:  legacy `(D & B) | (~D & C)` rewritten as
//     `andnl C, D, TMP`   ; TMP = (~D) & C
//     `addl  TMP, A`
//     `movl  D, TMP`
//     `andl  B, TMP`      ; TMP = D & B
//     `addl  TMP, A`
//   The two summands are bitwise-disjoint, so `+` ≡ `|` exactly. Saves a
//   `notl` (and lets the OoO engine schedule two independent `addl`s
//   instead of an `or`-then-add chain).
//
// * ROUND_I / ROUND_I_LAST: legacy `((~D) | B) ^ C` with `+K, +TMP`
//   rewritten via the identity
//     `K + (((~D)|B) ^ C) ≡ (K-1) - (((~B)&D) ^ C)   (mod 2^32)`
//   (because `~(~D|B) = D & ~B`, and complement-pairs sum to -1; XORing
//   each with C preserves the complement relationship). This saves a
//   `notl` and removes the `or` from the dep chain:
//     `addl $K-1, A`
//     `andnl D, B, TMP`   ; TMP = (~B) & D
//     `xorl  C, TMP`
//     `subl  TMP, A`      ; A += K - 1 - TMP, equivalent to legacy formula
//
// The function is gated by `#[target_feature(enable = "bmi1")]`. Callers
// MUST verify BMI1 support (e.g. `is_x86_feature_detected!("bmi1")`)
// before invoking, otherwise behaviour is undefined.

#![cfg(target_arch = "x86_64")]

use core::arch::asm;

/// Process one 64-byte block for each of two MD5 lanes, using BMI1
/// `andn` in the G and I round bodies.
///
/// `state[0..4]` is lane 1 (A1, B1, C1, D1); `state[4..8]` is lane 2.
///
/// # Safety
/// * `data1` and `data2` must each be valid for 64-byte reads (may overlap).
/// * The CPU must support BMI1. Verify with `is_x86_feature_detected!("bmi1")`.
#[target_feature(enable = "bmi1")]
#[inline]
pub unsafe fn process_block_x2_bmi1(state: &mut [u32; 8], data1: *const u8, data2: *const u8) {
    let mut a1 = state[0];
    let mut b1 = state[1];
    let mut c1 = state[2];
    let mut d1 = state[3];
    let mut a2 = state[4];
    let mut b2 = state[5];
    let mut c2 = state[6];
    let mut d2 = state[7];

    // Pre-add input word 0 into A (mirrors upstream's pre-loop fold-in).
    a1 = a1.wrapping_add(read32(data1, 0));
    a2 = a2.wrapping_add(read32(data2, 0));

    // ---- F: identical to scalar ---------------------------------------
    macro_rules! round_f {
        ($a1:ident, $b1:ident, $c1:ident, $d1:ident,
         $a2:ident, $b2:ident, $c2:ident, $d2:ident,
         $i_off:expr, $k:expr, $r:expr) => {
            asm!(
                "movl {d1:e}, {tmp1:e}",
                "movl {d2:e}, {tmp2:e}",
                "addl ${k}, {a1:e}",
                "addl ${k}, {a2:e}",
                "xorl {c1:e}, {tmp1:e}",
                "xorl {c2:e}, {tmp2:e}",
                "andl {b1:e}, {tmp1:e}",
                "andl {b2:e}, {tmp2:e}",
                "xorl {d1:e}, {tmp1:e}",
                "xorl {d2:e}, {tmp2:e}",
                "addl {i_off}({base1}), {d1:e}",
                "addl {i_off}({base2}), {d2:e}",
                "addl {tmp1:e}, {a1:e}",
                "addl {tmp2:e}, {a2:e}",
                "roll ${r}, {a1:e}",
                "roll ${r}, {a2:e}",
                "addl {b1:e}, {a1:e}",
                "addl {b2:e}, {a2:e}",
                k = const ($k as u32 as i32),
                r = const $r,
                i_off = const ($i_off * 4),
                a1 = inout(reg) $a1,
                b1 = in(reg) $b1,
                c1 = in(reg) $c1,
                d1 = inout(reg) $d1,
                a2 = inout(reg) $a2,
                b2 = in(reg) $b2,
                c2 = in(reg) $c2,
                d2 = inout(reg) $d2,
                tmp1 = out(reg) _,
                tmp2 = out(reg) _,
                base1 = in(reg) data1,
                base2 = in(reg) data2,
                options(att_syntax, nostack, readonly),
            );
        };
    }

    // ---- G: BMI1 form — andnl C,D,TMP ; addl TMP,A ; movl D,TMP ;
    //                     addl Mi,D ; andl B,TMP ; addl TMP,A ; rol ; +B
    macro_rules! round_g {
        ($a1:ident, $b1:ident, $c1:ident, $d1:ident,
         $a2:ident, $b2:ident, $c2:ident, $d2:ident,
         $i_off:expr, $k:expr, $r:expr) => {
            asm!(
                "addl ${k}, {a1:e}",
                "addl ${k}, {a2:e}",
                // TMP = (~D) & C
                "andnl {c1:e}, {d1:e}, {tmp1:e}",
                "andnl {c2:e}, {d2:e}, {tmp2:e}",
                "addl {tmp1:e}, {a1:e}",
                "addl {tmp2:e}, {a2:e}",
                // TMP = D_old, then D += Mi (input fold), then TMP &= B → D & B
                "movl {d1:e}, {tmp1:e}",
                "movl {d2:e}, {tmp2:e}",
                "addl {i_off}({base1}), {d1:e}",
                "addl {i_off}({base2}), {d2:e}",
                "andl {b1:e}, {tmp1:e}",
                "andl {b2:e}, {tmp2:e}",
                "addl {tmp1:e}, {a1:e}",
                "addl {tmp2:e}, {a2:e}",
                "roll ${r}, {a1:e}",
                "roll ${r}, {a2:e}",
                "addl {b1:e}, {a1:e}",
                "addl {b2:e}, {a2:e}",
                k = const ($k as u32 as i32),
                r = const $r,
                i_off = const ($i_off * 4),
                a1 = inout(reg) $a1,
                b1 = in(reg) $b1,
                c1 = in(reg) $c1,
                d1 = inout(reg) $d1,
                a2 = inout(reg) $a2,
                b2 = in(reg) $b2,
                c2 = in(reg) $c2,
                d2 = inout(reg) $d2,
                tmp1 = out(reg) _,
                tmp2 = out(reg) _,
                base1 = in(reg) data1,
                base2 = in(reg) data2,
                options(att_syntax, nostack, readonly),
            );
        };
    }

    // ---- H: identical to scalar ---------------------------------------
    macro_rules! round_h {
        ($a1:ident, $b1:ident, $c1:ident, $d1:ident,
         $a2:ident, $b2:ident, $c2:ident, $d2:ident,
         $i_off:expr, $k:expr, $r:expr) => {
            asm!(
                "movl {d1:e}, {tmp1:e}",
                "movl {d2:e}, {tmp2:e}",
                "addl ${k}, {a1:e}",
                "addl ${k}, {a2:e}",
                "xorl {c1:e}, {tmp1:e}",
                "xorl {c2:e}, {tmp2:e}",
                "addl {i_off}({base1}), {d1:e}",
                "addl {i_off}({base2}), {d2:e}",
                "xorl {b1:e}, {tmp1:e}",
                "xorl {b2:e}, {tmp2:e}",
                "addl {tmp1:e}, {a1:e}",
                "addl {tmp2:e}, {a2:e}",
                "roll ${r}, {a1:e}",
                "roll ${r}, {a2:e}",
                "addl {b1:e}, {a1:e}",
                "addl {b2:e}, {a2:e}",
                k = const ($k as u32 as i32),
                r = const $r,
                i_off = const ($i_off * 4),
                a1 = inout(reg) $a1,
                b1 = in(reg) $b1,
                c1 = in(reg) $c1,
                d1 = inout(reg) $d1,
                a2 = inout(reg) $a2,
                b2 = in(reg) $b2,
                c2 = in(reg) $c2,
                d2 = inout(reg) $d2,
                tmp1 = out(reg) _,
                tmp2 = out(reg) _,
                base1 = in(reg) data1,
                base2 = in(reg) data2,
                options(att_syntax, nostack, readonly),
            );
        };
    }

    // ---- I: BMI1 form — addl K-1,A ; andnl D,B,TMP ; xorl C,TMP ;
    //                     addl Mi,D ; subl TMP,A ; rol ; +B
    macro_rules! round_i {
        ($a1:ident, $b1:ident, $c1:ident, $d1:ident,
         $a2:ident, $b2:ident, $c2:ident, $d2:ident,
         $i_off:expr, $k:expr, $r:expr) => {
            asm!(
                "addl ${km1}, {a1:e}",
                "addl ${km1}, {a2:e}",
                // TMP = (~B) & D
                "andnl {d1:e}, {b1:e}, {tmp1:e}",
                "andnl {d2:e}, {b2:e}, {tmp2:e}",
                "xorl {c1:e}, {tmp1:e}",
                "xorl {c2:e}, {tmp2:e}",
                "addl {i_off}({base1}), {d1:e}",
                "addl {i_off}({base2}), {d2:e}",
                "subl {tmp1:e}, {a1:e}",
                "subl {tmp2:e}, {a2:e}",
                "roll ${r}, {a1:e}",
                "roll ${r}, {a2:e}",
                "addl {b1:e}, {a1:e}",
                "addl {b2:e}, {a2:e}",
                km1 = const (($k as u32 as i32).wrapping_sub(1)),
                r = const $r,
                i_off = const ($i_off * 4),
                a1 = inout(reg) $a1,
                b1 = in(reg) $b1,
                c1 = in(reg) $c1,
                d1 = inout(reg) $d1,
                a2 = inout(reg) $a2,
                b2 = in(reg) $b2,
                c2 = in(reg) $c2,
                d2 = inout(reg) $d2,
                tmp1 = out(reg) _,
                tmp2 = out(reg) _,
                base1 = in(reg) data1,
                base2 = in(reg) data2,
                options(att_syntax, nostack, readonly),
            );
        };
    }

    // ---- I_LAST: same as I but no input fold into D ------------------
    macro_rules! round_i_last {
        ($a1:ident, $b1:ident, $c1:ident, $d1:ident,
         $a2:ident, $b2:ident, $c2:ident, $d2:ident,
         $k:expr, $r:expr) => {
            asm!(
                "addl ${km1}, {a1:e}",
                "addl ${km1}, {a2:e}",
                "andnl {d1:e}, {b1:e}, {tmp1:e}",
                "andnl {d2:e}, {b2:e}, {tmp2:e}",
                "xorl {c1:e}, {tmp1:e}",
                "xorl {c2:e}, {tmp2:e}",
                "subl {tmp1:e}, {a1:e}",
                "subl {tmp2:e}, {a2:e}",
                "roll ${r}, {a1:e}",
                "roll ${r}, {a2:e}",
                "addl {b1:e}, {a1:e}",
                "addl {b2:e}, {a2:e}",
                km1 = const (($k as u32 as i32).wrapping_sub(1)),
                r = const $r,
                a1 = inout(reg) $a1,
                b1 = in(reg) $b1,
                c1 = in(reg) $c1,
                d1 = in(reg) $d1,
                a2 = inout(reg) $a2,
                b2 = in(reg) $b2,
                c2 = in(reg) $c2,
                d2 = in(reg) $d2,
                tmp1 = out(reg) _,
                tmp2 = out(reg) _,
                options(att_syntax, nostack, readonly),
            );
        };
    }

    // Round 1 (F) — identical schedule to scalar
    round_f!(a1, b1, c1, d1, a2, b2, c2, d2, 1, -0x28955b88_i32, 7);
    round_f!(d1, a1, b1, c1, d2, a2, b2, c2, 2, -0x173848aa_i32, 12);
    round_f!(c1, d1, a1, b1, c2, d2, a2, b2, 3, 0x242070db_i32, 17);
    round_f!(b1, c1, d1, a1, b2, c2, d2, a2, 4, -0x3e423112_i32, 22);

    round_f!(a1, b1, c1, d1, a2, b2, c2, d2, 5, -0x0a83f051_i32, 7);
    round_f!(d1, a1, b1, c1, d2, a2, b2, c2, 6, 0x4787c62a_i32, 12);
    round_f!(c1, d1, a1, b1, c2, d2, a2, b2, 7, -0x57cfb9ed_i32, 17);
    round_f!(b1, c1, d1, a1, b2, c2, d2, a2, 8, -0x02b96aff_i32, 22);

    round_f!(a1, b1, c1, d1, a2, b2, c2, d2, 9, 0x698098d8_i32, 7);
    round_f!(d1, a1, b1, c1, d2, a2, b2, c2, 10, -0x74bb0851_i32, 12);
    round_f!(c1, d1, a1, b1, c2, d2, a2, b2, 11, -0x0000a44f_i32, 17);
    round_f!(b1, c1, d1, a1, b2, c2, d2, a2, 12, -0x76a32842_i32, 22);

    round_f!(a1, b1, c1, d1, a2, b2, c2, d2, 13, 0x6b901122_i32, 7);
    round_f!(d1, a1, b1, c1, d2, a2, b2, c2, 14, -0x02678e6d_i32, 12);
    round_f!(c1, d1, a1, b1, c2, d2, a2, b2, 15, -0x5986bc72_i32, 17);
    round_f!(b1, c1, d1, a1, b2, c2, d2, a2, 1, 0x49b40821_i32, 22);

    // Round 2 (G) — BMI1 form
    round_g!(a1, b1, c1, d1, a2, b2, c2, d2, 6, -0x09e1da9e_i32, 5);
    round_g!(d1, a1, b1, c1, d2, a2, b2, c2, 11, -0x3fbf4cc0_i32, 9);
    round_g!(c1, d1, a1, b1, c2, d2, a2, b2, 0, 0x265e5a51_i32, 14);
    round_g!(b1, c1, d1, a1, b2, c2, d2, a2, 5, -0x16493856_i32, 20);

    round_g!(a1, b1, c1, d1, a2, b2, c2, d2, 10, -0x29d0efa3_i32, 5);
    round_g!(d1, a1, b1, c1, d2, a2, b2, c2, 15, 0x02441453_i32, 9);
    round_g!(c1, d1, a1, b1, c2, d2, a2, b2, 4, -0x275e197f_i32, 14);
    round_g!(b1, c1, d1, a1, b2, c2, d2, a2, 9, -0x182c0438_i32, 20);

    round_g!(a1, b1, c1, d1, a2, b2, c2, d2, 14, 0x21e1cde6_i32, 5);
    round_g!(d1, a1, b1, c1, d2, a2, b2, c2, 3, -0x3cc8f82a_i32, 9);
    round_g!(c1, d1, a1, b1, c2, d2, a2, b2, 8, -0x0b2af279_i32, 14);
    round_g!(b1, c1, d1, a1, b2, c2, d2, a2, 13, 0x455a14ed_i32, 20);

    round_g!(a1, b1, c1, d1, a2, b2, c2, d2, 2, -0x561c16fb_i32, 5);
    round_g!(d1, a1, b1, c1, d2, a2, b2, c2, 7, -0x03105c08_i32, 9);
    round_g!(c1, d1, a1, b1, c2, d2, a2, b2, 12, 0x676f02d9_i32, 14);
    round_g!(b1, c1, d1, a1, b2, c2, d2, a2, 5, -0x72d5b376_i32, 20);

    // Round 3 (H) — identical schedule to scalar
    round_h!(a1, b1, c1, d1, a2, b2, c2, d2, 8, -0x0005c6be_i32, 4);
    round_h!(d1, a1, b1, c1, d2, a2, b2, c2, 11, -0x788e097f_i32, 11);
    round_h!(c1, d1, a1, b1, c2, d2, a2, b2, 14, 0x6d9d6122_i32, 16);
    round_h!(b1, c1, d1, a1, b2, c2, d2, a2, 1, -0x021ac7f4_i32, 23);

    round_h!(a1, b1, c1, d1, a2, b2, c2, d2, 4, -0x5b4115bc_i32, 4);
    round_h!(d1, a1, b1, c1, d2, a2, b2, c2, 7, 0x4bdecfa9_i32, 11);
    round_h!(c1, d1, a1, b1, c2, d2, a2, b2, 10, -0x0944b4a0_i32, 16);
    round_h!(b1, c1, d1, a1, b2, c2, d2, a2, 13, -0x41404390_i32, 23);

    round_h!(a1, b1, c1, d1, a2, b2, c2, d2, 0, 0x289b7ec6_i32, 4);
    round_h!(d1, a1, b1, c1, d2, a2, b2, c2, 3, -0x155ed806_i32, 11);
    round_h!(c1, d1, a1, b1, c2, d2, a2, b2, 6, -0x2b10cf7b_i32, 16);
    round_h!(b1, c1, d1, a1, b2, c2, d2, a2, 9, 0x04881d05_i32, 23);

    round_h!(a1, b1, c1, d1, a2, b2, c2, d2, 12, -0x262b2fc7_i32, 4);
    round_h!(d1, a1, b1, c1, d2, a2, b2, c2, 15, -0x1924661b_i32, 11);
    round_h!(c1, d1, a1, b1, c2, d2, a2, b2, 2, 0x1fa27cf8_i32, 16);
    round_h!(b1, c1, d1, a1, b2, c2, d2, a2, 0, -0x3b53a99b_i32, 23);

    // Round 4 (I) — BMI1 form
    round_i!(a1, b1, c1, d1, a2, b2, c2, d2, 7, -0x0bd6ddbc_i32, 6);
    round_i!(d1, a1, b1, c1, d2, a2, b2, c2, 14, 0x432aff97_i32, 10);
    round_i!(c1, d1, a1, b1, c2, d2, a2, b2, 5, -0x546bdc59_i32, 15);
    round_i!(b1, c1, d1, a1, b2, c2, d2, a2, 12, -0x036c5fc7_i32, 21);

    round_i!(a1, b1, c1, d1, a2, b2, c2, d2, 3, 0x655b59c3_i32, 6);
    round_i!(d1, a1, b1, c1, d2, a2, b2, c2, 10, -0x70f3336e_i32, 10);
    round_i!(c1, d1, a1, b1, c2, d2, a2, b2, 1, -0x00100b83_i32, 15);
    round_i!(b1, c1, d1, a1, b2, c2, d2, a2, 8, -0x7a7ba22f_i32, 21);

    round_i!(a1, b1, c1, d1, a2, b2, c2, d2, 15, 0x6fa87e4f_i32, 6);
    round_i!(d1, a1, b1, c1, d2, a2, b2, c2, 6, -0x01d31920_i32, 10);
    round_i!(c1, d1, a1, b1, c2, d2, a2, b2, 13, -0x5cfebcec_i32, 15);
    round_i!(b1, c1, d1, a1, b2, c2, d2, a2, 4, 0x4e0811a1_i32, 21);

    round_i!(a1, b1, c1, d1, a2, b2, c2, d2, 11, -0x08ac817e_i32, 6);
    round_i!(d1, a1, b1, c1, d2, a2, b2, c2, 2, -0x42c50dcb_i32, 10);
    round_i!(c1, d1, a1, b1, c2, d2, a2, b2, 9, 0x2ad7d2bb_i32, 15);
    round_i_last!(b1, c1, d1, a1, b2, c2, d2, a2, -0x14792c6f_i32, 21);

    state[0] = state[0].wrapping_add(a1);
    state[1] = state[1].wrapping_add(b1);
    state[2] = state[2].wrapping_add(c1);
    state[3] = state[3].wrapping_add(d1);
    state[4] = state[4].wrapping_add(a2);
    state[5] = state[5].wrapping_add(b2);
    state[6] = state[6].wrapping_add(c2);
    state[7] = state[7].wrapping_add(d2);
}

#[inline]
unsafe fn read32(p: *const u8, word_idx: usize) -> u32 {
    let off = word_idx * 4;
    let bytes = core::ptr::read_unaligned(p.add(off) as *const [u8; 4]);
    u32::from_le_bytes(bytes)
}

/// `Md5x2` backend wrapper for the BMI1 implementation. Identical state
/// layout / IVs to the scalar backend; the dispatcher in `hasher_input`
/// chooses this on CPUs that report BMI1 (e.g. Zen3, Skylake+).
pub struct Bmi1;

impl crate::parpar_hasher::md5x2::Md5x2 for Bmi1 {
    type State = [u32; 8];

    #[inline]
    fn init_state() -> Self::State {
        crate::parpar_hasher::md5x2_scalar::init_state()
    }

    #[inline]
    fn init_lane(state: &mut Self::State, lane: usize) {
        crate::parpar_hasher::md5x2_scalar::init_lane(state, lane);
    }

    #[inline]
    fn extract_lane(state: &Self::State, lane: usize) -> [u8; 16] {
        let off = lane * 4;
        let mut out = [0u8; 16];
        for i in 0..4 {
            out[i * 4..i * 4 + 4].copy_from_slice(&state[off + i].to_le_bytes());
        }
        out
    }

    #[inline]
    unsafe fn process_block(state: &mut Self::State, data1: *const u8, data2: *const u8) {
        // Caller must verify BMI1 support before instantiating
        // HasherInput<Bmi1>; the runtime dispatcher in hasher_input does
        // this via is_x86_feature_detected!. Direct construction of
        // HasherInput<Bmi1> on a non-BMI1 CPU is UB, mirroring the
        // upstream contract for HasherInput_BMI1.
        process_block_x2_bmi1(state, data1, data2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parpar_hasher::md5x2_scalar::process_block_x2_scalar;

    /// Cross-check: compress the same two messages with scalar and BMI1;
    /// every state word must match. This is the strongest test we have —
    /// scalar already passes its own RFC 1321 oracle test, so equality
    /// here proves BMI1 is correct.
    #[test]
    fn cross_check_against_scalar() {
        if !std::is_x86_feature_detected!("bmi1") {
            return; // skip on non-BMI1 hosts
        }

        // Deterministic varied inputs; 17 blocks per lane to exercise
        // multi-block accumulation through the state array.
        let mut buf1 = [0u8; 64 * 17];
        let mut buf2 = [0u8; 64 * 17];
        for (i, b) in buf1.iter_mut().enumerate() {
            *b = (i.wrapping_mul(31) ^ 0xa5) as u8;
        }
        for (i, b) in buf2.iter_mut().enumerate() {
            *b = (i.wrapping_mul(17) ^ 0x5a) as u8;
        }

        let mut s_scalar = crate::parpar_hasher::md5x2_scalar::init_state();
        let mut s_bmi1 = crate::parpar_hasher::md5x2_scalar::init_state();
        for blk in 0..17 {
            unsafe {
                let p1 = buf1.as_ptr().add(blk * 64);
                let p2 = buf2.as_ptr().add(blk * 64);
                process_block_x2_scalar(&mut s_scalar, p1, p2);
                process_block_x2_bmi1(&mut s_bmi1, p1, p2);
            }
            assert_eq!(s_scalar, s_bmi1, "state divergence at block {blk}");
        }
    }

    /// Single-block sanity check: known vector "abc"-padded.
    #[test]
    fn known_vector_abc() {
        if !std::is_x86_feature_detected!("bmi1") {
            return;
        }
        let mut block = [0u8; 64];
        block[0] = b'a';
        block[1] = b'b';
        block[2] = b'c';
        block[3] = 0x80; // padding bit
                         // 24-bit (3-byte) length in little-endian at the end
        block[56] = 24;

        let mut s_scalar = crate::parpar_hasher::md5x2_scalar::init_state();
        let mut s_bmi1 = crate::parpar_hasher::md5x2_scalar::init_state();
        unsafe {
            process_block_x2_scalar(&mut s_scalar, block.as_ptr(), block.as_ptr());
            process_block_x2_bmi1(&mut s_bmi1, block.as_ptr(), block.as_ptr());
        }
        assert_eq!(s_scalar, s_bmi1);
    }
}
