//! Allocation-disciplined backend for PAR2 create recovery generation.

use crate::reed_solomon::codec::{
    build_split_mul_table, process_slice_multiply_add, SplitMulTable,
};
use crate::reed_solomon::galois::Galois16;
use crate::reed_solomon::AlignedVec;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

#[cfg(target_arch = "x86_64")]
use crate::reed_solomon::simd::{
    detect_simd_support, finish_xor_jit_bitplane_packed_output_cksum, prepare_avx2_coeff,
    prepare_xor_jit_bitplane_packed_input_cksum, process_slice_multiply_add_prepared_avx2,
    process_slice_multiply_add_xor_jit, process_slices_multiply_add_prepared_avx2_x2,
    process_slices_multiply_add_prepared_avx2_x4, process_slices_multiply_add_xor_jit_x2,
    process_slices_multiply_add_xor_jit_x4,
    process_slices_multiply_add_xor_jit_x4_inputs_x2_outputs,
    process_slices_multiply_add_xor_jit_x4_inputs_x4_outputs, xor_packed_multi_region_v16i1_ptr,
    Avx2PreparedCoeff, SimdLevel, XorJitBitplaneScratch, XorJitFlavor, XorJitPreparedCoeff,
    XorJitPreparedCoeffCache,
};

const DEFAULT_INPUT_GROUPING: usize = 12;
const TRANSFER_BUFFER_COUNT: usize = 2;
const CREATE_SEGMENT_SIZE: usize = 256 * 1024;
// The prepared x1 PSHUFB path currently retires fewer instructions on the
// large-file create proxy than the x2/x4 packed kernels on this CPU.
const PSHUFB_PACKED_INPUTS: usize = 1;
const AVX2_ALIGNMENT: usize = 32;
const XOR_JIT_BITPLANE_ALIGNMENT: usize = 512;
#[cfg(target_arch = "x86_64")]
const XOR_JIT_SEGMENT_LEN_ENV: &str = "PAR2RS_CREATE_XOR_JIT_SEGMENT_BYTES";
#[cfg(target_arch = "x86_64")]
const XOR_JIT_PREFETCH_OUTPUT_ROUNDS: usize = 2;
#[cfg(target_arch = "x86_64")]
const XOR_JIT_PREFETCH_DOWNSCALE: usize = 1;
#[cfg(target_arch = "x86_64")]
const XOR_JIT_PREFETCH_SPREAD_SHIFT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateGf16Method {
    Auto,
    Avx2PshufbPrepared,
    Avx2XorJit,
    Scalar,
}

impl CreateGf16Method {
    fn from_env() -> Self {
        match std::env::var("PAR2RS_CREATE_GF16") {
            Ok(value) => match value.to_ascii_lowercase().as_str() {
                "auto" => Self::Auto,
                "pshufb" | "avx2-pshufb" | "avx2_pshufb" => Self::Avx2PshufbPrepared,
                "xor-jit" | "xor_jit" | "xorit" | "avx2-xor-jit" | "xor-jit-port"
                | "xor_jit_port" | "avx2-xor-jit-port" => Self::Avx2XorJit,
                "scalar" => Self::Scalar,
                _ => Self::Auto,
            },
            Err(_) => Self::Auto,
        }
    }

    fn resolve(self) -> Self {
        match self {
            Self::Auto => {
                #[cfg(target_arch = "x86_64")]
                {
                    if matches!(detect_simd_support(), SimdLevel::Avx2) {
                        return Self::Avx2PshufbPrepared;
                    }
                }
                Self::Scalar
            }
            Self::Avx2PshufbPrepared => {
                #[cfg(target_arch = "x86_64")]
                {
                    if matches!(detect_simd_support(), SimdLevel::Avx2) {
                        return Self::Avx2PshufbPrepared;
                    }
                }
                Self::Scalar
            }
            Self::Avx2XorJit => {
                #[cfg(target_arch = "x86_64")]
                {
                    if matches!(detect_simd_support(), SimdLevel::Avx2)
                        && is_x86_feature_detected!("vpclmulqdq")
                    {
                        return self;
                    }
                }
                panic!("XOR-JIT create backend requires x86_64 AVX2 and VPCLMULQDQ support");
            }
            Self::Scalar => Self::Scalar,
        }
    }

    #[inline]
    fn ideal_input_multiple(self) -> usize {
        match self {
            Self::Auto | Self::Avx2PshufbPrepared => 4,
            // Turbo's AVX2 XOR-JIT leaves idealInputMultiple at 1, so its
            // default input batch rounds to DEFAULT_INPUT_GROUPING.
            Self::Avx2XorJit => 1,
            Self::Scalar => 1,
        }
    }

