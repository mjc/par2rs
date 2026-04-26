//! Allocation-disciplined backend for PAR2 create recovery generation.

use crate::reed_solomon::codec::{
    build_split_mul_table, process_slice_multiply_add, SplitMulTable,
};
use crate::reed_solomon::galois::Galois16;
use crate::reed_solomon::AlignedVec;
use rayon::prelude::*;

#[cfg(target_arch = "x86_64")]
use crate::reed_solomon::simd::{
    detect_simd_support, prepare_avx2_coeff, process_slice_multiply_add_prepared_avx2,
    process_slices_multiply_add_prepared_avx2_x2, Avx2PreparedCoeff, SimdLevel,
};

const DEFAULT_INPUT_BATCH_SIZE: usize = 12;
const TRANSFER_BUFFER_COUNT: usize = 2;

/// Prepared coefficient for one `(recovery output, source input)` pair.
pub struct Gf16Coeff {
    pub value: u16,
    pub split: SplitMulTable,
    #[cfg(target_arch = "x86_64")]
    pub avx2: Option<Avx2PreparedCoeff>,
}

impl Gf16Coeff {
    #[inline]
    fn new(value: u16) -> Self {
        let split = build_split_mul_table(Galois16::new(value));
        #[cfg(target_arch = "x86_64")]
        let avx2 = Some(prepare_avx2_coeff(&split));

        Self {
            value,
            split,
            #[cfg(target_arch = "x86_64")]
            avx2,
        }
    }
}

/// Create-side recovery backend with all hot-path storage owned up front.
pub struct CreateRecoveryBackend {
    pub recovery_exponents: Vec<u16>,
    pub source_count: usize,
    pub chunk_len: usize,
    pub output_chunks: Vec<AlignedVec>,
    pub input_staging: Vec<AlignedVec>,
    pub coeffs: Vec<Gf16Coeff>,
    #[cfg(target_arch = "x86_64")]
    pub batch_coeffs: Vec<Avx2PreparedCoeff>,
    transfer_buffers: Vec<AlignedVec>,
    batch_source_indices: Vec<usize>,
    batch_len: usize,
    #[cfg(target_arch = "x86_64")]
    simd_level: SimdLevel,
}

impl CreateRecoveryBackend {
    pub fn new(
        base_values: &[u16],
        first_recovery_block: u32,
        recovery_count: usize,
        max_chunk_len: usize,
    ) -> Self {
        let source_count = base_values.len();
        let recovery_exponents = (0..recovery_count)
            .map(|offset| (first_recovery_block + offset as u32) as u16)
            .collect::<Vec<_>>();

        let coeffs = recovery_exponents
            .iter()
            .flat_map(|&exponent| {
                base_values
                    .iter()
                    .map(move |&base| Galois16::new(base).pow(exponent).value())
            })
            .map(Gf16Coeff::new)
            .collect::<Vec<_>>();

        #[cfg(target_arch = "x86_64")]
        let batch_coeffs = coeffs
            .iter()
            .filter_map(|coeff| coeff.avx2.clone())
            .collect::<Vec<_>>();

        Self {
            recovery_exponents,
            source_count,
            chunk_len: 0,
            output_chunks: (0..recovery_count)
                .map(|_| AlignedVec::new_zeroed(max_chunk_len))
                .collect(),
            input_staging: (0..DEFAULT_INPUT_BATCH_SIZE)
                .map(|_| AlignedVec::new_zeroed(max_chunk_len))
                .collect(),
            coeffs,
            #[cfg(target_arch = "x86_64")]
            batch_coeffs,
            transfer_buffers: (0..TRANSFER_BUFFER_COUNT)
                .map(|_| AlignedVec::new_zeroed(max_chunk_len))
                .collect(),
            batch_source_indices: vec![0; DEFAULT_INPUT_BATCH_SIZE],
            batch_len: 0,
            #[cfg(target_arch = "x86_64")]
            simd_level: detect_simd_support(),
        }
    }

    #[inline]
    pub fn begin_chunk(&mut self, chunk_len: usize) {
        self.chunk_len = chunk_len;
        self.batch_len = 0;

        debug_assert_eq!(
            self.coeffs.len(),
            self.recovery_exponents.len() * self.source_count
        );
        #[cfg(target_arch = "x86_64")]
        debug_assert_eq!(self.batch_coeffs.len(), self.coeffs.len());
        debug_assert!(self
            .input_staging
            .iter()
            .all(|buffer| (buffer.as_ptr() as usize).is_multiple_of(32)));
        debug_assert!(self
            .output_chunks
            .iter()
            .all(|buffer| chunk_len <= buffer.len()));

        self.output_chunks
            .iter_mut()
            .for_each(|chunk| chunk[..chunk_len].fill(0));
    }

