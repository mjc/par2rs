// Copyright (C) Anomaly Industries Inc. and par2rs contributors.
// SPDX-License-Identifier: GPL-2.0-or-later
//
// Fused per-file HasherInput driver — block-MD5 + file-MD5 + block-CRC32
// from a single 64-byte cache-line read.
//
// Direct Rust port of:
//   parpar/hasher/hasher_input_base.h    (the driver: update / getBlock / end)
//   parpar/hasher/hasher_input.cpp       (the per-impl instantiation glue)
//   parpar/hasher/md5-final.c            (md5_final_block tail finalisation)
//   parpar/hasher/crc_zeropad.c          (crc_zeroPad GF(2) multiply)
// from par2cmdline-turbo / ParPar (https://github.com/animetosho/ParPar),
// both GPL-2.0-or-later. par2rs is GPL-2.0-or-later, licenses compatible.
//
// Lane convention (matches upstream HASH2X_BLOCK / HASH2X_FILE):
//
//   * `md5_state[0..4]` = lane 0 = per-block MD5 (reset between blocks)
//   * `md5_state[4..8]` = lane 1 = per-file MD5 (rolls across all blocks)
//
// Staggered offset bookkeeping (the trick that lets two MD5 lanes share
// one walk over the input bytes when they don't start at the same byte):
//
//   * `tmp[0..64]`            — staging area for the file-MD5 lane.
//   * `tmp[pos_offset..pos_offset+64]` — staging area for the block-MD5
//                              lane. `pos_offset` ∈ [0, 63].
//   * After each `get_block`, `pos_offset := tmp_len` so the new block
//     lane's logical start point realigns with where the file lane is.
//
// This is the always-available scalar+CLMul tier (Tier 1): GPR MD5x2 +
// PCLMULQDQ CRC32. Future tiers (SSE2/AVX-512 MD5x2, ARM NEON+PMULL) plug
// in via runtime dispatch on top of this contract.

#![cfg(target_arch = "x86_64")]

use super::{crc_clmul, md5x2_scalar};

const MD5_BLOCKSIZE: usize = 64;

/// Fused hasher state for one source file. Computes file-MD5
/// continuously while emitting per-block (MD5, CRC32) pairs at every
/// `get_block` call.
///
/// Mirrors upstream `HasherInput` (`hasher_input_base.h`).
pub struct HasherInput {
    md5_state: [u32; 8],
    crc_state: crc_clmul::State,
    tmp: [u8; 128],
    tmp_len: usize,
    pos_offset: usize,
    data_len_block: u64,
    data_len_file: u64,
}

/// Output of `get_block`: per-block MD5 and per-block CRC32 (IEEE).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockHash {
    pub md5: [u8; 16],
    pub crc32: u32,
}

impl HasherInput {
    /// Create a new per-file hasher state. Equivalent to upstream
    /// `HasherInput::HasherInput()` (which calls `reset()`).
    #[inline]
    pub fn new() -> Self {
        let mut h = Self {
            md5_state: [0; 8],
            crc_state: crc_clmul::State::new(),
            tmp: [0; 128],
            tmp_len: 0,
            pos_offset: 0,
            data_len_block: 0,
            data_len_file: 0,
        };
        h.reset();
        h
    }

    /// Reset to the post-construction state.
    #[inline]
    pub fn reset(&mut self) {
        self.md5_state = md5x2_scalar::init_state();
        self.tmp_len = 0;
        self.pos_offset = 0;
        // SAFETY: x86_64 with sse2 always available; init touches state only.
        unsafe { crc_clmul::init(&mut self.crc_state) };
        self.data_len_block = 0;
        self.data_len_file = 0;
    }

