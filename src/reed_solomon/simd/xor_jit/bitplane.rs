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
    for group in 0..GROUPS_PER_BLOCK {
        let mut low_masks = [0u32; BITS_PER_BYTE];
        let mut high_masks = [0u32; BITS_PER_BYTE];

        for lane in 0..WORDS_PER_GROUP {
            let word_offset = (group * WORDS_PER_GROUP + lane) * 2;
            accumulate_byte_planes(&mut low_masks, lane, src[word_offset]);
            accumulate_byte_planes(&mut high_masks, lane, src[word_offset + 1]);
        }

        write_plane_group(dst, ByteHalf::High, group, &high_masks);
        write_plane_group(dst, ByteHalf::Low, group, &low_masks);
    }
}

pub fn prepare_avx2(dst: &mut [u8], src: &[u8]) -> usize {
    let prepared_len = src.len().next_multiple_of(AVX2_BLOCK_BYTES);
    assert!(dst.len() >= prepared_len);

    let full_len = src.len() / AVX2_BLOCK_BYTES * AVX2_BLOCK_BYTES;
    for (block_index, input_block) in src[..full_len].chunks_exact(AVX2_BLOCK_BYTES).enumerate() {
        let output_start = block_index * AVX2_BLOCK_BYTES;
        let output_block = prepared_block_mut(dst, output_start);
        prepare_avx2_block(
            output_block,
            input_block.try_into().expect("full input block"),
        );
    }

    if full_len < src.len() {
        let mut block = [0u8; AVX2_BLOCK_BYTES];
        block[..src.len() - full_len].copy_from_slice(&src[full_len..]);
        let output_block = prepared_block_mut(dst, full_len);
        prepare_avx2_block(output_block, &block);
    }

    prepared_len
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
    for group in 0..GROUPS_PER_BLOCK {
        let low_masks = read_plane_group(prepared, ByteHalf::Low, group);
        let high_masks = read_plane_group(prepared, ByteHalf::High, group);

        for lane in 0..WORDS_PER_GROUP {
            let word = WordLane::new(group, lane);
            let low = byte_from_planes(&low_masks, lane);
            let high = byte_from_planes(&high_masks, lane);
            write_output_word(dst, word, u16::from_le_bytes([low, high]));
        }
    }
}

fn accumulate_byte_planes(masks: &mut [u32; BITS_PER_BYTE], lane: usize, byte: u8) {
    let lane_mask = 1u32 << lane;
    for bit_from_msb in 0..BITS_PER_BYTE {
        let bit = ((byte >> (BITS_PER_BYTE - 1 - bit_from_msb)) & 1) as u32;
        masks[bit_from_msb] |= 0u32.wrapping_sub(bit) & lane_mask;
    }
}

fn write_plane_group(
    dst: &mut [u8; AVX2_BLOCK_BYTES],
    half: ByteHalf,
    group: usize,
    masks: &[u32; BITS_PER_BYTE],
) {
    for (bit_from_msb, &mask) in masks.iter().enumerate() {
        write_mask(dst, Plane::new(half, bit_from_msb, group), mask);
    }
}

fn read_plane_group(
    prepared: &[u8; AVX2_BLOCK_BYTES],
    half: ByteHalf,
    group: usize,
) -> [u32; BITS_PER_BYTE] {
    core::array::from_fn(|bit_from_msb| read_mask(prepared, Plane::new(half, bit_from_msb, group)))
}

fn byte_from_planes(masks: &[u32; BITS_PER_BYTE], lane: usize) -> u8 {
    let mut byte = 0u8;
    for (bit_from_msb, &mask) in masks.iter().enumerate() {
        let bit = ((mask >> lane) & 1) as u8;
        byte |= bit << (BITS_PER_BYTE - 1 - bit_from_msb);
    }
    byte
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
        let bit = ((value >> (BITS_PER_BYTE - 1 - bit_from_msb)) & 1) as u32;
        let next = (mask & !lane_mask) | (0u32.wrapping_sub(bit) & lane_mask);
        write_mask(prepared, plane, next);
    }
}

fn prepared_byte(prepared: &[u8], half: ByteHalf, word: WordLane) -> u8 {
    byte_from_planes(
        &read_plane_group_slice(prepared, half, word.group),
        word.lane,
    )
}

fn read_mask(prepared: &[u8], plane: Plane) -> u32 {
    let offset = plane.offset();
    u32::from_le_bytes(prepared[offset..offset + MASK_BYTES].try_into().unwrap())
}

fn write_mask(prepared: &mut [u8], plane: Plane, value: u32) {
    let offset = plane.offset();
    prepared[offset..offset + MASK_BYTES].copy_from_slice(&value.to_le_bytes());
}

fn read_plane_group_slice(prepared: &[u8], half: ByteHalf, group: usize) -> [u32; BITS_PER_BYTE] {
    core::array::from_fn(|bit_from_msb| read_mask(prepared, Plane::new(half, bit_from_msb, group)))
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