    #[inline]
    pub fn prepare_transfer_buffer(&mut self, ring_index: usize) -> &mut [u8] {
        let idx = ring_index % self.transfer_buffers.len();
        let chunk = &mut self.transfer_buffers[idx][..self.chunk_len];
        chunk.fill(0);
        chunk
    }

    #[inline]
    pub fn add_input(&mut self, source_idx: usize, input_chunk: &[u8]) {
        debug_assert!(source_idx < self.source_count);
        debug_assert_eq!(input_chunk.len(), self.chunk_len);
        debug_assert!(self.batch_len < self.input_staging.len());

        let slot = self.batch_len;
        self.input_staging[slot][..self.chunk_len].copy_from_slice(input_chunk);
        self.batch_source_indices[slot] = source_idx;
        self.batch_len += 1;

        if self.batch_len == self.input_staging.len() {
            self.flush_batch();
        }
    }

    #[inline]
    pub fn add_transfer_input(&mut self, source_idx: usize, ring_index: usize) {
        let idx = ring_index % self.transfer_buffers.len();
        debug_assert!(self.chunk_len <= self.transfer_buffers[idx].len());
        debug_assert!(self.batch_len < self.input_staging.len());

        let slot = self.batch_len;
        self.input_staging[slot][..self.chunk_len]
            .copy_from_slice(&self.transfer_buffers[idx][..self.chunk_len]);
        self.batch_source_indices[slot] = source_idx;
        self.batch_len += 1;

        if self.batch_len == self.input_staging.len() {
            self.flush_batch();
        }
    }

    #[inline]
    pub fn finish_chunk(&mut self, recovery_blocks: &mut [(u16, Vec<u8>)], block_size: usize) {
        self.flush_batch();

        self.output_chunks
            .iter()
            .zip(recovery_blocks.iter_mut())
            .for_each(|(output_chunk, (_, recovery_data))| {
                debug_assert!(recovery_data.capacity() >= block_size);
                debug_assert!(recovery_data.len() + self.chunk_len <= recovery_data.capacity());
                debug_assert!(self.chunk_len <= output_chunk.len());
                recovery_data.extend_from_slice(&output_chunk[..self.chunk_len]);
            });
    }

    #[inline]
    pub fn recovery_blocks(&self, block_size: usize) -> Vec<(u16, Vec<u8>)> {
        self.recovery_exponents
            .iter()
            .map(|&exponent| (exponent, Vec::with_capacity(block_size)))
            .collect()
    }