    /// Feed `data` into the hasher. Mirrors upstream
    /// `HasherInput::update`. Maintains:
    ///
    ///   * file-MD5 (lane 1) over the full byte stream.
    ///   * block-MD5 (lane 0) over bytes since last `get_block`.
    ///   * block-CRC32 over the same window as block-MD5.
    pub fn update(&mut self, data: &[u8]) {
        let len_in = data.len() as u64;
        self.data_len_block += len_in;
        self.data_len_file += len_in;

        let mut data_ = data.as_ptr();
        let mut len = data.len();

        // ---------- Drain anything currently buffered in `tmp`. ----------
        // Upstream loops 1-2 times here. The block lane lives at
        // tmp[pos_offset..], the file lane at tmp[0..]. We only have
        // useful work to do here if there's something staged; otherwise
        // skip straight to the steady-state loop.
        if self.tmp_len != 0 {
            loop {
                debug_assert!(self.tmp_len >= self.pos_offset);
                // Bytes still wanted to complete the *block lane's* slot
                // at tmp[pos_offset..pos_offset+64]. Equivalently, how
                // many more bytes before tmp_len reaches 64+pos_offset.
                let mut wanted = MD5_BLOCKSIZE + self.pos_offset - self.tmp_len;

                if len < wanted {
                    // Not enough new data to complete a block. Stash and
                    // return — caller will feed more later.
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            data_,
                            self.tmp.as_mut_ptr().add(self.tmp_len),
                            len,
                        );
                    }
                    self.tmp_len += len;
                    return;
                }

                // We have enough to process one block.
                //
                // If tmp_len <= pos_offset, the block lane's slot
                // starts past where tmp_len reaches — meaning the block
                // hash will read entirely from incoming `data_`, not
                // from `tmp`. Recompute `wanted` to fill the file
                // lane's slot only (tmp[..64]); pull block-lane bytes
                // straight from data_.
                let block_data: *const u8 = if self.tmp_len <= self.pos_offset {
                    wanted = MD5_BLOCKSIZE - self.tmp_len;
                    data_
                } else {
                    // Block lane fully in tmp.
                    unsafe { self.tmp.as_ptr().add(self.pos_offset) }
                };

                // Append the just-decided `wanted` bytes of new data
                // into tmp at tmp_len so the file lane's tmp[..64] slot
                // is complete by the time we call MD5x2.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        data_,
                        self.tmp.as_mut_ptr().add(self.tmp_len),
                        wanted,
                    );

