// Copyright (C) Anomaly Industries Inc. and par2rs contributors.
// SPDX-License-Identifier: GPL-2.0-or-later
//
// MD5x2 backend trait — the contract every two-lane MD5 implementation
// implements so `HasherInput` can be generic over the SIMD/scalar tier
// chosen at compile or run time.
//
// Upstream's equivalent is the macro `_FN(md5_*)` machinery in
// `parpar/hasher/md5x2-base.h`, which textually re-substitutes the
// active backend's name into the round bodies. Rust uses a trait + a
// concrete impl per backend, which gives us monomorphisation without
// repeating the driver code per backend.

#![cfg(target_arch = "x86_64")]

/// Two-lane MD5 block compressor.
///
/// The state type is an associated type because each backend chooses
/// the layout that's most natural for its instruction set:
///
/// * scalar uses `[u32; 8]` (lanes laid out flat: `[a0,b0,c0,d0,a1,b1,c1,d1]`).
/// * SSE2 uses `[__m128i; 4]` with each register holding both lanes'
///   matching state word in the funky `[lane0, GARBAGE, lane1, GARBAGE]`
///   layout from `md5x2-sse.h`.
///
/// Backends keep the state in the form they prefer, and only convert
/// at the boundaries the driver actually observes (`init_state`,
/// `init_lane`, `extract_lane`).
pub trait Md5x2 {
    /// Backend-private state representation.
    type State;

    /// Whether this backend's host CPU also supports the AVX-512VL
    /// `vpternlogd` form of the CLMul CRC32 fold (`crc_clmul_avx512`).
    ///
    /// Default `false` — the SSE4.1 + PCLMULQDQ baseline is always safe.
    /// Backends that already require AVX-512VL (e.g. the AVX-512 MD5x2
    /// path) override this to `true` so `HasherInput` picks the
    /// `vpternlogd`-collapsed fold without needing a second generic
    /// parameter. Mirrors upstream's coupling: the `_CRC_USE_AVX512_`
    /// switch in `crc_clmul.h` is gated by the same CPU feature set as
    /// the AVX-512 MD5 path, so they're never selected independently.
    const USE_AVX512_CRC: bool = false;

    /// Build a fresh state with both lanes initialised to the standard
    /// MD5 IV.
    fn init_state() -> Self::State;

    /// Reset just one of the two lanes to the standard MD5 IV. Used by
    /// `HasherInput::get_block` to restart the block-MD5 lane while the
    /// file-MD5 lane keeps accumulating.
    fn init_lane(state: &mut Self::State, lane: usize);

    /// Read one lane's current state out as a 16-byte little-endian
    /// MD5 digest. This is the canonical wire form the finalizer
    /// (`md5_final_block`) consumes.
    fn extract_lane(state: &Self::State, lane: usize) -> [u8; 16];

    /// Compress one 64-byte block on each lane.
    ///
    /// `data1` feeds lane 0 (the block-MD5 lane in `HasherInput`).
    /// `data2` feeds lane 1 (the file-MD5 lane).
    ///
    /// # Safety
    /// Both pointers must be valid for 64-byte reads. They may overlap
    /// (the upstream API is happy with `data1 == data2`, which happens
    /// on the very first block of every source file).
    unsafe fn process_block(state: &mut Self::State, data1: *const u8, data2: *const u8);
}