    #[inline]
    fn flush_batch(&mut self) {
        if self.batch_len == 0 {
            return;
        }

        let chunk_len = self.chunk_len;
        let source_count = self.source_count;
        let batch_len = self.batch_len;
        let coeffs = &self.coeffs;
        let input_staging = &self.input_staging;
        let source_indices = &self.batch_source_indices;
        #[cfg(target_arch = "x86_64")]
        let simd_level = self.simd_level;

        self.output_chunks
            .par_iter_mut()
            .enumerate()
            .for_each(|(recovery_idx, output_chunk)| {
                let output = &mut output_chunk[..chunk_len];
                #[cfg(target_arch = "x86_64")]
                if matches!(simd_level, SimdLevel::Avx2) {
                    Self::process_batch_add_avx2_x2(
                        recovery_idx,
                        source_count,
                        batch_len,
                        input_staging,
                        source_indices,
                        coeffs,
                        output,
                    );
                    return;
                }

                (0..batch_len).for_each(|batch_idx| {
                    let source_idx = source_indices[batch_idx];
                    let coeff = &coeffs[gf_coeff_index(recovery_idx, source_idx, source_count)];
                    let input = &input_staging[batch_idx][..chunk_len];
                    #[cfg(target_arch = "x86_64")]
                    Self::process_input_add(input, output, coeff, simd_level);
                    #[cfg(not(target_arch = "x86_64"))]
                    Self::process_input_add(input, output, coeff);
                });
            });

        self.batch_len = 0;
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    fn process_batch_add_avx2_x2(
        recovery_idx: usize,
        source_count: usize,
        batch_len: usize,
        input_staging: &[AlignedVec],
        source_indices: &[usize],
        coeffs: &[Gf16Coeff],
        output: &mut [u8],
    ) {
        let mut batch_idx = 0;
        while batch_idx + 1 < batch_len {
            let source_a = source_indices[batch_idx];
            let source_b = source_indices[batch_idx + 1];
            let coeff_a = &coeffs[gf_coeff_index(recovery_idx, source_a, source_count)];
            let coeff_b = &coeffs[gf_coeff_index(recovery_idx, source_b, source_count)];

            match (&coeff_a.avx2, &coeff_b.avx2) {
                (Some(prepared_a), Some(prepared_b)) => unsafe {
                    process_slices_multiply_add_prepared_avx2_x2(
                        &input_staging[batch_idx][..output.len()],
                        prepared_a,
                        &coeff_a.split,
                        &input_staging[batch_idx + 1][..output.len()],
                        prepared_b,
                        &coeff_b.split,
                        output,
                    );
                },
                _ => {
                    Self::process_input_add(
                        &input_staging[batch_idx][..output.len()],
                        output,
                        coeff_a,
                        SimdLevel::None,
                    );
                    Self::process_input_add(
                        &input_staging[batch_idx + 1][..output.len()],
                        output,
                        coeff_b,
                        SimdLevel::None,
                    );
                }
            }

            batch_idx += 2;
        }

        if batch_idx < batch_len {
            let source_idx = source_indices[batch_idx];
            let coeff = &coeffs[gf_coeff_index(recovery_idx, source_idx, source_count)];
            Self::process_input_add(
                &input_staging[batch_idx][..output.len()],
                output,
                coeff,
                SimdLevel::Avx2,
            );
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    fn process_input_add(
        input: &[u8],
        output: &mut [u8],
        coeff: &Gf16Coeff,
        simd_level: SimdLevel,
    ) {
        if matches!(simd_level, SimdLevel::Avx2) {
            if let Some(prepared) = &coeff.avx2 {
                unsafe {
                    process_slice_multiply_add_prepared_avx2(input, output, prepared, &coeff.split);
                }
                return;
            }
        }

        process_slice_multiply_add(input, output, &coeff.split);
    }

    #[cfg(not(target_arch = "x86_64"))]
    #[inline]
    fn process_input_add(input: &[u8], output: &mut [u8], coeff: &Gf16Coeff) {
        process_slice_multiply_add(input, output, &coeff.split);
    }
}

#[inline]
pub fn gf_coeff_index(recovery_idx: usize, source_idx: usize, source_count: usize) -> usize {
    recovery_idx * source_count + source_idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reed_solomon::RecoveryBlockEncoder;

    #[test]
    fn backend_output_matches_encoder_for_partial_batch() {
        let block_size = 64;
        let source_count = 5;
        let encoder = RecoveryBlockEncoder::new(block_size, source_count);
        let inputs = (0..source_count)
            .map(|src| {
                (0..block_size)
                    .map(|byte| (src * 17 + byte) as u8)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let mut backend = CreateRecoveryBackend::new(encoder.base_values(), 0, 3, block_size);
        let mut recovery_blocks = backend.recovery_blocks(block_size);
        backend.begin_chunk(block_size);
        inputs
            .iter()
            .enumerate()
            .for_each(|(idx, input)| backend.add_input(idx, input));
        backend.finish_chunk(&mut recovery_blocks, block_size);

        recovery_blocks
            .iter()
            .for_each(|(exponent, recovery_data)| {
                let refs = inputs.iter().map(Vec::as_slice).collect::<Vec<_>>();
                let expected = encoder.encode_recovery_block(*exponent, &refs).unwrap();
                assert_eq!(recovery_data, &expected);
            });
    }

    #[test]
    fn backend_output_matches_encoder_across_multiple_chunks() {
        let block_size = 64;
        let chunk_size = 16;
        let source_count = 4;
        let encoder = RecoveryBlockEncoder::new(block_size, source_count);
        let inputs = (0..source_count)
            .map(|src| {
                (0..block_size)
                    .map(|byte| (src * 17 + byte * 3) as u8)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let mut backend = CreateRecoveryBackend::new(encoder.base_values(), 0, 2, chunk_size);
        let mut recovery_blocks = backend.recovery_blocks(block_size);

        for offset in (0..block_size).step_by(chunk_size) {
            backend.begin_chunk(chunk_size);
            inputs.iter().enumerate().for_each(|(idx, input)| {
                backend.add_input(idx, &input[offset..offset + chunk_size]);
            });
            backend.finish_chunk(&mut recovery_blocks, block_size);
        }

        recovery_blocks
            .iter()
            .for_each(|(exponent, recovery_data)| {
                let refs = inputs.iter().map(Vec::as_slice).collect::<Vec<_>>();
                let expected = encoder.encode_recovery_block(*exponent, &refs).unwrap();
                assert_eq!(recovery_data, &expected);
            });
    }

    #[test]
    fn backend_reuses_fixed_transfer_buffers() {
        let encoder = RecoveryBlockEncoder::new(64, 2);
        let mut backend = CreateRecoveryBackend::new(encoder.base_values(), 7, 1, 64);
        backend.begin_chunk(32);
        let first = backend.prepare_transfer_buffer(0).as_ptr();
        let second = backend.prepare_transfer_buffer(1).as_ptr();
        let first_again = backend.prepare_transfer_buffer(2).as_ptr();

        assert_ne!(first, second);
        assert_eq!(first, first_again);
        assert_eq!(first as usize % 32, 0);
        assert_eq!(second as usize % 32, 0);
    }
}