                    // Process the 64 B block: CRC over the block-lane
                    // window; MD5x2 with block_data on lane 0, tmp[0..64]
                    // on lane 1. (Note upstream argument order:
                    // md5_update_block_x2(state, block_data, tmp) — the
                    // FIRST data arg goes to lane HASH2X_BLOCK = 0 and
                    // the SECOND to HASH2X_FILE = 1.)
                    crc_clmul::process_block(&mut self.crc_state, block_data);
                    md5x2_scalar::process_block_x2_scalar(
                        &mut self.md5_state,
                        block_data,
                        self.tmp.as_ptr(),
                    );
                }
                len -= wanted;
                data_ = unsafe { data_.add(wanted) };

                // If both halves came out of tmp, shift the file-lane
                // half down (tmp[64..64+pos_offset] -> tmp[..pos_offset])
                // so the next iteration sees a clean buffer with only
                // the file-lane bytes carried over.
                if self.tmp_len > self.pos_offset {
                    unsafe {
                        core::ptr::copy(
                            self.tmp.as_ptr().add(MD5_BLOCKSIZE),
                            self.tmp.as_mut_ptr(),
                            self.pos_offset,
                        );
                    }
                    self.tmp_len = self.pos_offset;
                    continue;
                }
                break;
            }
            // Note: we deliberately do NOT reset `self.tmp_len` here.
            // Upstream leaves `tmpLen` stale at this point; the
            // unconditional `tmp_len = len` write at the end of `update`
            // overwrites it. Matches upstream hasher_input_base.h:75-87.
        }

        // ---------- Steady state: process full blocks straight from data. ----------
        // Block lane: data_ + pos_offset; file lane: data_.
        // Need at least 64 + pos_offset bytes to satisfy both lanes.
        while len >= MD5_BLOCKSIZE + self.pos_offset {
            unsafe {
                let block_ptr = data_.add(self.pos_offset);
                crc_clmul::process_block(&mut self.crc_state, block_ptr);
                md5x2_scalar::process_block_x2_scalar(&mut self.md5_state, block_ptr, data_);
            }
            data_ = unsafe { data_.add(MD5_BLOCKSIZE) };
            len -= MD5_BLOCKSIZE;
        }

        // Stash the tail in tmp for next update / get_block / end.
        unsafe {
            core::ptr::copy_nonoverlapping(data_, self.tmp.as_mut_ptr(), len);
        }
        self.tmp_len = len;
    }

    /// Finalise the current block: emit (block-MD5, block-CRC32),
    /// optionally extending it by `zero_pad` virtual zero bytes (PAR2
    /// pads short last blocks up to the slice/block size). Then reset
    /// the block lane + CRC for the next block; the file lane
    /// continues unchanged.
    ///
    /// Mirrors upstream `HasherInput::getBlock`.
    pub fn get_block(&mut self, zero_pad: u64) -> BlockHash {
        // Extract block-MD5 lane state into 16 raw bytes, then run the
        // upstream md5_final_block over tmp[pos_offset..pos_offset + (data_len_block & 63)],
        // adding `zero_pad` virtual zero bytes after the real tail.
        let mut md5_out = extract_lane(&self.md5_state, 0);
        let block_tail_start = self.pos_offset;
        let block_tail_len = (self.data_len_block & 63) as usize;
        md5_final_block(
            &mut md5_out,
            &self.tmp[block_tail_start..block_tail_start + block_tail_len],
            self.data_len_block,
            zero_pad,
        );

        // CRC: finish over the same partial tail, then GF(2)-extend by
        // zero_pad bytes via crc_zeropad.
        let crc_partial = unsafe {
            crc_clmul::finish(
                &mut self.crc_state,
                self.tmp.as_ptr().add(block_tail_start),
                block_tail_len,
            )
        };
        let crc = crc_zero_pad(crc_partial, zero_pad);

        // If tmp_len >= 64, the block-lane consumed less than the
        // file-lane staged. Push that extra block through the file lane
        // only (block lane will be reset immediately after, so its
        // input value here is irrelevant — we can pass tmp for both).
        if self.tmp_len >= MD5_BLOCKSIZE {
            unsafe {
                md5x2_scalar::process_block_x2_scalar(
                    &mut self.md5_state,
                    self.tmp.as_ptr(),
                    self.tmp.as_ptr(),
                );
            }
            self.tmp_len -= MD5_BLOCKSIZE;
            // Shift the carry down: tmp[64..64+tmp_len] -> tmp[..tmp_len].
            unsafe {
                core::ptr::copy(
                    self.tmp.as_ptr().add(MD5_BLOCKSIZE),
                    self.tmp.as_mut_ptr(),
                    self.tmp_len,
                );
            }
        }

        // Reset block lane MD5 + CRC; file lane keeps rolling.
        md5x2_scalar::init_lane(&mut self.md5_state, 0);
        unsafe { crc_clmul::init(&mut self.crc_state) };
        self.pos_offset = self.tmp_len;
        self.data_len_block = 0;

        BlockHash {
            md5: md5_out,
            crc32: crc,
        }
    }

    /// Finalise the file-MD5 lane and return the 16-byte file-level MD5.
    /// Mirrors upstream `HasherInput::end`. Consumes self because no
    /// further operations are valid afterwards.
    pub fn end(mut self) -> [u8; 16] {
        // Defensive: usually getBlock already drained tmp_len < 64.
        if self.tmp_len >= MD5_BLOCKSIZE {
            unsafe {
                md5x2_scalar::process_block_x2_scalar(
                    &mut self.md5_state,
                    self.tmp.as_ptr(),
                    self.tmp.as_ptr(),
                );
            }
            // Upstream doesn't shift here because `end` won't iterate;
            // the remaining tmp bytes for the file lane are always at
            // offset 0 (file lane uses tmp[0..]) but bytes 64..tmp_len
            // are leftover from the block lane's larger window. The
            // file-lane finalise below uses `tmp[0..tmp_len & 63]` as
            // its partial tail, but only the first `data_len_file & 63`
            // bytes are meaningful (md5_final_block uses dataLen). We
            // don't need to shift — the staging buffer already has the
            // right file-lane bytes at offset 0. (Same as upstream:
            // upstream's `end` just calls md5_final_block(md5, tmp,
            // dataLen[FILE], 0) without any shift.)
        }

        let mut md5_out = extract_lane(&self.md5_state, 1);
        let file_tail_len = (self.data_len_file & 63) as usize;
        md5_final_block(
            &mut md5_out,
            &self.tmp[..file_tail_len],
            self.data_len_file,
            0,
        );
        md5_out
    }
}

