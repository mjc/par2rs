// Copyright (C) Anomaly Industries Inc. and par2rs contributors.
// SPDX-License-Identifier: GPL-2.0-or-later
//
// Two-lane scalar (GPR) MD5 block compression for x86_64.
//
// This is a Rust line-for-line port of:
//   parpar/hasher/md5x2-x86-asm.h
// from par2cmdline-turbo (https://github.com/animetosho/par2cmdline-turbo)
// and the upstream ParPar (https://github.com/animetosho/ParPar), both
// licensed GPL-2.0-or-later.
//
// The core trick: each MD5 round operates on independent state, so two
// independent MD5 streams ("lane 1" and "lane 2") can be interleaved on a
// single core and the out-of-order engine schedules them in parallel for
// almost no extra cost vs a single MD5. ParPar uses this so the per-block
// MD5 (lane 1) and the per-file MD5 (lane 2) advance from a single walk
// over the input bytes.
//
// State layout (matches upstream):
//   state[0..4] = lane 1 (A1, B1, C1, D1)
//   state[4..8] = lane 2 (A2, B2, C2, D2)
//
// The asm! blocks below mirror the upstream `__asm__` round bodies one
// for one, with the same instruction order, the same register roles, and
// the same memory operand pattern (input read by base+displacement so it
// stays in memory and doesn't burn registers). AT&T syntax is used so the
// translation reads identically to the upstream.
//
// This file is the always-available x86_64 baseline (no SIMD, no BMI1).
// Faster tiers (SSE2, AVX-512) layer on top via runtime dispatch.

#![cfg(target_arch = "x86_64")]

use core::arch::asm;

