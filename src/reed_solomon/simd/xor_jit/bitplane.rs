#![allow(dead_code)]

pub const AVX2_BLOCK_BYTES: usize = 512;
const WORDS_PER_GROUP: usize = 32;
const GROUPS_PER_BLOCK: usize = 8;
const BITS_PER_BYTE: usize = 8;
const MASK_BYTES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteHalf {
    High,
    Low,
}

pub fn prepare_avx2_block(dst: &mut [u8; AVX2_BLOCK_BYTES], src: &[u8; AVX2_BLOCK_BYTES]) {
    dst.fill(0);

    for group in 0..GROUPS_PER_BLOCK {
        for lane in 0..WORDS_PER_GROUP {
            let word_offset = (group * WORDS_PER_GROUP + lane) * 2;
            write_byte_planes(dst, ByteHalf::Low, group, lane, src[word_offset]);
            write_byte_planes(dst, ByteHalf::High, group, lane, src[word_offset + 1]);
        }
    }
}

pub fn mask_offset(half: ByteHalf, bit_from_msb: usize, group: usize) -> usize {
    debug_assert!(bit_from_msb < BITS_PER_BYTE);
    debug_assert!(group < GROUPS_PER_BLOCK);
    (half.base_mask_index() + bit_from_msb * GROUPS_PER_BLOCK + group) * MASK_BYTES
}

fn write_byte_planes(
    dst: &mut [u8; AVX2_BLOCK_BYTES],
    half: ByteHalf,
    group: usize,
    lane: usize,
    byte: u8,
) {
    for bit_from_msb in 0..BITS_PER_BYTE {
        if byte & (0x80 >> bit_from_msb) != 0 {
            let offset = mask_offset(half, bit_from_msb, group);
            let mut mask = u32::from_le_bytes(dst[offset..offset + MASK_BYTES].try_into().unwrap());
            mask |= 1 << lane;
            dst[offset..offset + MASK_BYTES].copy_from_slice(&mask.to_le_bytes());
        }
    }
}

impl ByteHalf {
    const fn base_mask_index(self) -> usize {
        match self {
            Self::High => 0,
            Self::Low => 64,
        }
    }
}