impl Default for HasherInput {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract one MD5 lane's 4 state words as little-endian bytes — i.e.
/// the standard MD5 digest representation that
/// `md5_final_block` consumes and updates in-place.
#[inline]
fn extract_lane(state: &[u32; 8], lane: usize) -> [u8; 16] {
    let off = lane * 4;
    let mut out = [0u8; 16];
    for i in 0..4 {
        out[i * 4..i * 4 + 4].copy_from_slice(&state[off + i].to_le_bytes());
    }
    out
}

/// Pack a 16-byte MD5 state back into 4 little-endian state words.
#[inline]
fn pack_state(out: &mut [u32; 4], bytes: &[u8; 16]) {
    for i in 0..4 {
        out[i] = u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
    }
}

// ----------------------------------------------------------------------
// md5_final_block — direct port of parpar/hasher/md5-final.c.
//
// Take an in-progress MD5 state (16 raw bytes representing four LE u32
// words), a partial tail (`tail_data`) of length `tail_data.len() < 64`
// already absorbed into `total_length` but not yet pushed through the
// compressor, the running total length (BYTES, not bits — same as
// upstream), and a virtual zero-pad count. Append 0x80, zero-pad to 56
// mod 64, write 64-bit LE bit length, run as many blocks as needed, and
// write the final 16-byte digest back into `state`.
// ----------------------------------------------------------------------
fn md5_final_block(state: &mut [u8; 16], tail_data: &[u8], total_length: u64, zero_pad: u64) {
    let mut block = [0u8; 64];
    let mut remaining = (total_length & 63) as usize;
    debug_assert_eq!(remaining, tail_data.len());
    block[..remaining].copy_from_slice(tail_data);
    // (block[remaining..] is already zero from initialiser.)

    let total_length = total_length + zero_pad;
    let mut zero_pad = zero_pad;
    // loop_state mirrors upstream's funky state machine:
    //   0 -> still draining a multi-block zero-pad with leftover real tail.
    //   1 -> draining further full zero-pad blocks (block already zeroed).
    //   2 -> place the 0x80 sentinel.
    //   3 -> separator block emitted, need a follow-up length-only block.
    //   4 -> final block (with 64-bit LE bit length at the end).
    let mut loop_state: u8 = if (remaining as u64) + zero_pad < 64 {
        2
    } else {
        0
    };

    let mut md5_words = [0u32; 4];
    pack_state(&mut md5_words, state);

    loop {
        if loop_state == 1 && zero_pad < 64 {
            loop_state = 2;
        }
        if loop_state == 2 {
            remaining = (total_length & 63) as usize;
            block[remaining] = 0x80;
            remaining += 1;
            if remaining <= 64 - 8 {
                loop_state = 4;
            } else {
                loop_state = 3;
                remaining = 0;
            }
        }
        if loop_state == 4 {
            for b in &mut block[remaining..64 - 8] {
                *b = 0;
            }
            let bit_len = total_length.wrapping_shl(3);
            block[56..64].copy_from_slice(&bit_len.to_le_bytes());
        }

        // Compress this 64-byte block into md5_words (single-lane MD5).
        md5_compress_one(&mut md5_words, &block);

        if loop_state == 4 {
            break;
        } else if loop_state == 3 {
            loop_state = 4;
        } else if loop_state == 1 {
            zero_pad -= 64;
        } else if loop_state == 0 {
            // We just compressed a block whose contents were the real
            // tail bytes followed by zeros (filling out the first
            // partial block of the zero-pad). Subsequent blocks are
            // pure zeros until we run out of zero_pad.
            block.fill(0);
            zero_pad -= 64 - (remaining as u64);
            loop_state = 1;
        }
    }

    // Pack words back to little-endian bytes.
    for i in 0..4 {
        state[i * 4..i * 4 + 4].copy_from_slice(&md5_words[i].to_le_bytes());
    }
}

/// Single-block, single-lane MD5 compressor (RFC 1321 reference).
/// Used only on the final 1-2 blocks per file/per get_block — not on
/// the hot 64 B steady-state path. Kept here to avoid pulling in the
/// `md-5` crate at runtime.
fn md5_compress_one(state: &mut [u32; 4], block: &[u8; 64]) {
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
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
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

// ----------------------------------------------------------------------
// crc_zero_pad — direct port of parpar/hasher/crc_zeropad.c.
// Multiply (NOT crc) by 2^(8 * zero_pad) over GF(2)/IEEE polynomial,
// using the precomputed `crc_power` table.
// ----------------------------------------------------------------------

fn crc_multiply(mut a: u32, mut b: u32) -> u32 {
    let mut res = 0u32;
    for _ in 0..31 {
        // mask = -((b >> 31) as i32) as u32  -> all-ones if top bit set, else 0
        let mask = ((b >> 31) as i32).wrapping_neg() as u32;
        res ^= mask & a;
        // a = (a >> 1) ^ (0xEDB88320 & -(a & 1))
        let a_lsb_mask = ((a & 1) as i32).wrapping_neg() as u32;
        a = (a >> 1) ^ (0xEDB88320 & a_lsb_mask);
        b = b.wrapping_shl(1);
    }
    let mask = ((b >> 31) as i32).wrapping_neg() as u32;
    res ^= mask & a;
    res
}

const CRC_POWER: [u32; 32] = [
    0x00800000, 0x00008000, 0xedb88320, 0xb1e6b092, 0xa06a2517, 0xed627dae, 0x88d14467, 0xd7bbfe6a,
    0xec447f11, 0x8e7ea170, 0x6427800e, 0x4d47bae0, 0x09fe548f, 0x83852d0f, 0x30362f1a, 0x7b5a9cc3,
    0x31fec169, 0x9fec022a, 0x6c8dedc4, 0x15d6874d, 0x5fde7a4e, 0xbad90e37, 0x2e4e5eef, 0x4eaba214,
    0xa8a472c0, 0x429a969e, 0x148d302a, 0xc40ba6d0, 0xc4e22c3c, 0x40000000, 0x20000000, 0x08000000,
];

fn crc_zero_pad(crc: u32, mut zero_pad: u64) -> u32 {
    let mut crc = !crc;
    let mut power: usize = 0;
    while zero_pad != 0 {
        if zero_pad & 1 != 0 {
            crc = crc_multiply(crc, CRC_POWER[power]);
        }
        zero_pad >>= 1;
        power = (power + 1) & 31;
    }
    !crc
}

// ----------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use md5::Digest as _;
    use md5::Md5;

    /// Naive 3-pass reference: returns (file_md5, [(block_md5, block_crc), ...]).
    fn reference(data: &[u8], block_size: usize) -> ([u8; 16], Vec<([u8; 16], u32)>) {
        let mut file = Md5::new();
        file.update(data);
        let file_md5: [u8; 16] = file.finalize().into();

        let mut blocks = Vec::new();
        let mut off = 0;
        while off < data.len() {
            let end = (off + block_size).min(data.len());
            let real = &data[off..end];
            let mut bm = Md5::new();
            bm.update(real);
            // Zero-pad short last block to block_size.
            let pad = block_size - real.len();
            if pad > 0 {
                let zeros = vec![0u8; pad];
                bm.update(&zeros);
            }
            let bmd5: [u8; 16] = bm.finalize().into();
            let mut bc = crc32fast::Hasher::new();
            bc.update(real);
            if pad > 0 {
                let zeros = vec![0u8; pad];
                bc.update(&zeros);
            }
            let bcrc = bc.finalize();
            blocks.push((bmd5, bcrc));
            off = end;
        }
        (file_md5, blocks)
    }

    fn synth(len: usize, seed: u8) -> Vec<u8> {
        (0..len)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    /// Drive a `HasherInput` through a stream split into arbitrary
    /// chunks, then compare its outputs to the naive reference.
    fn drive(data: &[u8], block_size: usize, chunks: &[usize]) {
        let mut h = HasherInput::new();
        let mut got_blocks = Vec::new();
        let mut written_in_block = 0usize;

        // Walk `data` in irregular chunks (for `update`), and call
        // `get_block` whenever we cross a block boundary.
        let mut cursor = 0;
        let mut chunk_idx = 0usize;

        while cursor < data.len() {
            let chunk_size = if chunks.is_empty() {
                data.len() - cursor
            } else {
                chunks[chunk_idx % chunks.len()]
            };
            let mut piece_size = chunk_size.min(data.len() - cursor);

            // Don't cross a block boundary in a single update — split.
            let block_remaining = block_size - written_in_block;
            if piece_size >= block_remaining {
                piece_size = block_remaining;
            }
            h.update(&data[cursor..cursor + piece_size]);
            cursor += piece_size;
            written_in_block += piece_size;
            if written_in_block == block_size {
                let bh = h.get_block(0);
                got_blocks.push((bh.md5, bh.crc32));
                written_in_block = 0;
            }
            chunk_idx += 1;
        }
        // Tail block: may be short — call get_block with zero-pad to
        // match the reference's "pad to block_size".
        if written_in_block > 0 {
            let pad = (block_size - written_in_block) as u64;
            let bh = h.get_block(pad);
            got_blocks.push((bh.md5, bh.crc32));
        }

        let file_md5 = h.end();
        let (ref_file, ref_blocks) = reference(data, block_size);
        assert_eq!(file_md5, ref_file, "file MD5 mismatch");
        assert_eq!(got_blocks.len(), ref_blocks.len(), "block count mismatch");
        for (i, (g, r)) in got_blocks.iter().zip(ref_blocks.iter()).enumerate() {
            assert_eq!(g.0, r.0, "block {i} MD5 mismatch");
            assert_eq!(g.1, r.1, "block {i} CRC mismatch");
        }
    }

    #[test]
    fn empty() {
        let h = HasherInput::new();
        let file = h.end();
        let mut ref_md5 = Md5::new();
        ref_md5.update([]);
        let ref_md5: [u8; 16] = ref_md5.finalize().into();
        assert_eq!(file, ref_md5);
    }

    #[test]
    fn single_block_aligned() {
        // 1 block exactly, no zero-pad.
        let data = synth(4096, 7);
        drive(&data, 4096, &[]);
    }

    #[test]
    fn multiple_blocks_aligned() {
        let data = synth(4096 * 5, 11);
        drive(&data, 4096, &[]);
    }

    #[test]
    fn multiple_blocks_short_tail() {
        // Last block is shorter than block_size; get_block must zero-pad.
        let data = synth(4096 * 3 + 123, 17);
        drive(&data, 4096, &[]);
    }

    #[test]
    fn small_chunked_updates() {
        let data = synth(4096 * 2 + 50, 23);
        drive(&data, 4096, &[1, 7, 13, 64, 65, 100, 1024]);
    }

    #[test]
    fn one_byte_at_a_time() {
        let data = synth(4096 + 10, 41);
        drive(&data, 4096, &[1]);
    }

    #[test]
    fn chunks_crossing_blocks() {
        // Big chunks that would naturally cross block boundaries — the
        // drive helper splits them at the boundary, but each piece is
        // still large multi-cache-line.
        let data = synth(4096 * 4 + 7, 53);
        drive(&data, 4096, &[1234, 5678, 9999]);
    }

    #[test]
    fn small_block_size() {
        // 64-byte blocks — exercises the staggered-offset code paths
        // heavily (pos_offset cycles through values rapidly).
        let data = synth(64 * 17 + 9, 67);
        drive(&data, 64, &[1, 33, 65, 100]);
    }

    #[test]
    fn unaligned_block_size() {
        // 73-byte blocks (not multiple of 64) — pos_offset takes many
        // distinct values, including the staggered carry case.
        let data = synth(73 * 11 + 19, 71);
        drive(&data, 73, &[1, 5, 60, 73, 200]);
    }

    #[test]
    fn large_buffer() {
        let data = synth(64 * 1024 + 17, 89);
        drive(&data, 4096, &[64 * 1024]);
    }
}
