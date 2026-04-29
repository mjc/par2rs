// Copyright (C) Anomaly Industries Inc. and par2rs contributors.
// SPDX-License-Identifier: GPL-2.0-or-later
//
// Portable fused per-file HasherInput driver — block-MD5 + file-MD5 + block-CRC32.
//
// Software fallback equivalent of hasher_input.rs, using:
//   * md5x2_scalar for both MD5 lanes (portable, always-available)
//   * crc32fast Hasher for streaming CRC32
//
// The staggered-offset algorithm, md5_final_block, and crc_zero_pad are
// identical to the x86_64 version — only the CRC accumulator type differs.
//
// aarch64 uses this as its default implementation today. x86_64 also uses it
// as the safe fallback when CLMUL CRC prerequisites are unavailable.

use super::md5x2::Md5x2;
use crc32fast::Hasher as CrcHasher;

/// Default backend on aarch64 — portable scalar MD5x2.
pub type DefaultBackend = super::md5x2_scalar::Scalar;

const MD5_BLOCKSIZE: usize = 64;

/// Per-block output: block MD5 and block CRC32.
/// Field names match hasher_input.rs for architecture-agnostic consumers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockHash {
    pub md5: [u8; 16],
    pub crc32: u32,
}

/// Fused hasher state for one source file.
pub struct HasherInput<B: Md5x2 = DefaultBackend> {
    md5_state: B::State,
    crc_hasher: CrcHasher,
    tmp: [u8; 128],
    tmp_len: usize,
    pos_offset: usize,
    data_len_block: u64,
    data_len_file: u64,
}

impl<B: Md5x2> HasherInput<B> {
    #[inline]
    pub fn new() -> Self {
        let mut h = Self {
            md5_state: B::init_state(),
            crc_hasher: CrcHasher::new(),
            tmp: [0; 128],
            tmp_len: 0,
            pos_offset: 0,
            data_len_block: 0,
            data_len_file: 0,
        };
        h.reset();
        h
    }

    #[inline]
    pub fn reset(&mut self) {
        self.md5_state = B::init_state();
        self.crc_hasher = CrcHasher::new();
        self.tmp_len = 0;
        self.pos_offset = 0;
        self.data_len_block = 0;
        self.data_len_file = 0;
    }

