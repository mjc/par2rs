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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plane {
    half: ByteHalf,
    bit_from_msb: usize,
    group: usize,
}

impl Plane {
    pub fn new(half: ByteHalf, bit_from_msb: usize, group: usize) -> Self {
        debug_assert!(bit_from_msb < BITS_PER_BYTE);
        debug_assert!(group < GROUPS_PER_BLOCK);
        Self {
            half,
            bit_from_msb,
            group,
        }
    }

    pub fn offset(self) -> usize {
        (self.half.base_mask_index() + self.bit_from_msb * GROUPS_PER_BLOCK + self.group)
            * MASK_BYTES
    }
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

pub fn prepare_avx2(dst: &mut [u8], src: &[u8]) -> usize {
    let prepared_len = src.len().next_multiple_of(AVX2_BLOCK_BYTES);
    assert!(dst.len() >= prepared_len);

    for (block_index, input_block) in source_blocks(src).enumerate() {
        let output_start = block_index * AVX2_BLOCK_BYTES;
        let output_block = prepared_block_mut(dst, output_start);
        prepare_avx2_block(output_block, &input_block);
    }

    prepared_len
}

fn source_blocks(src: &[u8]) -> impl Iterator<Item = [u8; AVX2_BLOCK_BYTES]> + '_ {
    src.chunks(AVX2_BLOCK_BYTES).map(|chunk| {
        let mut block = [0u8; AVX2_BLOCK_BYTES];
        block[..chunk.len()].copy_from_slice(chunk);
        block
    })
}

fn prepared_block_mut(dst: &mut [u8], output_start: usize) -> &mut [u8; AVX2_BLOCK_BYTES] {
    (&mut dst[output_start..output_start + AVX2_BLOCK_BYTES])
        .try_into()
        .expect("prepared block length")
}

pub fn mask_offset(half: ByteHalf, bit_from_msb: usize, group: usize) -> usize {
    Plane::new(half, bit_from_msb, group).offset()
}

pub fn multiply_add_prepared_avx2_block(prepared: &[u8], coefficient: u16, output: &mut [u8]) {
    assert!(prepared.len() >= AVX2_BLOCK_BYTES);
    assert!(output.len() >= AVX2_BLOCK_BYTES);

    for word_lane in WordLane::all() {
        let multiplied = multiply_word(prepared_word(prepared, word_lane), coefficient);
        let result = output_word(output, word_lane) ^ multiplied;
        write_output_word(output, word_lane, result);
    }
}

pub fn multiply_add_prepared_avx2_block_to_prepared(
    prepared: &[u8; AVX2_BLOCK_BYTES],
    coefficient: u16,
    output: &mut [u8; AVX2_BLOCK_BYTES],
) {
    for word_lane in WordLane::all() {
        let multiplied = multiply_word(prepared_word(prepared, word_lane), coefficient);
        let result = prepared_word(output, word_lane) ^ multiplied;
        write_prepared_word(output, word_lane, result);
    }
}

pub fn finish_avx2_block(dst: &mut [u8; AVX2_BLOCK_BYTES], prepared: &[u8; AVX2_BLOCK_BYTES]) {
    for word_lane in WordLane::all() {
        write_output_word(dst, word_lane, prepared_word(prepared, word_lane));
    }
}

fn write_byte_planes(
    dst: &mut [u8; AVX2_BLOCK_BYTES],
    half: ByteHalf,
    group: usize,
    lane: usize,
    byte: u8,
) {
    for bit_from_msb in 0..BITS_PER_BYTE {
        if byte_bit_is_set(byte, bit_from_msb) {
            let offset = Plane::new(half, bit_from_msb, group).offset();
            let mut mask = u32::from_le_bytes(dst[offset..offset + MASK_BYTES].try_into().unwrap());
            mask |= 1 << lane;
            dst[offset..offset + MASK_BYTES].copy_from_slice(&mask.to_le_bytes());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WordLane {
    group: usize,
    lane: usize,
}

impl WordLane {
    fn new(group: usize, lane: usize) -> Self {
        debug_assert!(group < GROUPS_PER_BLOCK);
        debug_assert!(lane < WORDS_PER_GROUP);
        Self { group, lane }
    }

    fn all() -> impl Iterator<Item = Self> {
        (0..GROUPS_PER_BLOCK)
            .flat_map(|group| (0..WORDS_PER_GROUP).map(move |lane| WordLane::new(group, lane)))
    }

    fn byte_offset(self) -> usize {
        (self.group * WORDS_PER_GROUP + self.lane) * 2
    }
}

fn output_word(output: &[u8], word: WordLane) -> u16 {
    let offset = word.byte_offset();
    u16::from_le_bytes([output[offset], output[offset + 1]])
}

fn write_output_word(output: &mut [u8], word: WordLane, value: u16) {
    let offset = word.byte_offset();
    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn prepared_word(prepared: &[u8], word: WordLane) -> u16 {
    let low = prepared_byte(prepared, ByteHalf::Low, word);
    let high = prepared_byte(prepared, ByteHalf::High, word);
    u16::from_le_bytes([low, high])
}

fn write_prepared_word(prepared: &mut [u8], word: WordLane, value: u16) {
    let [low, high] = value.to_le_bytes();
    write_prepared_byte(prepared, ByteHalf::Low, word, low);
    write_prepared_byte(prepared, ByteHalf::High, word, high);
}

fn write_prepared_byte(prepared: &mut [u8], half: ByteHalf, word: WordLane, value: u8) {
    let lane_mask = 1 << word.lane;

    for bit_from_msb in 0..BITS_PER_BYTE {
        let plane = Plane::new(half, bit_from_msb, word.group);
        let mask = read_mask(prepared, plane);
        let next = if byte_bit_is_set(value, bit_from_msb) {
            mask | lane_mask
        } else {
            mask & !lane_mask
        };
        write_mask(prepared, plane, next);
    }
}

fn prepared_byte(prepared: &[u8], half: ByteHalf, word: WordLane) -> u8 {
    (0..BITS_PER_BYTE)
        .filter(|&bit_from_msb| {
            let mask = read_mask(prepared, Plane::new(half, bit_from_msb, word.group));
            mask & (1 << word.lane) != 0
        })
        .fold(0u8, |byte, bit_from_msb| byte | byte_bit_mask(bit_from_msb))
}

fn byte_bit_is_set(value: u8, bit_from_msb: usize) -> bool {
    value & byte_bit_mask(bit_from_msb) != 0
}

fn byte_bit_mask(bit_from_msb: usize) -> u8 {
    0x80 >> bit_from_msb
}

fn read_mask(prepared: &[u8], plane: Plane) -> u32 {
    let offset = plane.offset();
    u32::from_le_bytes(prepared[offset..offset + MASK_BYTES].try_into().unwrap())
}

fn write_mask(prepared: &mut [u8], plane: Plane, value: u32) {
    let offset = plane.offset();
    prepared[offset..offset + MASK_BYTES].copy_from_slice(&value.to_le_bytes());
}

fn multiply_word(mut input: u16, coefficient: u16) -> u16 {
    let mut coeff = coefficient;
    let mut result = 0u16;

    while coeff != 0 {
        if coeff & 1 != 0 {
            result ^= input;
        }
        coeff >>= 1;
        if coeff != 0 {
            let carry = input & 0x8000 != 0;
            input <<= 1;
            if carry {
                input ^= super::GF16_REDUCTION;
            }
        }
    }

    result
}

impl ByteHalf {
    const fn base_mask_index(self) -> usize {
        match self {
            Self::High => 0,
            Self::Low => 64,
        }
    }
}