    #[inline]
    fn ideal_segment_len(self) -> usize {
        match self {
            Self::Auto | Self::Avx2PshufbPrepared => CREATE_SEGMENT_SIZE,
            // Turbo's AVX2 XOR-JIT method reports a 128KiB ideal chunk. Keep
            // the port segmentation aligned with that.
            Self::Avx2XorJit => xor_jit_segment_len_override().unwrap_or(128 * 1024),
            Self::Scalar => CREATE_SEGMENT_SIZE / 2,
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    fn xor_jit_flavor(self) -> Option<XorJitFlavor> {
        match self {
            Self::Avx2XorJit => Some(XorJitFlavor::Jit),
            _ => None,
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn xor_jit_segment_len_override() -> Option<usize> {
    match std::env::var(XOR_JIT_SEGMENT_LEN_ENV) {
        Ok(value) => {
            let parsed = value.parse::<usize>().unwrap_or_else(|_| {
                panic!(
                    "{XOR_JIT_SEGMENT_LEN_ENV} must be a positive integer byte count, got {value:?}"
                )
            });
            assert!(
                parsed > 0,
                "{XOR_JIT_SEGMENT_LEN_ENV} must be greater than zero"
            );
            Some(parsed)
        }
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            panic!("{XOR_JIT_SEGMENT_LEN_ENV} must be valid UTF-8")
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn xor_jit_segment_len_override() -> Option<usize> {
    None
}

/// Prepared coefficient for one `(recovery output, source input)` pair.
pub struct CreateCoeff {
    pub value: u16,
    pub split: SplitMulTable,
    #[cfg(target_arch = "x86_64")]
    pub avx2: Option<Avx2PreparedCoeff>,
    #[cfg(target_arch = "x86_64")]
    pub xor_jit: Option<XorJitPreparedCoeff>,
    #[cfg(target_arch = "x86_64")]
    pub xor_jit_bitplane: Option<XorJitPreparedCoeff>,
}

pub type Gf16Coeff = CreateCoeff;

impl CreateCoeff {
    #[inline]
    #[cfg(target_arch = "x86_64")]
    fn new(
        value: u16,
        prepare_pshufb: bool,
        prepare_bitplane: bool,
        xor_jit_cache: &mut XorJitPreparedCoeffCache,
    ) -> Self {
        let split = build_split_mul_table(Galois16::new(value));
        let avx2 = prepare_pshufb.then(|| prepare_avx2_coeff(&split));
        let xor_jit = prepare_bitplane.then(|| {
            let prepared = xor_jit_cache.prepare(value);
            prepared.ensure_bitplane_emitted();
            xor_jit_cache.cache_bitplane_handle(value, prepared.bitplane_handle());
            prepared
        });
        let xor_jit_bitplane = xor_jit.clone();

        Self {
            value,
            split,
            avx2,
            xor_jit,
            xor_jit_bitplane,
        }
    }

    #[cfg(not(target_arch = "x86_64"))]
    fn new(value: u16, _prepare_pshufb: bool) -> Self {
        Self {
            value,
            split: build_split_mul_table(Galois16::new(value)),
        }
    }
}

pub struct StagingArea {
    inputs: AlignedVec,
    source_indices: Vec<usize>,
    #[cfg(target_arch = "x86_64")]
    xor_jit_coeffs: Vec<u16>,
    batch_len: usize,
}

impl StagingArea {
    #[cfg(target_arch = "x86_64")]
    fn new(input_grouping: usize, input_storage_len: usize, recovery_count: usize) -> Self {
        Self {
            inputs: AlignedVec::new_zeroed(input_storage_len),
            source_indices: vec![0; input_grouping],
            xor_jit_coeffs: vec![0; input_grouping * recovery_count],
            batch_len: 0,
        }
    }

    #[cfg(not(target_arch = "x86_64"))]
    fn new(input_grouping: usize, input_storage_len: usize) -> Self {
        Self {
            inputs: AlignedVec::new_zeroed(input_storage_len),
            source_indices: vec![0; input_grouping],
            batch_len: 0,
        }
    }

    #[inline]
    fn slot_mut(&mut self, slot: usize, aligned_chunk_len: usize, chunk_len: usize) -> &mut [u8] {
        let start = slot * aligned_chunk_len;
        let end = start + chunk_len;
        &mut self.inputs[start..end]
    }
}

#[cfg(target_arch = "x86_64")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct XorJitBitplaneLayout {
    aligned_slice_len: usize,
    stride: usize,
    chunk_len: usize,
    input_batch_size: usize,
    recovery_count: usize,
}

#[cfg(target_arch = "x86_64")]
impl XorJitBitplaneLayout {
    fn new(
        max_slice_len: usize,
        chunk_len: usize,
        input_batch_size: usize,
        recovery_count: usize,
    ) -> Self {
        let stride = XOR_JIT_BITPLANE_ALIGNMENT;
        let aligned_slice_len = align_up(max_slice_len, stride) + stride;
        debug_assert!(chunk_len > 0);
        debug_assert!(chunk_len.is_multiple_of(XOR_JIT_BITPLANE_ALIGNMENT));
        debug_assert!(aligned_slice_len.is_multiple_of(XOR_JIT_BITPLANE_ALIGNMENT));
        Self {
            aligned_slice_len,
            stride,
            chunk_len,
            input_batch_size,
            recovery_count,
        }
    }

    #[inline]
    fn align_to_stride(&self, len: usize) -> usize {
        align_up(len, self.stride)
    }

    #[inline]
    fn aligned_current_slice_size(&self, slice_len: usize) -> usize {
        self.align_to_stride(slice_len) + self.stride
    }

    #[inline]
    fn segment_count_for(&self, aligned_current_slice_size: usize) -> usize {
        aligned_current_slice_size.div_ceil(self.chunk_len)
    }

    #[inline]
    fn input_offset(&self, slice_offset: usize, batch_idx: usize, proc_size: usize) -> usize {
        debug_assert!(batch_idx < self.input_batch_size);
        let offset = slice_offset * self.input_batch_size + batch_idx * proc_size;
        debug_assert!(offset.is_multiple_of(XOR_JIT_BITPLANE_ALIGNMENT));
        offset
    }

    #[inline]
    fn output_offset(&self, slice_offset: usize, recovery_idx: usize, proc_size: usize) -> usize {
        debug_assert!(recovery_idx < self.recovery_count);
        let offset = slice_offset * self.recovery_count + recovery_idx * proc_size;
        debug_assert!(offset.is_multiple_of(XOR_JIT_BITPLANE_ALIGNMENT));
        offset
    }

    #[inline]
    fn slice_offset(&self, segment_idx: usize) -> usize {
        segment_idx * self.chunk_len
    }

    #[inline]
    fn segment_len_for(&self, slice_offset: usize, aligned_current_slice_size: usize) -> usize {
        debug_assert!(slice_offset < aligned_current_slice_size);
        let len = (aligned_current_slice_size - slice_offset).min(self.chunk_len);
        debug_assert!(len.is_multiple_of(XOR_JIT_BITPLANE_ALIGNMENT));
        len
    }

    #[inline]
    fn input_storage_len(&self) -> usize {
        self.input_batch_size * self.aligned_slice_len
    }

    #[inline]
    fn output_storage_len(&self) -> usize {
        self.recovery_count * self.aligned_slice_len
    }
}

/// Create-side recovery backend with all hot-path storage owned up front.
pub struct CreateRecoveryBackend {
    pub source_count: usize,
    pub recovery_exponents: Vec<u16>,
    pub max_chunk_len: usize,
    pub chunk_len: usize,
    pub method: CreateGf16Method,
    pub input_grouping: usize,
    #[cfg(target_arch = "x86_64")]
    xor_jit_bitplane: bool,
    #[cfg(target_arch = "x86_64")]
    xor_jit_layout: Option<XorJitBitplaneLayout>,
    transfer_buffers: [AlignedVec; TRANSFER_BUFFER_COUNT],
    pub staging: Vec<StagingArea>,
    pub output_chunks: AlignedVec,
    pub coeffs: Vec<CreateCoeff>,
    workers: CreateWorkerPool,
    aligned_chunk_len: usize,
    active_staging: usize,
    compute_in_flight: bool,
    #[cfg(target_arch = "x86_64")]
    xor_jit_zero_outputs_pending: bool,
    job_storage: Vec<ComputeJob>,
}

impl CreateRecoveryBackend {
    pub fn new(
        base_values: &[u16],
        first_recovery_block: u32,
        recovery_count: usize,
        max_chunk_len: usize,
        thread_count: usize,
    ) -> Self {
        let requested_method = CreateGf16Method::from_env();
        let method = requested_method.resolve();
        let source_count = base_values.len();
        #[cfg(target_arch = "x86_64")]
        let xor_jit_bitplane = method.xor_jit_flavor().is_some();
        #[cfg(target_arch = "x86_64")]
        let chunk_alignment = if xor_jit_bitplane {
            XOR_JIT_BITPLANE_ALIGNMENT
        } else {
            AVX2_ALIGNMENT
        };
        #[cfg(not(target_arch = "x86_64"))]
        let chunk_alignment = AVX2_ALIGNMENT;
        let aligned_chunk_len = align_up(max_chunk_len, chunk_alignment);
        let input_grouping = input_grouping(source_count, method);
        #[cfg(target_arch = "x86_64")]
        let xor_jit_layout = xor_jit_bitplane.then(|| {
            let chunk_len = align_down(
                method
                    .ideal_segment_len()
                    .min(aligned_chunk_len)
                    .max(XOR_JIT_BITPLANE_ALIGNMENT),
                XOR_JIT_BITPLANE_ALIGNMENT,
            )
            .max(XOR_JIT_BITPLANE_ALIGNMENT);
            XorJitBitplaneLayout::new(aligned_chunk_len, chunk_len, input_grouping, recovery_count)
        });
        #[cfg(target_arch = "x86_64")]
        let staging_storage_len = xor_jit_layout
            .map(|layout| layout.input_storage_len())
            .unwrap_or(input_grouping * aligned_chunk_len);
        #[cfg(not(target_arch = "x86_64"))]
        let staging_storage_len = input_grouping * aligned_chunk_len;
        #[cfg(target_arch = "x86_64")]
        let output_storage_len = xor_jit_layout
            .map(|layout| layout.output_storage_len())
            .unwrap_or(recovery_count * aligned_chunk_len);
        #[cfg(not(target_arch = "x86_64"))]
        let output_storage_len = recovery_count * aligned_chunk_len;
        let recovery_exponents = (0..recovery_count)
            .map(|offset| (first_recovery_block + offset as u32) as u16)
            .collect::<Vec<_>>();
        let prepare_pshufb = matches!(method, CreateGf16Method::Avx2PshufbPrepared);
        #[cfg(target_arch = "x86_64")]
        let mut xor_jit_cache = XorJitPreparedCoeffCache::new();
        let coeffs = recovery_exponents
            .iter()
            .flat_map(|&exponent| {
                base_values
                    .iter()
                    .map(move |&base| Galois16::new(base).pow(exponent).value())
            })
            .map(|value| {
                #[cfg(target_arch = "x86_64")]
                {
                    CreateCoeff::new(value, prepare_pshufb, xor_jit_bitplane, &mut xor_jit_cache)
                }
                #[cfg(not(target_arch = "x86_64"))]
                {
                    CreateCoeff::new(value, prepare_pshufb)
                }
            })
            .collect::<Vec<_>>();
        let worker_count = thread_count.max(1);
        let max_job_count = max_compute_jobs(
            aligned_chunk_len.max(max_chunk_len),
            recovery_count,
            worker_count,
            method,
        );

        Self {
            source_count,
            recovery_exponents,
            max_chunk_len,
            chunk_len: 0,
            method,
            input_grouping,
            #[cfg(target_arch = "x86_64")]
            xor_jit_bitplane,
            #[cfg(target_arch = "x86_64")]
            xor_jit_layout,
            transfer_buffers: [
                AlignedVec::new_zeroed(aligned_chunk_len),
                AlignedVec::new_zeroed(aligned_chunk_len),
            ],
            staging: vec![
                {
                    #[cfg(target_arch = "x86_64")]
                    {
                        StagingArea::new(input_grouping, staging_storage_len, recovery_count)
                    }
                    #[cfg(not(target_arch = "x86_64"))]
                    {
                        StagingArea::new(input_grouping, staging_storage_len)
                    }
                },
                {
                    #[cfg(target_arch = "x86_64")]
                    {
                        StagingArea::new(input_grouping, staging_storage_len, recovery_count)
                    }
                    #[cfg(not(target_arch = "x86_64"))]
                    {
                        StagingArea::new(input_grouping, staging_storage_len)
                    }
                },
            ],
            output_chunks: AlignedVec::new_zeroed(output_storage_len),
            coeffs,
            workers: CreateWorkerPool::new(worker_count, max_job_count),
            aligned_chunk_len,
            active_staging: 0,
            compute_in_flight: false,
            #[cfg(target_arch = "x86_64")]
            xor_jit_zero_outputs_pending: true,
            job_storage: vec![ComputeJob::default(); max_job_count],
        }
    }

    #[inline]
    pub fn begin_chunk(&mut self, chunk_len: usize) {
        self.chunk_len = chunk_len;
        self.active_staging = 0;
        debug_assert!(!self.compute_in_flight);
        #[cfg(target_arch = "x86_64")]
        {
            self.xor_jit_zero_outputs_pending = true;
        }
        self.staging
            .iter_mut()
            .for_each(|staging| staging.batch_len = 0);

        debug_assert!(chunk_len <= self.max_chunk_len);
        debug_assert_eq!(
            self.coeffs.len(),
            self.recovery_exponents.len() * self.source_count
        );
        debug_assert!(self
            .staging
            .iter()
            .all(|staging| (staging.inputs.as_ptr() as usize).is_multiple_of(AVX2_ALIGNMENT)));
        debug_assert!((self.output_chunks.as_ptr() as usize).is_multiple_of(AVX2_ALIGNMENT));
        #[cfg(target_arch = "x86_64")]
        if let Some(layout) = self.xor_jit_layout {
            debug_assert!(self.output_chunks.len() >= layout.output_storage_len());
        } else {
            debug_assert!(
                self.output_chunks.len() >= self.recovery_exponents.len() * self.aligned_chunk_len
            );
        }
        #[cfg(not(target_arch = "x86_64"))]
        debug_assert!(
            self.output_chunks.len() >= self.recovery_exponents.len() * self.aligned_chunk_len
        );
        debug_assert!(self.workers.capacity() >= self.job_storage.len());
        #[cfg(target_arch = "x86_64")]
        debug_assert!(
            self.method != CreateGf16Method::Avx2PshufbPrepared
                || self.coeffs.iter().all(|c| c.avx2.is_some())
        );
        #[cfg(target_arch = "x86_64")]
        debug_assert!(
            !self.xor_jit_bitplane || self.coeffs.iter().all(|c| c.xor_jit_bitplane.is_some())
        );

        #[cfg(target_arch = "x86_64")]
        if !self.xor_jit_bitplane {
            self.output_chunks.fill(0);
        }
        #[cfg(not(target_arch = "x86_64"))]
        self.output_chunks.fill(0);
    }

    #[inline]
    pub fn transfer_buffer(&mut self, ring_index: usize) -> &mut [u8] {
        let idx = ring_index % TRANSFER_BUFFER_COUNT;
        let chunk = &mut self.transfer_buffers[idx][..self.chunk_len];
        chunk.fill(0);
        chunk
    }

    #[inline]
    pub fn prepare_transfer_buffer(&mut self, ring_index: usize) -> &mut [u8] {
        self.transfer_buffer(ring_index)
    }

    #[inline]
    pub fn add_input(&mut self, source_idx: usize, input_chunk: &[u8]) {
        debug_assert!(source_idx < self.source_count);
        debug_assert_eq!(input_chunk.len(), self.chunk_len);
        let staging_idx = self.active_staging;
        let staging = &mut self.staging[staging_idx];
        debug_assert!(staging.batch_len < self.input_grouping);

        let slot = staging.batch_len;
        #[cfg(target_arch = "x86_64")]
        if self.xor_jit_bitplane {
            let layout = self
                .xor_jit_layout
                .expect("XOR-JIT bitplane layout initialized");
            prepare_xor_jit_bitplane_staging(layout, staging, slot, self.chunk_len, input_chunk);
        } else {
            staging
                .slot_mut(slot, self.aligned_chunk_len, self.chunk_len)
                .copy_from_slice(input_chunk);
        }
        #[cfg(not(target_arch = "x86_64"))]
        staging
            .slot_mut(slot, self.aligned_chunk_len, self.chunk_len)
            .copy_from_slice(input_chunk);
        staging.source_indices[slot] = source_idx;
        #[cfg(target_arch = "x86_64")]
        if self.xor_jit_bitplane {
            pack_xor_jit_bitplane_coeffs(
                &self.coeffs,
                self.source_count,
                self.recovery_exponents.len(),
                self.input_grouping,
                staging,
                slot,
                source_idx,
            );
        }
        staging.batch_len += 1;

        if staging.batch_len == self.input_grouping {
            self.flush_active_staging();
        }
    }

    #[inline]
    pub fn add_transfer_input(&mut self, source_idx: usize, ring_index: usize) {
        let idx = ring_index % TRANSFER_BUFFER_COUNT;
        debug_assert!(source_idx < self.source_count);
        debug_assert!(self.chunk_len <= self.transfer_buffers[idx].len());
        let staging_idx = self.active_staging;
        let staging = &mut self.staging[staging_idx];
        debug_assert!(staging.batch_len < self.input_grouping);

        let slot = staging.batch_len;
        #[cfg(target_arch = "x86_64")]
        if self.xor_jit_bitplane {
            let layout = self
                .xor_jit_layout
                .expect("XOR-JIT bitplane layout initialized");
            prepare_xor_jit_bitplane_staging(
                layout,
                staging,
                slot,
                self.chunk_len,
                &self.transfer_buffers[idx][..self.chunk_len],
            );
        } else {
            staging
                .slot_mut(slot, self.aligned_chunk_len, self.chunk_len)
                .copy_from_slice(&self.transfer_buffers[idx][..self.chunk_len]);
        }
        #[cfg(not(target_arch = "x86_64"))]
        staging
            .slot_mut(slot, self.aligned_chunk_len, self.chunk_len)
            .copy_from_slice(&self.transfer_buffers[idx][..self.chunk_len]);
        staging.source_indices[slot] = source_idx;
        #[cfg(target_arch = "x86_64")]
        if self.xor_jit_bitplane {
            pack_xor_jit_bitplane_coeffs(
                &self.coeffs,
                self.source_count,
                self.recovery_exponents.len(),
                self.input_grouping,
                staging,
                slot,
                source_idx,
            );
        }
        staging.batch_len += 1;

        if staging.batch_len == self.input_grouping {
            self.flush_active_staging();
        }
    }

    #[inline]
    pub fn end_input(&mut self) {
        self.flush_active_staging();
        self.wait_for_compute();
    }

    #[inline]
    pub fn finish_chunk(
        &mut self,
        recovery_blocks: &mut [(u16, Vec<u8>)],
        block_size: usize,
    ) -> bool {
        self.end_input();
        let mut all_checksum_ok = true;

        recovery_blocks
            .iter_mut()
            .enumerate()
            .for_each(|(recovery_idx, (_, recovery_data))| {
                debug_assert!(recovery_data.capacity() >= block_size);
                debug_assert!(recovery_data.len() + self.chunk_len <= recovery_data.capacity());
                #[cfg(target_arch = "x86_64")]
                if self.xor_jit_bitplane {
                    let layout = self
                        .xor_jit_layout
                        .expect("XOR-JIT bitplane layout initialized");
                    let output_start = recovery_data.len();
                    recovery_data.resize(output_start + self.chunk_len, 0);
                    let output_checksum_ok = finish_xor_jit_bitplane_packed_output_cksum(
                        &mut recovery_data[output_start..output_start + self.chunk_len],
                        &self.output_chunks,
                        layout.recovery_count,
                        recovery_idx,
                        layout.chunk_len,
                    );
                    all_checksum_ok &= output_checksum_ok;
                } else {
                    let start = recovery_idx * self.aligned_chunk_len;
                    let end = start + self.chunk_len;
                    debug_assert!(end <= self.output_chunks.len());
                    recovery_data.extend_from_slice(&self.output_chunks[start..end]);
                }
                #[cfg(not(target_arch = "x86_64"))]
                {
                    let start = recovery_idx * self.aligned_chunk_len;
                    let end = start + self.chunk_len;
                    debug_assert!(end <= self.output_chunks.len());
                    recovery_data.extend_from_slice(&self.output_chunks[start..end]);
                }
            });
        all_checksum_ok
    }

    #[inline]
    pub fn recovery_blocks(&self, block_size: usize) -> Vec<(u16, Vec<u8>)> {
        self.recovery_exponents
            .iter()
            .map(|&exponent| (exponent, Vec::with_capacity(block_size)))
            .collect()
    }

    #[inline]
    pub fn selected_method(&self) -> CreateGf16Method {
        self.method
    }

    #[inline]
    fn flush_active_staging(&mut self) {
        let staging_idx = self.active_staging;
        if self.staging[staging_idx].batch_len == 0 {
            return;
        }

        self.wait_for_compute();
        let job_count = self.build_compute_jobs(staging_idx);
        self.workers.submit(&self.job_storage[..job_count]);
        self.compute_in_flight = true;
        self.staging[staging_idx].batch_len = 0;
        self.active_staging = (self.active_staging + 1) % self.staging.len();
    }

    #[inline]
    fn wait_for_compute(&mut self) {
        if self.compute_in_flight {
            self.workers.wait();
            self.compute_in_flight = false;
        }
    }

    fn build_compute_jobs(&mut self, staging_idx: usize) -> usize {
        let worker_count = self.workers.worker_count();
        let recovery_count = self.recovery_exponents.len();
        #[cfg(target_arch = "x86_64")]
        let compute_len = if self.xor_jit_bitplane {
            self.xor_jit_layout
                .expect("XOR-JIT bitplane layout initialized")
                .aligned_current_slice_size(self.chunk_len)
        } else {
            self.chunk_len
        };
        #[cfg(not(target_arch = "x86_64"))]
        let compute_len = self.chunk_len;
        let segment_len = align_down(
            self.method
                .ideal_segment_len()
                .min(compute_len)
                .max(AVX2_ALIGNMENT),
            AVX2_ALIGNMENT,
        )
        .max(AVX2_ALIGNMENT);
        let segment_count = compute_len.div_ceil(segment_len);
        let output_groups = worker_count.min(recovery_count).max(1);
        let outputs_per_group = recovery_count.div_ceil(output_groups);
        let staging = &self.staging[staging_idx];
        debug_assert!(staging.batch_len <= self.input_grouping);
        debug_assert!(self.coeffs.len() == recovery_count * self.source_count);

        #[cfg(target_arch = "x86_64")]
        if self.xor_jit_bitplane {
            let layout = self
                .xor_jit_layout
                .expect("XOR-JIT bitplane layout initialized");
            return self.build_compute_jobs_xor_jit_bitplane(
                compute_len,
                layout,
                worker_count,
                recovery_count,
                staging_idx,
            );
        }

        let mut job_count = 0;
        for segment_idx in 0..segment_count {
            let start = segment_idx * segment_len;
            let len = (compute_len - start).min(segment_len);
            for output_group in 0..output_groups {
                let output_start = output_group * outputs_per_group;
                let output_end = ((output_group + 1) * outputs_per_group).min(recovery_count);
                if output_start == output_end {
                    continue;
                }
                debug_assert!(job_count < self.job_storage.len());
                self.job_storage[job_count] = ComputeJob {
                    method: self.method,
                    input_base: staging.inputs.as_ptr() as usize,
                    output_base: self.output_chunks.as_ptr() as usize,
                    coeffs: self.coeffs.as_ptr() as usize,
                    source_indices: staging.source_indices.as_ptr() as usize,
                    recovery_exponents: self.recovery_exponents.as_ptr() as usize,
                    xor_jit_coeffs: staging.xor_jit_coeffs.as_ptr() as usize,
                    source_count: self.source_count,
                    batch_len: staging.batch_len,
                    aligned_chunk_len: self.aligned_chunk_len,
                    compute_len,
                    segment_start: start,
                    segment_len: len,
                    segment_count: 1,
                    output_start,
                    output_end,
                    #[cfg(target_arch = "x86_64")]
                    xor_jit_bitplane: self.xor_jit_bitplane,
                    #[cfg(target_arch = "x86_64")]
                    xor_jit_segment_len: 0,
                    #[cfg(target_arch = "x86_64")]
                    xor_jit_input_grouping: 0,
                    #[cfg(target_arch = "x86_64")]
                    xor_jit_add_outputs: false,
                };
                job_count += 1;
            }
        }
        job_count
    }

    #[cfg(target_arch = "x86_64")]
    fn build_compute_jobs_xor_jit_bitplane(
        &mut self,
        compute_len: usize,
        layout: XorJitBitplaneLayout,
        worker_count: usize,
        recovery_count: usize,
        staging_idx: usize,
    ) -> usize {
        let staging = &self.staging[staging_idx];
        let mut job_count = 0;
        let add_outputs = !self.xor_jit_zero_outputs_pending;
        self.xor_jit_zero_outputs_pending = false;
        let segment_count = layout.segment_count_for(compute_len);

        let full_chunks_per_worker = segment_count / worker_count.max(1);
        let leftover_chunks = segment_count % worker_count.max(1);
        let mut segment_idx = 0usize;

        if leftover_chunks != 0 {
            let threads_per_chunk = (worker_count / leftover_chunks).min(recovery_count).max(1);
            let outputs_per_thread = recovery_count as f64 / threads_per_chunk as f64;

            for _ in 0..leftover_chunks {
                let slice_offset = layout.slice_offset(segment_idx);
                let segment_len = layout.segment_len_for(slice_offset, compute_len);
                let mut output_start = 0usize;
                for thread_chunk in 0..threads_per_chunk {
                    let output_end = (((thread_chunk + 1) as f64 * outputs_per_thread).round()
                        as usize)
                        .min(recovery_count)
                        .max(output_start + 1);
                    let local_output_count = output_end - output_start;
                    debug_assert!(job_count < self.job_storage.len());
                    self.job_storage[job_count] = ComputeJob {
                        method: self.method,
                        input_base: unsafe {
                            staging.inputs.as_ptr().add(layout.input_offset(
                                slice_offset,
                                0,
                                segment_len,
                            )) as usize
                        },
                        output_base: unsafe {
                            self.output_chunks.as_ptr().add(layout.output_offset(
                                slice_offset,
                                output_start,
                                segment_len,
                            )) as usize
                        },
                        coeffs: self.coeffs.as_ptr() as usize,
                        source_indices: staging.source_indices.as_ptr() as usize,
                        recovery_exponents: unsafe {
                            self.recovery_exponents.as_ptr().add(output_start) as usize
                        },
                        xor_jit_coeffs: unsafe {
                            staging
                                .xor_jit_coeffs
                                .as_ptr()
                                .add(output_start * layout.input_batch_size)
                                as usize
                        },
                        source_count: self.source_count,
                        batch_len: staging.batch_len,
                        aligned_chunk_len: self.aligned_chunk_len,
                        compute_len: segment_len,
                        segment_start: 0,
                        segment_len,
                        segment_count: 1,
                        output_start: 0,
                        output_end: local_output_count,
                        xor_jit_bitplane: true,
                        xor_jit_segment_len: layout.chunk_len,
                        xor_jit_input_grouping: layout.input_batch_size,
                        xor_jit_add_outputs: add_outputs,
                    };
                    job_count += 1;
                    output_start = output_end;
                }
                debug_assert_eq!(output_start, recovery_count);
                segment_idx += 1;
            }
        }

        if full_chunks_per_worker != 0 {
            while segment_idx < segment_count {
                let slice_offset = layout.slice_offset(segment_idx);
                let segment_len = layout.segment_len_for(slice_offset, compute_len);
                let total_len =
                    (compute_len - slice_offset).min(full_chunks_per_worker * layout.chunk_len);
                debug_assert!(job_count < self.job_storage.len());
                self.job_storage[job_count] = ComputeJob {
                    method: self.method,
                    input_base: unsafe {
                        staging.inputs.as_ptr().add(layout.input_offset(
                            slice_offset,
                            0,
                            segment_len,
                        )) as usize
                    },
                    output_base: unsafe {
                        self.output_chunks.as_ptr().add(layout.output_offset(
                            slice_offset,
                            0,
                            segment_len,
                        )) as usize
                    },
                    coeffs: self.coeffs.as_ptr() as usize,
                    source_indices: staging.source_indices.as_ptr() as usize,
                    recovery_exponents: self.recovery_exponents.as_ptr() as usize,
                    xor_jit_coeffs: staging.xor_jit_coeffs.as_ptr() as usize,
                    source_count: self.source_count,
                    batch_len: staging.batch_len,
                    aligned_chunk_len: self.aligned_chunk_len,
                    compute_len: total_len,
                    segment_start: 0,
                    segment_len,
                    segment_count: full_chunks_per_worker,
                    output_start: 0,
                    output_end: recovery_count,
                    xor_jit_bitplane: true,
                    xor_jit_segment_len: layout.chunk_len,
                    xor_jit_input_grouping: layout.input_batch_size,
                    xor_jit_add_outputs: add_outputs,
                };
                job_count += 1;
                segment_idx += full_chunks_per_worker;
            }
        }

        debug_assert_eq!(segment_idx, segment_count);
        job_count.max(1)
    }
}

#[cfg(target_arch = "x86_64")]
fn prepare_xor_jit_bitplane_staging(
    layout: XorJitBitplaneLayout,
    staging: &mut StagingArea,
    slot: usize,
    slice_len: usize,
    input_chunk: &[u8],
) {
    let aligned_slice_len = layout.align_to_stride(slice_len);
    debug_assert!(aligned_slice_len.is_multiple_of(XOR_JIT_BITPLANE_ALIGNMENT));
    debug_assert!(input_chunk.len() <= slice_len);
    prepare_xor_jit_bitplane_packed_input_cksum(
        &mut staging.inputs,
        input_chunk,
        aligned_slice_len,
        layout.input_batch_size,
        slot,
        layout.chunk_len,
    );
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn pack_xor_jit_bitplane_coeffs(
    coeffs: &[CreateCoeff],
    source_count: usize,
    recovery_count: usize,
    input_grouping: usize,
    staging: &mut StagingArea,
    slot: usize,
    source_idx: usize,
) {
    for recovery_idx in 0..recovery_count {
        let coeff = &coeffs[gf_coeff_index(recovery_idx, source_idx, source_count)];
        staging.xor_jit_coeffs[recovery_idx * input_grouping + slot] = coeff.value;
    }
}

#[derive(Clone, Copy, Default)]
struct ComputeJob {
    method: CreateGf16Method,
    input_base: usize,
    output_base: usize,
    coeffs: usize,
    source_indices: usize,
    recovery_exponents: usize,
    #[cfg(target_arch = "x86_64")]
    xor_jit_coeffs: usize,
    source_count: usize,
    batch_len: usize,
    aligned_chunk_len: usize,
    compute_len: usize,
    segment_start: usize,
    segment_len: usize,
    segment_count: usize,
    output_start: usize,
    output_end: usize,
    #[cfg(target_arch = "x86_64")]
    xor_jit_bitplane: bool,
    #[cfg(target_arch = "x86_64")]
    xor_jit_segment_len: usize,
    #[cfg(target_arch = "x86_64")]
    xor_jit_input_grouping: usize,
    #[cfg(target_arch = "x86_64")]
    xor_jit_add_outputs: bool,
}

impl Default for CreateGf16Method {
    fn default() -> Self {
        Self::Scalar
    }
}

struct CreateWorkerPool {
    shared: Arc<WorkerShared>,
    handles: Vec<JoinHandle<()>>,
}

struct WorkerShared {
    state: Mutex<WorkerState>,
    ready: Condvar,
    done: Condvar,
}

struct WorkerState {
    jobs: Vec<ComputeJob>,
    job_count: usize,
    next_job: usize,
    remaining_jobs: usize,
    generation: u64,
    stop: bool,
}

#[cfg(target_arch = "x86_64")]
#[derive(Default)]
struct WorkerContext {
    xor_jit_bitplane_scratch: Option<XorJitBitplaneScratch>,
}

impl CreateWorkerPool {
    fn new(worker_count: usize, max_jobs: usize) -> Self {
        let shared = Arc::new(WorkerShared {
            state: Mutex::new(WorkerState {
                jobs: vec![ComputeJob::default(); max_jobs.max(1)],
                job_count: 0,
                next_job: 0,
                remaining_jobs: 0,
                generation: 0,
                stop: false,
            }),
            ready: Condvar::new(),
            done: Condvar::new(),
        });
        let handles = (0..worker_count)
            .map(|_| {
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || worker_loop(shared))
            })
            .collect::<Vec<_>>();

        Self { shared, handles }
    }

    #[inline]
    fn worker_count(&self) -> usize {
        self.handles.len().max(1)
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.shared.state.lock().unwrap().jobs.len()
    }

    fn submit(&self, jobs: &[ComputeJob]) {
        if jobs.is_empty() {
            return;
        }

        let mut state = self.shared.state.lock().unwrap();
        debug_assert_eq!(state.remaining_jobs, 0);
        debug_assert!(jobs.len() <= state.jobs.len());
        state.jobs[..jobs.len()].copy_from_slice(jobs);
        state.job_count = jobs.len();
        state.next_job = 0;
        state.remaining_jobs = jobs.len();
        state.generation = state.generation.wrapping_add(1);
        self.shared.ready.notify_all();
    }

    fn wait(&self) {
        let mut state = self.shared.state.lock().unwrap();
        while state.remaining_jobs != 0 {
            state = self.shared.done.wait(state).unwrap();
        }
    }
}

impl Drop for CreateWorkerPool {
    fn drop(&mut self) {
        self.wait();
        {
            let mut state = self.shared.state.lock().unwrap();
            state.stop = true;
            state.generation = state.generation.wrapping_add(1);
        }
        self.shared.ready.notify_all();
        while let Some(handle) = self.handles.pop() {
            let _ = handle.join();
        }
    }
}

fn worker_loop(shared: Arc<WorkerShared>) {
    #[cfg(target_arch = "x86_64")]
    let mut context = WorkerContext::default();
    let mut seen_generation = 0u64;
    loop {
        let job = {
            let mut state = shared.state.lock().unwrap();
            loop {
                if state.stop {
                    return;
                }
                if state.generation != seen_generation && state.next_job < state.job_count {
                    let job = state.jobs[state.next_job];
                    state.next_job += 1;
                    break job;
                }
                if state.generation != seen_generation && state.next_job >= state.job_count {
                    seen_generation = state.generation;
                }
                state = shared.ready.wait(state).unwrap();
            }
        };

        #[cfg(target_arch = "x86_64")]
        process_compute_job(job, &mut context);
        #[cfg(not(target_arch = "x86_64"))]
        process_compute_job(job);

        let mut state = shared.state.lock().unwrap();
        state.remaining_jobs -= 1;
        if state.remaining_jobs == 0 {
            seen_generation = state.generation;
            shared.done.notify_one();
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn process_compute_job(job: ComputeJob) {
    for recovery_idx in job.output_start..job.output_end {
        let output_start = recovery_idx * job.aligned_chunk_len + job.segment_start;
        let output = unsafe {
            std::slice::from_raw_parts_mut(
                (job.output_base as *mut u8).add(output_start),
                job.segment_len,
            )
        };
        debug_assert!(output.as_ptr() as usize >= job.output_base);

        for batch_idx in 0..job.batch_len {
            let source_idx = unsafe { *(job.source_indices as *const usize).add(batch_idx) };
            let coeff = unsafe {
                &*(job.coeffs as *const CreateCoeff).add(gf_coeff_index(
                    recovery_idx,
                    source_idx,
                    job.source_count,
                ))
            };
            let input_start = batch_idx * job.aligned_chunk_len + job.segment_start;
            let input = unsafe {
                std::slice::from_raw_parts(
                    (job.input_base as *const u8).add(input_start),
                    job.segment_len,
                )
            };
            process_slice_multiply_add(input, output, &coeff.split);
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn process_compute_job(job: ComputeJob, context: &mut WorkerContext) {
    #[cfg(target_arch = "x86_64")]
    if let Some(flavor) = job.method.xor_jit_flavor() {
        process_compute_job_xor_jit(job, flavor, context);
        return;
    }

    for recovery_idx in job.output_start..job.output_end {
        let output_start = recovery_idx * job.aligned_chunk_len + job.segment_start;
        let output = unsafe {
            std::slice::from_raw_parts_mut(
                (job.output_base as *mut u8).add(output_start),
                job.segment_len,
            )
        };
        debug_assert!(output.as_ptr() as usize >= job.output_base);

        if matches!(job.method, CreateGf16Method::Avx2PshufbPrepared) {
            process_batch_add_avx2_pshufb(job, recovery_idx, output);
            continue;
        }

        for batch_idx in 0..job.batch_len {
            let source_idx = unsafe { *(job.source_indices as *const usize).add(batch_idx) };
            let coeff = unsafe {
                &*(job.coeffs as *const CreateCoeff).add(gf_coeff_index(
                    recovery_idx,
                    source_idx,
                    job.source_count,
                ))
            };
            let input_start = batch_idx * job.aligned_chunk_len + job.segment_start;
            let input = unsafe {
                std::slice::from_raw_parts(
                    (job.input_base as *const u8).add(input_start),
                    job.segment_len,
                )
            };
            process_slice_multiply_add(input, output, &coeff.split);
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn process_compute_job_xor_jit(job: ComputeJob, flavor: XorJitFlavor, context: &mut WorkerContext) {
    if job.xor_jit_bitplane {
        process_compute_job_xor_jit_bitplane(job, context);
        return;
    }

    let mut recovery_idx = job.output_start;
    while recovery_idx + 3 < job.output_end {
        let output_a_start = recovery_idx * job.aligned_chunk_len + job.segment_start;
        let output_b_start = (recovery_idx + 1) * job.aligned_chunk_len + job.segment_start;
        let output_c_start = (recovery_idx + 2) * job.aligned_chunk_len + job.segment_start;
        let output_d_start = (recovery_idx + 3) * job.aligned_chunk_len + job.segment_start;
        let output_a = unsafe {
            std::slice::from_raw_parts_mut(
                (job.output_base as *mut u8).add(output_a_start),
                job.segment_len,
            )
        };
        let output_b = unsafe {
            std::slice::from_raw_parts_mut(
                (job.output_base as *mut u8).add(output_b_start),
                job.segment_len,
            )
        };
        let output_c = unsafe {
            std::slice::from_raw_parts_mut(
                (job.output_base as *mut u8).add(output_c_start),
                job.segment_len,
            )
        };
        let output_d = unsafe {
            std::slice::from_raw_parts_mut(
                (job.output_base as *mut u8).add(output_d_start),
                job.segment_len,
            )
        };
        process_batch_add_avx2_xor_jit_x4_outputs(
            job,
            recovery_idx,
            output_a,
            recovery_idx + 1,
            output_b,
            recovery_idx + 2,
            output_c,
            recovery_idx + 3,
            output_d,
            flavor,
        );
        recovery_idx += 4;
    }

    while recovery_idx + 1 < job.output_end {
        let output_a_start = recovery_idx * job.aligned_chunk_len + job.segment_start;
        let output_b_start = (recovery_idx + 1) * job.aligned_chunk_len + job.segment_start;
        let output_a = unsafe {
            std::slice::from_raw_parts_mut(
                (job.output_base as *mut u8).add(output_a_start),
                job.segment_len,
            )
        };
        let output_b = unsafe {
            std::slice::from_raw_parts_mut(
                (job.output_base as *mut u8).add(output_b_start),
                job.segment_len,
            )
        };
        process_batch_add_avx2_xor_jit_x2_outputs(
            job,
            recovery_idx,
            output_a,
            recovery_idx + 1,
            output_b,
            flavor,
        );
        recovery_idx += 2;
    }

    if recovery_idx < job.output_end {
        let output_start = recovery_idx * job.aligned_chunk_len + job.segment_start;
        let output = unsafe {
            std::slice::from_raw_parts_mut(
                (job.output_base as *mut u8).add(output_start),
                job.segment_len,
            )
        };
        process_batch_add_avx2_xor_jit(job, recovery_idx, output, flavor);
    }
}

#[cfg(target_arch = "x86_64")]
fn process_compute_job_xor_jit_bitplane(job: ComputeJob, context: &mut WorkerContext) {
    let scratch = context
        .xor_jit_bitplane_scratch
        .get_or_insert_with(|| XorJitBitplaneScratch::new().expect("allocate xor-jit scratch"));
    let num_outputs = job.output_end - job.output_start;
    let mut inputs_prefetched_per_invok = job.batch_len / job.xor_jit_input_grouping;
    let mut input_prefetch_out_offset = num_outputs.saturating_sub(1);
    const MAX_PF_FACTOR: usize = 3;
    if inputs_prefetched_per_invok > (1usize << XOR_JIT_PREFETCH_DOWNSCALE) {
        inputs_prefetched_per_invok -= 1usize << XOR_JIT_PREFETCH_DOWNSCALE;
        inputs_prefetched_per_invok <<= MAX_PF_FACTOR - XOR_JIT_PREFETCH_DOWNSCALE;
        input_prefetch_out_offset =
            (job.batch_len << MAX_PF_FACTOR).div_ceil(inputs_prefetched_per_invok);
        input_prefetch_out_offset = num_outputs.saturating_sub(input_prefetch_out_offset);
    }

    for round in 0..job.segment_count {
        let proc_size =
            (job.compute_len - round * job.xor_jit_segment_len).min(job.xor_jit_segment_len);
        debug_assert!(proc_size.is_multiple_of(XOR_JIT_BITPLANE_ALIGNMENT));
        let final_round = round + 1 == job.segment_count;

        let input_ptr = unsafe {
            (job.input_base as *const u8)
                .add(round * job.xor_jit_segment_len * job.xor_jit_input_grouping)
        };

        for local_output_idx in 0..num_outputs {
            let output_ptr = unsafe {
                (job.output_base as *mut u8).add(
                    local_output_idx * proc_size + round * num_outputs * job.xor_jit_segment_len,
                )
            };
            if !job.xor_jit_add_outputs {
                unsafe {
                    std::ptr::write_bytes(output_ptr, 0, proc_size);
                }
            }

            if final_round {
                if local_output_idx + 1 < num_outputs {
                    let output_prefetch = Some(unsafe { output_ptr.add(proc_size) } as *const u8);
                    if xor_jit_bitplane_output_nonzero(job, local_output_idx) {
                        process_batch_add_avx2_xor_jit_bitplane_packed_with_prefetch(
                            job,
                            local_output_idx,
                            input_ptr,
                            output_ptr,
                            proc_size,
                            None,
                            output_prefetch,
                            scratch,
                        );
                    } else {
                        process_batch_add_avx2_xor_jit_bitplane_add_only_packed_with_prefetch(
                            job,
                            input_ptr,
                            output_ptr,
                            proc_size,
                            None,
                            output_prefetch,
                        );
                    }
                } else {
                    process_batch_add_avx2_xor_jit_bitplane_packed(
                        job,
                        local_output_idx,
                        input_ptr,
                        output_ptr,
                        proc_size,
                        scratch,
                    );
                }
            } else {
                let input_prefetch = if local_output_idx >= input_prefetch_out_offset {
                    Some(unsafe {
                        (job.input_base as *const u8).add(
                            (round + 1) * job.xor_jit_segment_len * job.xor_jit_input_grouping
                                + ((inputs_prefetched_per_invok
                                    * (local_output_idx - input_prefetch_out_offset)
                                    * proc_size)
                                    >> MAX_PF_FACTOR),
                        )
                    })
                } else {
                    None
                };
                let output_prefetch = Some(unsafe { output_ptr.add(proc_size) } as *const u8);

                if xor_jit_bitplane_output_nonzero(job, local_output_idx) {
                    process_batch_add_avx2_xor_jit_bitplane_packed_with_prefetch(
                        job,
                        local_output_idx,
                        input_ptr,
                        output_ptr,
                        proc_size,
                        input_prefetch,
                        output_prefetch,
                        scratch,
                    );
                } else {
                    process_batch_add_avx2_xor_jit_bitplane_add_only_packed_with_prefetch(
                        job,
                        input_ptr,
                        output_ptr,
                        proc_size,
                        input_prefetch,
                        output_prefetch,
                    );
                }
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn xor_jit_bitplane_output_nonzero(job: ComputeJob, local_output_idx: usize) -> bool {
    unsafe { *(job.recovery_exponents as *const u16).add(local_output_idx) != 0 }
}

#[cfg(target_arch = "x86_64")]
fn process_batch_add_avx2_xor_jit_bitplane_packed_with_prefetch(
    job: ComputeJob,
    local_output_idx: usize,
    input_ptr: *const u8,
    output: *mut u8,
    round_len: usize,
    input_prefetch: Option<*const u8>,
    output_prefetch: Option<*const u8>,
    scratch: &mut XorJitBitplaneScratch,
) {
    let coeff_ptr = unsafe {
        (job.xor_jit_coeffs as *const u16).add(local_output_idx * job.xor_jit_input_grouping)
    };
    unsafe {
        xor_jit_multi_region_packpf_ptr(
            scratch,
            coeff_ptr,
            input_ptr,
            job.batch_len,
            output,
            round_len,
            input_prefetch,
            output_prefetch,
        );
    }
}

#[cfg(target_arch = "x86_64")]
fn process_batch_add_avx2_xor_jit_bitplane_packed(
    job: ComputeJob,
    local_output_idx: usize,
    input_ptr: *const u8,
    output: *mut u8,
    round_len: usize,
    scratch: &mut XorJitBitplaneScratch,
) {
    let coeff_ptr = unsafe {
        (job.xor_jit_coeffs as *const u16).add(local_output_idx * job.xor_jit_input_grouping)
    };
    unsafe {
        xor_jit_multi_region_ptr(
            scratch,
            coeff_ptr,
            input_ptr,
            job.batch_len,
            output,
            round_len,
        );
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn xor_jit_multi_region_ptr(
    scratch: &mut XorJitBitplaneScratch,
    coeffs: *const u16,
    src: *const u8,
    regions: usize,
    output: *mut u8,
    len: usize,
) {
    debug_assert!(regions > 0);
    for region in 0..regions {
        let coefficient = unsafe { *coeffs.add(region) };
        let input = unsafe { src.add(region * len) };
        unsafe { scratch.multiply_add_ptr_coefficient(coefficient, input, output, len) };
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn xor_jit_multi_region_packpf_ptr(
    scratch: &mut XorJitBitplaneScratch,
    coeffs: *const u16,
    src: *const u8,
    regions: usize,
    output: *mut u8,
    len: usize,
    prefetch_in: Option<*const u8>,
    prefetch_out: Option<*const u8>,
) {
    const PREFETCH_FACTOR: usize = 1;
    const OUTPUT_PREFETCH_ROUNDS: usize = 1 << PREFETCH_FACTOR;

    debug_assert!(regions > 0);

    let pf_len = len >> PREFETCH_FACTOR;
    let mut region = 0usize;
    let mut prefetch_ptr = prefetch_out;
    let mut output_pf_rounds = OUTPUT_PREFETCH_ROUNDS;
    while region < regions && output_pf_rounds > 0 {
        let coefficient = unsafe { *coeffs.add(region) };
        let input = unsafe { src.add(region * len) };
        let current_prefetch = prefetch_ptr.unwrap_or(std::ptr::null());
        unsafe {
            scratch.multiply_add_ptr_coefficient_prefetch(
                coefficient,
                input,
                output,
                len,
                current_prefetch,
            );
        }
        region += 1;
        output_pf_rounds -= 1;
        prefetch_ptr = prefetch_ptr.map(|ptr| ptr.wrapping_add(pf_len));
    }

    if let Some(mut prefetch_ptr) = prefetch_in {
        while region < regions {
            let coefficient = unsafe { *coeffs.add(region) };
            let input = unsafe { src.add(region * len) };
            unsafe {
                scratch.multiply_add_ptr_coefficient_prefetch(
                    coefficient,
                    input,
                    output,
                    len,
                    prefetch_ptr,
                );
            }
            region += 1;
            prefetch_ptr = prefetch_ptr.wrapping_add(pf_len);
        }
    } else {
        unsafe {
            xor_jit_multi_region_ptr(
                scratch,
                coeffs.add(region),
                src.add(region * len),
                regions - region,
                output,
                len,
            );
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn process_batch_add_avx2_xor_jit_bitplane_add_only_packed_with_prefetch(
    job: ComputeJob,
    input_ptr: *const u8,
    output: *mut u8,
    round_len: usize,
    input_prefetch: Option<*const u8>,
    output_prefetch: Option<*const u8>,
) {
    xor_packed_multi_region_v16i1_ptr(
        input_ptr,
        job.batch_len,
        output,
        round_len,
        input_prefetch,
        output_prefetch,
    );
}

#[cfg(all(target_arch = "x86_64", test))]
fn xor_jit_bitplane_output_prefetch_ptr(
    job: ComputeJob,
    layout: XorJitBitplaneLayout,
    segment_idx: usize,
    recovery_idx: usize,
    batch_idx: usize,
) -> Option<*const u8> {
    xor_jit_bitplane_output_prefetch_ptr_from_base(
        job,
        layout,
        segment_idx,
        recovery_idx,
        batch_idx,
        job.output_base,
    )
}

#[cfg(all(target_arch = "x86_64", test))]
fn xor_jit_bitplane_output_prefetch_ptr_from_base(
    job: ComputeJob,
    layout: XorJitBitplaneLayout,
    segment_idx: usize,
    recovery_idx: usize,
    batch_idx: usize,
    output_base: usize,
) -> Option<*const u8> {
    let prefetch_len = job.segment_len >> XOR_JIT_PREFETCH_DOWNSCALE;
    let slice_offset = layout.slice_offset(segment_idx);
    let proc_size = layout.segment_len_for(slice_offset, layout.aligned_slice_len);
    let current = layout.output_offset(slice_offset, recovery_idx, proc_size);
    let start = current + job.segment_len + batch_idx * prefetch_len;
    let output_bytes = layout.output_storage_len();

    if start < output_bytes {
        Some((output_base as *const u8).wrapping_add(start))
    } else {
        None
    }
}

#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(test), allow(dead_code))]
fn xor_jit_bitplane_input_prefetch_ptr(
    job: ComputeJob,
    layout: XorJitBitplaneLayout,
    segment_idx: usize,
    recovery_idx: usize,
    batch_idx: usize,
) -> Option<*const u8> {
    if segment_idx + 1 >= layout.segment_count_for(layout.aligned_slice_len) {
        return None;
    }

    let output_count = job.output_end - job.output_start;
    let local_output_idx = recovery_idx - job.output_start;
    let offset = xor_jit_bitplane_input_prefetch_base_offset(job, output_count, local_output_idx)?
        + (batch_idx - XOR_JIT_PREFETCH_OUTPUT_ROUNDS)
            * (job.segment_len >> XOR_JIT_PREFETCH_DOWNSCALE);
    let next_slice_offset = layout.slice_offset(segment_idx + 1);
    let next_proc_size = layout.segment_len_for(next_slice_offset, layout.aligned_slice_len);
    let start = layout.input_offset(next_slice_offset, 0, next_proc_size);
    Some((job.input_base as *const u8).wrapping_add(start + offset))
}

#[cfg(target_arch = "x86_64")]
fn xor_jit_bitplane_input_prefetch_base_offset(
    job: ComputeJob,
    output_count: usize,
    local_output_idx: usize,
) -> Option<usize> {
    if job.batch_len <= XOR_JIT_PREFETCH_OUTPUT_ROUNDS {
        return None;
    }

    let scaled_inputs_per_output = (job.batch_len - XOR_JIT_PREFETCH_OUTPUT_ROUNDS)
        << (XOR_JIT_PREFETCH_SPREAD_SHIFT - XOR_JIT_PREFETCH_DOWNSCALE);
    let output_start = output_count.saturating_sub(
        (job.batch_len << XOR_JIT_PREFETCH_SPREAD_SHIFT).div_ceil(scaled_inputs_per_output),
    );
    if local_output_idx < output_start {
        return None;
    }

    let output_rank = local_output_idx - output_start;
    Some(
        (scaled_inputs_per_output * output_rank * job.segment_len) >> XOR_JIT_PREFETCH_SPREAD_SHIFT,
    )
}

#[cfg(target_arch = "x86_64")]
#[allow(clippy::too_many_arguments)]
fn process_batch_add_avx2_xor_jit_x4_outputs(
    job: ComputeJob,
    recovery_a: usize,
    output_a: &mut [u8],
    recovery_b: usize,
    output_b: &mut [u8],
    recovery_c: usize,
    output_c: &mut [u8],
    recovery_d: usize,
    output_d: &mut [u8],
    flavor: XorJitFlavor,
) {
    let mut batch_idx = 0;
    while batch_idx + 3 < job.batch_len {
        let source_a = unsafe { *(job.source_indices as *const usize).add(batch_idx) };
        let source_b = unsafe { *(job.source_indices as *const usize).add(batch_idx + 1) };
        let source_c = unsafe { *(job.source_indices as *const usize).add(batch_idx + 2) };
        let source_d = unsafe { *(job.source_indices as *const usize).add(batch_idx + 3) };
        let coeff_a0 = coeff_for(job, recovery_a, source_a);
        let coeff_b0 = coeff_for(job, recovery_a, source_b);
        let coeff_c0 = coeff_for(job, recovery_a, source_c);
        let coeff_d0 = coeff_for(job, recovery_a, source_d);
        let coeff_a1 = coeff_for(job, recovery_b, source_a);
        let coeff_b1 = coeff_for(job, recovery_b, source_b);
        let coeff_c1 = coeff_for(job, recovery_b, source_c);
        let coeff_d1 = coeff_for(job, recovery_b, source_d);
        let coeff_a2 = coeff_for(job, recovery_c, source_a);
        let coeff_b2 = coeff_for(job, recovery_c, source_b);
        let coeff_c2 = coeff_for(job, recovery_c, source_c);
        let coeff_d2 = coeff_for(job, recovery_c, source_d);
        let coeff_a3 = coeff_for(job, recovery_d, source_a);
        let coeff_b3 = coeff_for(job, recovery_d, source_b);
        let coeff_c3 = coeff_for(job, recovery_d, source_c);
        let coeff_d3 = coeff_for(job, recovery_d, source_d);
        let input_a = input_segment(job, batch_idx);
        let input_b = input_segment(job, batch_idx + 1);
        let input_c = input_segment(job, batch_idx + 2);
        let input_d = input_segment(job, batch_idx + 3);

        match (
            &coeff_a0.xor_jit,
            &coeff_b0.xor_jit,
            &coeff_c0.xor_jit,
            &coeff_d0.xor_jit,
            &coeff_a1.xor_jit,
            &coeff_b1.xor_jit,
            &coeff_c1.xor_jit,
            &coeff_d1.xor_jit,
            &coeff_a2.xor_jit,
            &coeff_b2.xor_jit,
            &coeff_c2.xor_jit,
            &coeff_d2.xor_jit,
            &coeff_a3.xor_jit,
            &coeff_b3.xor_jit,
            &coeff_c3.xor_jit,
            &coeff_d3.xor_jit,
        ) {
            (
                Some(prepared_a0),
                Some(prepared_b0),
                Some(prepared_c0),
                Some(prepared_d0),
                Some(prepared_a1),
                Some(prepared_b1),
                Some(prepared_c1),
                Some(prepared_d1),
                Some(prepared_a2),
                Some(prepared_b2),
                Some(prepared_c2),
                Some(prepared_d2),
                Some(prepared_a3),
                Some(prepared_b3),
                Some(prepared_c3),
                Some(prepared_d3),
            ) => unsafe {
                process_slices_multiply_add_xor_jit_x4_inputs_x4_outputs(
                    input_a,
                    input_b,
                    input_c,
                    input_d,
                    prepared_a0,
                    prepared_b0,
                    prepared_c0,
                    prepared_d0,
                    output_a,
                    prepared_a1,
                    prepared_b1,
                    prepared_c1,
                    prepared_d1,
                    output_b,
                    prepared_a2,
                    prepared_b2,
                    prepared_c2,
                    prepared_d2,
                    output_c,
                    prepared_a3,
                    prepared_b3,
                    prepared_c3,
                    prepared_d3,
                    output_d,
                    flavor,
                );
            },
            _ => panic!("XOR-JIT create backend missing prepared coefficient"),
        }
        batch_idx += 4;
    }

    if batch_idx < job.batch_len {
        process_batch_add_avx2_xor_jit_x2_outputs(
            ComputeJob {
                batch_len: job.batch_len - batch_idx,
                input_base: unsafe {
                    (job.input_base as *const u8).add(batch_idx * job.aligned_chunk_len) as usize
                },
                source_indices: unsafe {
                    (job.source_indices as *const usize).add(batch_idx) as usize
                },
                ..job
            },
            recovery_a,
            output_a,
            recovery_b,
            output_b,
            flavor,
        );
        process_batch_add_avx2_xor_jit_x2_outputs(
            ComputeJob {
                batch_len: job.batch_len - batch_idx,
                input_base: unsafe {
                    (job.input_base as *const u8).add(batch_idx * job.aligned_chunk_len) as usize
                },
                source_indices: unsafe {
                    (job.source_indices as *const usize).add(batch_idx) as usize
                },
                ..job
            },
            recovery_c,
            output_c,
            recovery_d,
            output_d,
            flavor,
        );
    }
}

#[cfg(target_arch = "x86_64")]
fn process_batch_add_avx2_xor_jit_x2_outputs(
    job: ComputeJob,
    recovery_a: usize,
    output_a: &mut [u8],
    recovery_b: usize,
    output_b: &mut [u8],
    flavor: XorJitFlavor,
) {
    let mut batch_idx = 0;
    while batch_idx + 3 < job.batch_len {
        let source_a = unsafe { *(job.source_indices as *const usize).add(batch_idx) };
        let source_b = unsafe { *(job.source_indices as *const usize).add(batch_idx + 1) };
        let source_c = unsafe { *(job.source_indices as *const usize).add(batch_idx + 2) };
        let source_d = unsafe { *(job.source_indices as *const usize).add(batch_idx + 3) };
        let coeff_a0 = coeff_for(job, recovery_a, source_a);
        let coeff_b0 = coeff_for(job, recovery_a, source_b);
        let coeff_c0 = coeff_for(job, recovery_a, source_c);
        let coeff_d0 = coeff_for(job, recovery_a, source_d);
        let coeff_a1 = coeff_for(job, recovery_b, source_a);
        let coeff_b1 = coeff_for(job, recovery_b, source_b);
        let coeff_c1 = coeff_for(job, recovery_b, source_c);
        let coeff_d1 = coeff_for(job, recovery_b, source_d);
        let input_a = input_segment(job, batch_idx);
        let input_b = input_segment(job, batch_idx + 1);
        let input_c = input_segment(job, batch_idx + 2);
        let input_d = input_segment(job, batch_idx + 3);

        match (
            &coeff_a0.xor_jit,
            &coeff_b0.xor_jit,
            &coeff_c0.xor_jit,
            &coeff_d0.xor_jit,
            &coeff_a1.xor_jit,
            &coeff_b1.xor_jit,
            &coeff_c1.xor_jit,
            &coeff_d1.xor_jit,
        ) {
            (
                Some(prepared_a0),
                Some(prepared_b0),
                Some(prepared_c0),
                Some(prepared_d0),
                Some(prepared_a1),
                Some(prepared_b1),
                Some(prepared_c1),
                Some(prepared_d1),
            ) => unsafe {
                process_slices_multiply_add_xor_jit_x4_inputs_x2_outputs(
                    input_a,
                    input_b,
                    input_c,
                    input_d,
                    prepared_a0,
                    prepared_b0,
                    prepared_c0,
                    prepared_d0,
                    output_a,
                    prepared_a1,
                    prepared_b1,
                    prepared_c1,
                    prepared_d1,
                    output_b,
                    flavor,
                );
            },
            _ => panic!("XOR-JIT create backend missing prepared coefficient"),
        }
        batch_idx += 4;
    }

    while batch_idx + 1 < job.batch_len {
        let source_a = unsafe { *(job.source_indices as *const usize).add(batch_idx) };
        let source_b = unsafe { *(job.source_indices as *const usize).add(batch_idx + 1) };
        let coeff_a0 = coeff_for(job, recovery_a, source_a);
        let coeff_b0 = coeff_for(job, recovery_a, source_b);
        let coeff_a1 = coeff_for(job, recovery_b, source_a);
        let coeff_b1 = coeff_for(job, recovery_b, source_b);
        let input_a = input_segment(job, batch_idx);
        let input_b = input_segment(job, batch_idx + 1);

        match (
            &coeff_a0.xor_jit,
            &coeff_b0.xor_jit,
            &coeff_a1.xor_jit,
            &coeff_b1.xor_jit,
        ) {
            (Some(prepared_a0), Some(prepared_b0), Some(prepared_a1), Some(prepared_b1)) => unsafe {
                process_slices_multiply_add_xor_jit_x2(
                    input_a,
                    prepared_a0,
                    input_b,
                    prepared_b0,
                    output_a,
                    flavor,
                );
                process_slices_multiply_add_xor_jit_x2(
                    input_a,
                    prepared_a1,
                    input_b,
                    prepared_b1,
                    output_b,
                    flavor,
                );
            },
            _ => panic!("XOR-JIT create backend missing prepared coefficient"),
        }
        batch_idx += 2;
    }

    while batch_idx < job.batch_len {
        let source_idx = unsafe { *(job.source_indices as *const usize).add(batch_idx) };
        let coeff_a = coeff_for(job, recovery_a, source_idx);
        let coeff_b = coeff_for(job, recovery_b, source_idx);
        let input = input_segment(job, batch_idx);
        match (&coeff_a.xor_jit, &coeff_b.xor_jit) {
            (Some(prepared_a), Some(prepared_b)) => unsafe {
                process_slice_multiply_add_xor_jit(input, output_a, prepared_a, flavor);
                process_slice_multiply_add_xor_jit(input, output_b, prepared_b, flavor);
            },
            _ => panic!("XOR-JIT create backend missing prepared coefficient"),
        }
        batch_idx += 1;
    }
}

#[cfg(target_arch = "x86_64")]
fn process_batch_add_avx2_xor_jit(
    job: ComputeJob,
    recovery_idx: usize,
    output: &mut [u8],
    flavor: XorJitFlavor,
) {
    let mut batch_idx = 0;
    while batch_idx + 3 < job.batch_len {
        let source_a = unsafe { *(job.source_indices as *const usize).add(batch_idx) };
        let source_b = unsafe { *(job.source_indices as *const usize).add(batch_idx + 1) };
        let source_c = unsafe { *(job.source_indices as *const usize).add(batch_idx + 2) };
        let source_d = unsafe { *(job.source_indices as *const usize).add(batch_idx + 3) };
        let coeff_a = coeff_for(job, recovery_idx, source_a);
        let coeff_b = coeff_for(job, recovery_idx, source_b);
        let coeff_c = coeff_for(job, recovery_idx, source_c);
        let coeff_d = coeff_for(job, recovery_idx, source_d);
        let input_a = input_segment(job, batch_idx);
        let input_b = input_segment(job, batch_idx + 1);
        let input_c = input_segment(job, batch_idx + 2);
        let input_d = input_segment(job, batch_idx + 3);

        match (
            &coeff_a.xor_jit,
            &coeff_b.xor_jit,
            &coeff_c.xor_jit,
            &coeff_d.xor_jit,
        ) {
            (Some(prepared_a), Some(prepared_b), Some(prepared_c), Some(prepared_d)) => unsafe {
                process_slices_multiply_add_xor_jit_x4(
                    input_a, prepared_a, input_b, prepared_b, input_c, prepared_c, input_d,
                    prepared_d, output, flavor,
                );
            },
            _ => panic!("XOR-JIT create backend missing prepared coefficient"),
        }
        batch_idx += 4;
    }

    while batch_idx + 1 < job.batch_len {
        let source_a = unsafe { *(job.source_indices as *const usize).add(batch_idx) };
        let source_b = unsafe { *(job.source_indices as *const usize).add(batch_idx + 1) };
        let coeff_a = coeff_for(job, recovery_idx, source_a);
        let coeff_b = coeff_for(job, recovery_idx, source_b);
        let input_a = input_segment(job, batch_idx);
        let input_b = input_segment(job, batch_idx + 1);

        match (&coeff_a.xor_jit, &coeff_b.xor_jit) {
            (Some(prepared_a), Some(prepared_b)) => unsafe {
                process_slices_multiply_add_xor_jit_x2(
                    input_a, prepared_a, input_b, prepared_b, output, flavor,
                );
            },
            _ => panic!("XOR-JIT create backend missing prepared coefficient"),
        }
        batch_idx += 2;
    }

    while batch_idx < job.batch_len {
        let source_idx = unsafe { *(job.source_indices as *const usize).add(batch_idx) };
        let coeff = coeff_for(job, recovery_idx, source_idx);
        let input = input_segment(job, batch_idx);
        match &coeff.xor_jit {
            Some(prepared) => unsafe {
                process_slice_multiply_add_xor_jit(input, output, prepared, flavor);
            },
            None => panic!("XOR-JIT create backend missing prepared coefficient"),
        }
        batch_idx += 1;
    }
}

#[cfg(target_arch = "x86_64")]
fn process_batch_add_avx2_pshufb(job: ComputeJob, recovery_idx: usize, output: &mut [u8]) {
    let mut batch_idx = 0;
    while PSHUFB_PACKED_INPUTS >= 4 && batch_idx + 3 < job.batch_len {
        let source_a = unsafe { *(job.source_indices as *const usize).add(batch_idx) };
        let source_b = unsafe { *(job.source_indices as *const usize).add(batch_idx + 1) };
        let source_c = unsafe { *(job.source_indices as *const usize).add(batch_idx + 2) };
        let source_d = unsafe { *(job.source_indices as *const usize).add(batch_idx + 3) };
        let coeff_a = unsafe {
            &*(job.coeffs as *const CreateCoeff).add(gf_coeff_index(
                recovery_idx,
                source_a,
                job.source_count,
            ))
        };
        let coeff_b = unsafe {
            &*(job.coeffs as *const CreateCoeff).add(gf_coeff_index(
                recovery_idx,
                source_b,
                job.source_count,
            ))
        };
        let coeff_c = unsafe {
            &*(job.coeffs as *const CreateCoeff).add(gf_coeff_index(
                recovery_idx,
                source_c,
                job.source_count,
            ))
        };
        let coeff_d = unsafe {
            &*(job.coeffs as *const CreateCoeff).add(gf_coeff_index(
                recovery_idx,
                source_d,
                job.source_count,
            ))
        };
        let input_a = input_segment(job, batch_idx);
        let input_b = input_segment(job, batch_idx + 1);
        let input_c = input_segment(job, batch_idx + 2);
        let input_d = input_segment(job, batch_idx + 3);

        match (&coeff_a.avx2, &coeff_b.avx2, &coeff_c.avx2, &coeff_d.avx2) {
            (Some(prepared_a), Some(prepared_b), Some(prepared_c), Some(prepared_d)) => unsafe {
                process_slices_multiply_add_prepared_avx2_x4(
                    input_a,
                    prepared_a,
                    &coeff_a.split,
                    input_b,
                    prepared_b,
                    &coeff_b.split,
                    input_c,
                    prepared_c,
                    &coeff_c.split,
                    input_d,
                    prepared_d,
                    &coeff_d.split,
                    output,
                );
            },
            _ => {
                process_slice_multiply_add(input_a, output, &coeff_a.split);
                process_slice_multiply_add(input_b, output, &coeff_b.split);
                process_slice_multiply_add(input_c, output, &coeff_c.split);
                process_slice_multiply_add(input_d, output, &coeff_d.split);
            }
        }
        batch_idx += 4;
    }

    while PSHUFB_PACKED_INPUTS >= 2 && batch_idx + 1 < job.batch_len {
        let source_a = unsafe { *(job.source_indices as *const usize).add(batch_idx) };
        let source_b = unsafe { *(job.source_indices as *const usize).add(batch_idx + 1) };
        let coeff_a = unsafe {
            &*(job.coeffs as *const CreateCoeff).add(gf_coeff_index(
                recovery_idx,
                source_a,
                job.source_count,
            ))
        };
        let coeff_b = unsafe {
            &*(job.coeffs as *const CreateCoeff).add(gf_coeff_index(
                recovery_idx,
                source_b,
                job.source_count,
            ))
        };
        let input_a = input_segment(job, batch_idx);
        let input_b = input_segment(job, batch_idx + 1);

        match (&coeff_a.avx2, &coeff_b.avx2) {
            (Some(prepared_a), Some(prepared_b)) => unsafe {
                process_slices_multiply_add_prepared_avx2_x2(
                    input_a,
                    prepared_a,
                    &coeff_a.split,
                    input_b,
                    prepared_b,
                    &coeff_b.split,
                    output,
                );
            },
            _ => {
                process_slice_multiply_add(input_a, output, &coeff_a.split);
                process_slice_multiply_add(input_b, output, &coeff_b.split);
            }
        }
        batch_idx += 2;
    }

    while batch_idx < job.batch_len {
        let source_idx = unsafe { *(job.source_indices as *const usize).add(batch_idx) };
        let coeff = unsafe {
            &*(job.coeffs as *const CreateCoeff).add(gf_coeff_index(
                recovery_idx,
                source_idx,
                job.source_count,
            ))
        };
        let input = input_segment(job, batch_idx);
        match &coeff.avx2 {
            Some(prepared) => unsafe {
                process_slice_multiply_add_prepared_avx2(input, output, prepared, &coeff.split);
            },
            None => process_slice_multiply_add(input, output, &coeff.split),
        }
        batch_idx += 1;
    }
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn input_segment(job: ComputeJob, batch_idx: usize) -> &'static [u8] {
    let start = batch_idx * job.aligned_chunk_len + job.segment_start;
    unsafe { std::slice::from_raw_parts((job.input_base as *const u8).add(start), job.segment_len) }
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn coeff_for(job: ComputeJob, recovery_idx: usize, source_idx: usize) -> &'static CreateCoeff {
    unsafe {
        &*(job.coeffs as *const CreateCoeff).add(gf_coeff_index(
            recovery_idx,
            source_idx,
            job.source_count,
        ))
    }
}

#[inline]
pub fn gf_coeff_index(recovery_idx: usize, source_idx: usize, source_count: usize) -> usize {
    recovery_idx * source_count + source_idx
}

#[inline]
fn input_grouping(source_count: usize, method: CreateGf16Method) -> usize {
    let default = DEFAULT_INPUT_GROUPING;
    let small = source_count.div_ceil(2).max(1);
    let requested = if small < default { small } else { default };
    let multiple = method.ideal_input_multiple();
    align_up(requested, multiple)
        .max(multiple)
        .min(source_count.max(1))
}

#[inline]
fn max_compute_jobs(
    max_chunk_len: usize,
    recovery_count: usize,
    worker_count: usize,
    method: CreateGf16Method,
) -> usize {
    let segment_len = align_down(
        method
            .ideal_segment_len()
            .min(max_chunk_len.max(AVX2_ALIGNMENT))
            .max(AVX2_ALIGNMENT),
        AVX2_ALIGNMENT,
    )
    .max(AVX2_ALIGNMENT);
    let segment_count = max_chunk_len.max(1).div_ceil(segment_len);
    #[cfg(target_arch = "x86_64")]
    if method.xor_jit_flavor().is_some() {
        if segment_count >= worker_count {
            return worker_count.min(segment_count).max(1);
        }

        let leftover_chunks = segment_count % worker_count.max(1);
        let full_chunks = segment_count / worker_count.max(1);
        let leftover_jobs = if leftover_chunks == 0 {
            0
        } else {
            let threads_per_chunk = (worker_count / leftover_chunks).min(recovery_count).max(1);
            leftover_chunks * threads_per_chunk
        };
        let full_chunk_jobs = if full_chunks == 0 {
            0
        } else {
            worker_count.max(1)
        };
        return (leftover_jobs + full_chunk_jobs).max(1);
    }

    let output_groups = worker_count.min(recovery_count).max(1);
    (segment_count * output_groups).max(1)
}

#[inline]
fn align_up(value: usize, alignment: usize) -> usize {
    if value == 0 {
        0
    } else {
        value.div_ceil(alignment) * alignment
    }
}

#[inline]
fn align_down(value: usize, alignment: usize) -> usize {
    value & !(alignment - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reed_solomon::RecoveryBlockEncoder;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn deterministic_inputs(source_count: usize, block_size: usize) -> Vec<Vec<u8>> {
        (0..source_count)
            .map(|src| {
                (0..block_size)
                    .map(|byte| (src * 31 + byte * 17 + (byte / 7)) as u8)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    }

    fn assert_forced_backend_matches_encoder(method: &str, expected_method: CreateGf16Method) {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("PAR2RS_CREATE_GF16", method);

        let block_size = 1024;
        let source_count = 12;
        let recovery_count = 5;
        let encoder = RecoveryBlockEncoder::new(block_size, source_count);
        let inputs = deterministic_inputs(source_count, block_size);
        let mut backend =
            CreateRecoveryBackend::new(encoder.base_values(), 3, recovery_count, block_size, 4);
        std::env::remove_var("PAR2RS_CREATE_GF16");

        if backend.selected_method() != expected_method {
            return;
        }

        let mut recovery_blocks = backend.recovery_blocks(block_size);
        backend.begin_chunk(block_size);
        inputs
            .iter()
            .enumerate()
            .for_each(|(idx, input)| backend.add_input(idx, input));
        assert!(backend.finish_chunk(&mut recovery_blocks, block_size));

        recovery_blocks
            .iter()
            .for_each(|(exponent, recovery_data)| {
                let refs = inputs.iter().map(Vec::as_slice).collect::<Vec<_>>();
                let expected = encoder.encode_recovery_block(*exponent, &refs).unwrap();
                assert_eq!(recovery_data, &expected);
            });
    }

    #[test]
    fn backend_output_matches_encoder_for_partial_batch() {
        let block_size = 64;
        let source_count = 5;
        let encoder = RecoveryBlockEncoder::new(block_size, source_count);
        let inputs = deterministic_inputs(source_count, block_size);

        let mut backend = CreateRecoveryBackend::new(encoder.base_values(), 0, 3, block_size, 2);
        let mut recovery_blocks = backend.recovery_blocks(block_size);
        backend.begin_chunk(block_size);
        inputs
            .iter()
            .enumerate()
            .for_each(|(idx, input)| backend.add_input(idx, input));
        assert!(backend.finish_chunk(&mut recovery_blocks, block_size));

        recovery_blocks
            .iter()
            .for_each(|(exponent, recovery_data)| {
                let refs = inputs.iter().map(Vec::as_slice).collect::<Vec<_>>();
                let expected = encoder.encode_recovery_block(*exponent, &refs).unwrap();
                assert_eq!(recovery_data, &expected);
            });
    }

    #[test]
    fn forced_pshufb_backend_matches_encoder_for_full_batch() {
        assert_forced_backend_matches_encoder("pshufb", CreateGf16Method::Avx2PshufbPrepared);
    }

    #[test]
    fn forced_xor_jit_backend_matches_encoder_for_full_batch() {
        assert_forced_backend_matches_encoder("xor-jit", CreateGf16Method::Avx2XorJit);
    }

    #[test]
    fn forced_xor_jit_backend_uses_bitplane_without_legacy_fallback() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("PAR2RS_CREATE_GF16", "xor-jit");

        let encoder = RecoveryBlockEncoder::new(1024, 12);
        let backend = CreateRecoveryBackend::new(encoder.base_values(), 0, 5, 1024, 4);
        std::env::remove_var("PAR2RS_CREATE_GF16");

        if backend.selected_method() != CreateGf16Method::Avx2XorJit {
            return;
        }

        assert!(backend.coeffs.iter().all(|coeff| coeff.xor_jit.is_some()));
        assert!(backend
            .coeffs
            .iter()
            .all(|coeff| coeff.xor_jit_bitplane.is_some()));
    }

    #[test]
    fn backend_reuses_fixed_transfer_buffers() {
        let encoder = RecoveryBlockEncoder::new(64, 2);
        let mut backend = CreateRecoveryBackend::new(encoder.base_values(), 7, 1, 64, 1);
        backend.begin_chunk(32);
        let first = backend.prepare_transfer_buffer(0).as_ptr();
        let second = backend.prepare_transfer_buffer(1).as_ptr();
        let first_again = backend.prepare_transfer_buffer(2).as_ptr();

        assert_ne!(first, second);
        assert_eq!(first, first_again);
        assert_eq!(first as usize % 32, 0);
        assert_eq!(second as usize % 32, 0);
    }

    #[test]
    fn env_method_override_selects_scalar() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("PAR2RS_CREATE_GF16", "scalar");
        let encoder = RecoveryBlockEncoder::new(64, 2);
        let backend = CreateRecoveryBackend::new(encoder.base_values(), 0, 1, 64, 1);
        std::env::remove_var("PAR2RS_CREATE_GF16");
        assert_eq!(backend.selected_method(), CreateGf16Method::Scalar);
    }

    #[test]
    fn recovery_vectors_keep_exact_reserved_capacity() {
        let encoder = RecoveryBlockEncoder::new(96, 4);
        let backend = CreateRecoveryBackend::new(encoder.base_values(), 0, 2, 32, 1);
        let blocks = backend.recovery_blocks(96);
        assert!(blocks.iter().all(|(_, bytes)| bytes.capacity() == 96));
    }

    #[test]
    fn input_grouping_handles_non_power_of_two_multiple() {
        assert_eq!(align_up(12, 12), 12);
        assert_eq!(align_up(13, 12), 24);
        assert_eq!(input_grouping(256, CreateGf16Method::Avx2XorJit), 12);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn xor_jit_bitplane_layout_offsets_are_segment_major() {
        let layout = XorJitBitplaneLayout::new(1024, 512, 3, 4);
        let aligned_current_slice_size = layout.aligned_slice_len;
        let first_slice_offset = layout.slice_offset(0);
        let second_slice_offset = layout.slice_offset(1);
        let first_proc_size =
            layout.segment_len_for(first_slice_offset, aligned_current_slice_size);
        let second_proc_size =
            layout.segment_len_for(second_slice_offset, aligned_current_slice_size);

        assert_eq!(
            layout.input_offset(first_slice_offset, 0, first_proc_size),
            0
        );
        assert_eq!(
            layout.input_offset(first_slice_offset, 1, first_proc_size),
            512
        );
        assert_eq!(
            layout.input_offset(first_slice_offset, 2, first_proc_size),
            1024
        );
        assert_eq!(
            layout.input_offset(second_slice_offset, 0, second_proc_size),
            1536
        );
        assert_eq!(
            layout.input_offset(second_slice_offset, 1, second_proc_size),
            2048
        );

        assert_eq!(
            layout.output_offset(first_slice_offset, 0, first_proc_size),
            0
        );
        assert_eq!(
            layout.output_offset(first_slice_offset, 1, first_proc_size),
            512
        );
        assert_eq!(
            layout.output_offset(first_slice_offset, 3, first_proc_size),
            1536
        );
        assert_eq!(
            layout.output_offset(second_slice_offset, 0, second_proc_size),
            2048
        );
        assert_eq!(
            layout.output_offset(second_slice_offset, 1, second_proc_size),
            2560
        );
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn xor_jit_bitplane_layout_offsets_do_not_overlap() {
        use std::collections::HashSet;

        let layout = XorJitBitplaneLayout::new(2048, 512, 12, 5);
        let aligned_current_slice_size = layout.aligned_slice_len;
        let segment_count = layout.segment_count_for(aligned_current_slice_size);
        let mut input_offsets = HashSet::new();
        for segment_idx in 0..segment_count {
            let slice_offset = layout.slice_offset(segment_idx);
            let proc_size = layout.segment_len_for(slice_offset, aligned_current_slice_size);
            for batch_idx in 0..layout.input_batch_size {
                assert!(input_offsets.insert(layout.input_offset(
                    slice_offset,
                    batch_idx,
                    proc_size
                )));
            }
        }
        assert_eq!(input_offsets.len(), segment_count * layout.input_batch_size);

        let mut output_offsets = HashSet::new();
        for segment_idx in 0..segment_count {
            let slice_offset = layout.slice_offset(segment_idx);
            let proc_size = layout.segment_len_for(slice_offset, aligned_current_slice_size);
            for recovery_idx in 0..layout.recovery_count {
                assert!(output_offsets.insert(layout.output_offset(
                    slice_offset,
                    recovery_idx,
                    proc_size
                )));
            }
        }
        assert_eq!(output_offsets.len(), segment_count * layout.recovery_count);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn xor_jit_bitplane_layout_handles_partial_final_segment() {
        let layout = XorJitBitplaneLayout::new(1024, 1024, 3, 2);

        assert_eq!(layout.segment_count_for(layout.aligned_slice_len), 2);
        assert_eq!(layout.slice_offset(0), 0);
        assert_eq!(layout.slice_offset(1), 1024);
        assert_eq!(layout.segment_len_for(0, layout.aligned_slice_len), 1024);
        assert_eq!(layout.segment_len_for(1024, layout.aligned_slice_len), 512);
        assert_eq!(layout.input_storage_len(), 3 * 1024 + 3 * 512);
        assert_eq!(layout.output_storage_len(), 2 * 1024 + 2 * 512);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn xor_jit_bitplane_output_prefetch_follows_adjacent_output_storage() {
        let layout = XorJitBitplaneLayout::new(2048, 1024, 12, 3);
        let job = ComputeJob {
            segment_len: 1024,
            xor_jit_segment_len: layout.chunk_len,
            xor_jit_input_grouping: layout.input_batch_size,
            ..ComputeJob::default()
        };

        assert_eq!(
            xor_jit_bitplane_output_prefetch_ptr(job, layout, 0, 1, 0).map(|ptr| ptr as usize),
            Some(layout.output_offset(0, 2, 1024))
        );
        assert_eq!(
            xor_jit_bitplane_output_prefetch_ptr(job, layout, 0, 2, 0).map(|ptr| ptr as usize),
            Some(layout.output_offset(1024, 0, 512))
        );
        assert_eq!(
            xor_jit_bitplane_output_prefetch_ptr(job, layout, 1, 2, 0).map(|ptr| ptr as usize),
            Some(layout.output_offset(2048, 0, 512))
        );
        assert_eq!(
            xor_jit_bitplane_output_prefetch_ptr(job, layout, 2, 2, 0).map(|ptr| ptr as usize),
            None
        );
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn xor_jit_bitplane_input_prefetch_uses_turbo_stream_offsets() {
        let segment_len = 128 * 1024;
        let layout = XorJitBitplaneLayout::new(segment_len * 2, segment_len, 12, 512);
        let job = ComputeJob {
            input_base: 4096,
            output_start: 0,
            output_end: 512,
            batch_len: 12,
            segment_len,
            xor_jit_segment_len: layout.chunk_len,
            xor_jit_input_grouping: layout.input_batch_size,
            ..ComputeJob::default()
        };

        assert_eq!(
            xor_jit_bitplane_input_prefetch_ptr(job, layout, 0, 508, 2).map(|ptr| ptr as usize),
            None
        );
        assert_eq!(
            xor_jit_bitplane_input_prefetch_ptr(job, layout, 0, 509, 2).map(|ptr| ptr as usize),
            Some(4096 + layout.input_offset(segment_len, 0, segment_len))
        );
        assert_eq!(
            xor_jit_bitplane_input_prefetch_ptr(job, layout, 0, 509, 11).map(|ptr| ptr as usize),
            Some(
                4096 + layout.input_offset(segment_len, 0, segment_len)
                    + 9 * (segment_len >> XOR_JIT_PREFETCH_DOWNSCALE)
            )
        );
        assert_eq!(
            xor_jit_bitplane_input_prefetch_ptr(job, layout, 0, 510, 2).map(|ptr| ptr as usize),
            Some(
                4096 + layout.input_offset(segment_len, 0, segment_len)
                    + 10 * (segment_len >> XOR_JIT_PREFETCH_DOWNSCALE)
            )
        );
        assert_eq!(
            xor_jit_bitplane_input_prefetch_ptr(job, layout, 0, 511, 11).map(|ptr| ptr as usize),
            Some(
                4096 + layout.input_offset(segment_len, 0, segment_len)
                    + 29 * (segment_len >> XOR_JIT_PREFETCH_DOWNSCALE)
            )
        );
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn forced_xor_jit_backend_uses_packed_bitplane_layout() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("PAR2RS_CREATE_GF16", "xor-jit");

        let encoder = RecoveryBlockEncoder::new(1024 * 1024, 24);
        let backend = CreateRecoveryBackend::new(encoder.base_values(), 0, 7, 1024 * 1024, 16);
        std::env::remove_var("PAR2RS_CREATE_GF16");

        if backend.selected_method() != CreateGf16Method::Avx2XorJit {
            return;
        }

        let layout = backend
            .xor_jit_layout
            .expect("forced XOR-JIT should initialize packed layout");
        assert_eq!(layout.chunk_len, 128 * 1024);
        assert_eq!(layout.segment_count_for(layout.aligned_slice_len), 9);
        assert_eq!(
            layout.input_offset(layout.chunk_len, 0, layout.chunk_len),
            backend.input_grouping * layout.chunk_len
        );
        assert_eq!(
            layout.output_offset(layout.chunk_len, 0, layout.chunk_len),
            7 * layout.chunk_len
        );
        assert_eq!(backend.staging[0].inputs.len(), layout.input_storage_len());
        assert_eq!(backend.output_chunks.len(), layout.output_storage_len());
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn forced_xor_jit_backend_honors_segment_len_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("PAR2RS_CREATE_GF16", "xor-jit");
        std::env::set_var(XOR_JIT_SEGMENT_LEN_ENV, "65536");

        let encoder = RecoveryBlockEncoder::new(1024 * 1024, 24);
        let backend = CreateRecoveryBackend::new(encoder.base_values(), 0, 7, 1024 * 1024, 16);
        std::env::remove_var(XOR_JIT_SEGMENT_LEN_ENV);
        std::env::remove_var("PAR2RS_CREATE_GF16");

        if backend.selected_method() != CreateGf16Method::Avx2XorJit {
            return;
        }

        let layout = backend
            .xor_jit_layout
            .expect("forced XOR-JIT should initialize packed layout");
        assert_eq!(layout.chunk_len, 64 * 1024);
        assert_eq!(layout.segment_count_for(layout.aligned_slice_len), 17);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    #[ignore]
    fn dump_xor_jit_prepared_staging_for_compare() {
        let output_path = std::env::var("PAR2RS_XOR_JIT_PREPARED_DUMP_PATH")
            .expect("PAR2RS_XOR_JIT_PREPARED_DUMP_PATH");
        let slice_len = std::env::var("PAR2RS_XOR_JIT_PREPARED_SLICE_LEN")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1024 * 1024);
        let chunk_len = std::env::var("PAR2RS_XOR_JIT_PREPARED_CHUNK_LEN")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(128 * 1024);
        let input_grouping = std::env::var("PAR2RS_XOR_JIT_PREPARED_INPUT_GROUPING")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(12);
        let slot = std::env::var("PAR2RS_XOR_JIT_PREPARED_SLOT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(5);
        let src_len = std::env::var("PAR2RS_XOR_JIT_PREPARED_SRC_LEN")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(slice_len);

        let layout = XorJitBitplaneLayout::new(slice_len, chunk_len, input_grouping, 1);
        let mut staging = StagingArea::new(input_grouping, layout.input_storage_len(), 1);
        let input = (0..src_len)
            .map(|idx| ((idx * 37 + 11) & 0xff) as u8)
            .collect::<Vec<_>>();

        prepare_xor_jit_bitplane_staging(layout, &mut staging, slot, slice_len, &input);
        std::fs::write(output_path, &staging.inputs[..]).expect("write prepared staging dump");
    }
}