/// Process one 64-byte block for each of two MD5 lanes.
///
/// `state[0..4]` is lane 1 (A1, B1, C1, D1); `state[4..8]` is lane 2.
/// `data1` and `data2` must each point to at least 64 readable bytes.
///
/// # Safety
/// `data1` and `data2` must be valid for 64-byte reads. They may overlap
/// (the upstream API is happy to be called with `data1 == data2`, which is
/// how the very first block of a file ends up hashed identically into
/// both lanes via the staggered-offset bookkeeping in `HasherInput`).
#[inline]
pub unsafe fn process_block_x2_scalar(state: &mut [u32; 8], data1: *const u8, data2: *const u8) {
    // Pull state into Rust locals — the asm! blocks operate on these.
    let mut a1 = state[0];
    let mut b1 = state[1];
    let mut c1 = state[2];
    let mut d1 = state[3];
    let mut a2 = state[4];
    let mut b2 = state[5];
    let mut c2 = state[6];
    let mut d2 = state[7];

    // Save initial A for the final fold-in. Upstream does `A1 += read32(_data[0])`
    // up-front before round 0, which folds in input word 0; this is equivalent
    // to absorbing the first input word into A as part of the F round 0 add.
    // We mirror upstream exactly: pre-add input word 0 to each lane's A.
    a1 = a1.wrapping_add(read32(data1, 0));
    a2 = a2.wrapping_add(read32(data2, 0));

    // The macros below translate the upstream ROUND_F / ROUND_G / ROUND_H /
    // ROUND_I_INIT / ROUND_I / ROUND_I_LAST sequences. Each `round_*!`
    // emits one asm! block whose instruction sequence is byte-for-byte the
    // upstream macro body, with %k[X] register operands replaced by Rust
    // template arguments and %[i0]/%[i1] memory operands replaced by
    // `{i_off:e}({base})`-style displacements off the input base pointers.

    // F round: TMP = (B & (C^D)) ^ D ; A = rol(A + F + K + Mi, R) + B
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

    // G round: TMP = (~D & C) + (B & D) added in two stages (non-BMI1 path).
    // Mirrors upstream non-BMI1 ROUND_G.
    macro_rules! round_g {
        ($a1:ident, $b1:ident, $c1:ident, $d1:ident,
         $a2:ident, $b2:ident, $c2:ident, $d2:ident,
         $i_off:expr, $k:expr, $r:expr) => {
            asm!(
                "movl {d1:e}, {tmp1:e}",
                "movl {d2:e}, {tmp2:e}",
                "addl ${k}, {a1:e}",
                "addl ${k}, {a2:e}",
                "notl {tmp1:e}",
                "notl {tmp2:e}",
                "andl {c1:e}, {tmp1:e}",
                "andl {c2:e}, {tmp2:e}",
                "addl {tmp1:e}, {a1:e}",
                "addl {tmp2:e}, {a2:e}",
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

    // H round: TMP = D ^ C ^ B (associative XOR sequence, D updated with input
    // before the final XOR). Upstream notes "can't use H shortcut because D
    // input is updated early".
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

    // I round: TMP_INIT = (~D | B); then TMP ^= C ; A += TMP ; D += Mi ; rol; +B.
    // (non-BMI1 path)
    macro_rules! round_i {
        ($a1:ident, $b1:ident, $c1:ident, $d1:ident,
         $a2:ident, $b2:ident, $c2:ident, $d2:ident,
         $i_off:expr, $k:expr, $r:expr) => {
            asm!(
                "movl {d1:e}, {tmp1:e}",
                "movl {d2:e}, {tmp2:e}",
                "addl ${k}, {a1:e}",
                "addl ${k}, {a2:e}",
                "notl {tmp1:e}",
                "notl {tmp2:e}",
                "orl {b1:e}, {tmp1:e}",
                "orl {b2:e}, {tmp2:e}",
                "xorl {c1:e}, {tmp1:e}",
                "xorl {c2:e}, {tmp2:e}",
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

    // ROUND_I_LAST: same as ROUND_I but without the input add into D
    // (it's the final round, no next input word to fold).
    macro_rules! round_i_last {
        ($a1:ident, $b1:ident, $c1:ident, $d1:ident,
         $a2:ident, $b2:ident, $c2:ident, $d2:ident,
         $k:expr, $r:expr) => {
            asm!(
                "movl {d1:e}, {tmp1:e}",
                "movl {d2:e}, {tmp2:e}",
                "addl ${k}, {a1:e}",
                "addl ${k}, {a2:e}",
                "notl {tmp1:e}",
                "notl {tmp2:e}",
                "orl {b1:e}, {tmp1:e}",
                "orl {b2:e}, {tmp2:e}",
                "xorl {c1:e}, {tmp1:e}",
                "xorl {c2:e}, {tmp2:e}",
                "addl {tmp1:e}, {a1:e}",
                "addl {tmp2:e}, {a2:e}",
                "roll ${r}, {a1:e}",
                "roll ${r}, {a2:e}",
                "addl {b1:e}, {a1:e}",
                "addl {b2:e}, {a2:e}",
                k = const ($k as u32 as i32),
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

    // RF4 / RG4 / RH4 / RI4 expand to four rounds with rotated register roles.

    // Round 1 (F)
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

    // Round 2 (G)
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

    // Round 3 (H)
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

    // Round 4 (I)
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

/// Initialise both lanes of an MD5x2 state to the standard MD5 IV.
#[inline]
pub fn init_state() -> [u32; 8] {
    [
        0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, // lane 1
        0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, // lane 2
    ]
}

/// Reset only one lane of an existing MD5x2 state. Used between blocks
/// for the per-block lane while the per-file lane keeps accumulating.
#[inline]
pub fn init_lane(state: &mut [u32; 8], lane: usize) {
    debug_assert!(lane < 2);
    let off = lane * 4;
    state[off] = 0x67452301;
    state[off + 1] = 0xefcdab89;
    state[off + 2] = 0x98badcfe;
    state[off + 3] = 0x10325476;
}

/// `Md5x2` backend wrapper for the scalar implementation. Used as the
/// portable fallback and as a correctness oracle for the SSE2 backend.
pub struct Scalar;

impl crate::parpar_hasher::md5x2::Md5x2 for Scalar {
    type State = [u32; 8];

    #[inline]
    fn init_state() -> Self::State {
        init_state()
    }

    #[inline]
    fn init_lane(state: &mut Self::State, lane: usize) {
        init_lane(state, lane);
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
        process_block_x2_scalar(state, data1, data2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run our two-lane block compressor over a single 64-byte block of
    /// each of two messages and compare both lanes against a portable
    /// reference MD5 block compressor (transcribed from RFC 1321 below).
    fn check_two_messages(msg1: &[u8; 64], msg2: &[u8; 64]) {
        let mut state = init_state();
        unsafe {
            process_block_x2_scalar(&mut state, msg1.as_ptr(), msg2.as_ptr());
        }

        for (lane, msg) in [msg1, msg2].iter().enumerate() {
            let mut ref_state = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
            md5_compress_one_block_reference(&mut ref_state, msg);

            let our_lane: [u32; 4] = [
                state[lane * 4],
                state[lane * 4 + 1],
                state[lane * 4 + 2],
                state[lane * 4 + 3],
            ];
            assert_eq!(our_lane, ref_state, "lane {lane} mismatch");
        }
    }

    /// Plain portable MD5 block compressor used as the test oracle. This
    /// is RFC 1321 transcribed straight to Rust and is intentionally
    /// independent of our asm! port so a bug in the asm! port can't hide
    /// behind the same bug in the oracle.
    fn md5_compress_one_block_reference(state: &mut [u32; 4], block: &[u8; 64]) {
        const K: [u32; 64] = [
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
        const R: [u32; 64] = [
            7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20,
            5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
            6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
        ];
        let mut m = [0u32; 16];
        for i in 0..16 {
            m[i] = u32::from_le_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let t = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                a.wrapping_add(f)
                    .wrapping_add(K[i])
                    .wrapping_add(m[g])
                    .rotate_left(R[i]),
            );
            a = t;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
    }

    #[test]
    fn zero_blocks() {
        check_two_messages(&[0u8; 64], &[0u8; 64]);
    }

    #[test]
    fn ascending_blocks() {
        let mut a = [0u8; 64];
        let mut b = [0u8; 64];
        for i in 0..64 {
            a[i] = i as u8;
            b[i] = (255 - i) as u8;
        }
        check_two_messages(&a, &b);
    }

    #[test]
    fn distinct_messages() {
        let a = *b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@";
        let b = *b"The quick brown fox jumps over the lazy dog. Pack my box with fi";
        check_two_messages(&a, &b);
    }
}