    pub fn update(&mut self, data: &[u8]) {
        let len_in = data.len() as u64;
        self.data_len_block += len_in;
        self.data_len_file += len_in;

        let mut data_ = data.as_ptr();
        let mut len = data.len();

        // Drain buffered bytes in tmp.
        if self.tmp_len != 0 {
            loop {
                debug_assert!(self.tmp_len >= self.pos_offset);
                let mut wanted = MD5_BLOCKSIZE + self.pos_offset - self.tmp_len;

                if len < wanted {
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

                let block_data: *const u8 = if self.tmp_len <= self.pos_offset {
                    wanted = MD5_BLOCKSIZE - self.tmp_len;
                    data_
                } else {
                    unsafe { self.tmp.as_ptr().add(self.pos_offset) }
                };

                unsafe {
                    core::ptr::copy_nonoverlapping(
                        data_,
                        self.tmp.as_mut_ptr().add(self.tmp_len),
                        wanted,
                    );
                    let block_slice = core::slice::from_raw_parts(block_data, MD5_BLOCKSIZE);
                    self.crc_hasher.update(block_slice);
                    B::process_block(&mut self.md5_state, block_data, self.tmp.as_ptr());
                }
                len -= wanted;
                data_ = unsafe { data_.add(wanted) };

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
        }

        // Steady-state: process full blocks straight from data.
        while len >= MD5_BLOCKSIZE + self.pos_offset {
            unsafe {
                let block_ptr = data_.add(self.pos_offset);
                let block_slice = core::slice::from_raw_parts(block_ptr, MD5_BLOCKSIZE);
                self.crc_hasher.update(block_slice);
                B::process_block(&mut self.md5_state, block_ptr, data_);
            }
            data_ = unsafe { data_.add(MD5_BLOCKSIZE) };
            len -= MD5_BLOCKSIZE;
        }

        // Stash the tail.
        unsafe {
            core::ptr::copy_nonoverlapping(data_, self.tmp.as_mut_ptr(), len);
        }
        self.tmp_len = len;
    }

    pub fn get_block(&mut self, zero_pad: u64) -> BlockHash {
        let mut md5_out = B::extract_lane(&self.md5_state, 0);
        let block_tail_start = self.pos_offset;
        let block_tail_len = (self.data_len_block & 63) as usize;
        md5_final_block(
            &mut md5_out,
            &self.tmp[block_tail_start..block_tail_start + block_tail_len],
            self.data_len_block,
            zero_pad,
        );

        // CRC: feed the partial tail, finalize, then GF(2)-extend for zero_pad.
        let tail = &self.tmp[block_tail_start..block_tail_start + block_tail_len];
        let mut crc_final = self.crc_hasher.clone();
        crc_final.update(tail);
        let crc_partial = crc_final.finalize();
        let crc = crc_zero_pad(crc_partial, zero_pad);

        if self.tmp_len >= MD5_BLOCKSIZE {
            unsafe {
                B::process_block(&mut self.md5_state, self.tmp.as_ptr(), self.tmp.as_ptr());
            }
            self.tmp_len -= MD5_BLOCKSIZE;
            unsafe {
                core::ptr::copy(
                    self.tmp.as_ptr().add(MD5_BLOCKSIZE),
                    self.tmp.as_mut_ptr(),
                    self.tmp_len,
                );
            }
        }

        B::init_lane(&mut self.md5_state, 0);
        self.crc_hasher = CrcHasher::new(); // Reset CRC for next block
        self.pos_offset = self.tmp_len;
        self.data_len_block = 0;

        BlockHash {
            md5: md5_out,
            crc32: crc,
        }
    }

    pub fn end(mut self) -> [u8; 16] {
        if self.tmp_len >= MD5_BLOCKSIZE {
            unsafe {
                B::process_block(&mut self.md5_state, self.tmp.as_ptr(), self.tmp.as_ptr());
            }
        }
        let mut md5_out = B::extract_lane(&self.md5_state, 1);
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

impl<B: Md5x2> Default for HasherInput<B> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// md5_final_block, md5_compress_one, crc_zero_pad
// (Copied from hasher_input.rs — platform-independent pure Rust)
// ============================================================================

fn md5_final_block(state: &mut [u8; 16], tail_data: &[u8], total_length: u64, zero_pad: u64) {
    let mut block = [0u8; 64];
    let mut remaining = (total_length & 63) as usize;
    debug_assert_eq!(remaining, tail_data.len());
    block[..remaining].copy_from_slice(tail_data);

    let total_length = total_length + zero_pad;
    let mut zero_pad = zero_pad;
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

        md5_compress_one(&mut md5_words, &block);

        if loop_state == 4 {
            break;
        } else if loop_state == 3 {
            loop_state = 4;
        } else if loop_state == 1 {
            zero_pad -= 64;
        } else if loop_state == 0 {
            block.fill(0);
            zero_pad -= 64 - (remaining as u64);
            loop_state = 1;
        }
    }

    for i in 0..4 {
        state[i * 4..i * 4 + 4].copy_from_slice(&md5_words[i].to_le_bytes());
    }
}

#[inline]
fn pack_state(out: &mut [u32; 4], bytes: &[u8; 16]) {
    for i in 0..4 {
        out[i] = u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
    }
}

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

fn crc_multiply(mut a: u32, mut b: u32) -> u32 {
    let mut res = 0u32;
    for _ in 0..31 {
        let mask = ((b >> 31) as i32).wrapping_neg() as u32;
        res ^= mask & a;
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
