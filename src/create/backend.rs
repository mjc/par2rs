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
    detect_simd_support, prepare_avx2_coeff, process_slice_multiply_add_prepared_avx2,
    process_slice_multiply_add_xor_jit, process_slices_multiply_add_prepared_avx2_x2,
    process_slices_multiply_add_prepared_avx2_x4, process_slices_multiply_add_xor_jit_x2,
    process_slices_multiply_add_xor_jit_x4,
    process_slices_multiply_add_xor_jit_x4_inputs_x2_outputs,
    process_slices_multiply_add_xor_jit_x4_inputs_x4_outputs, Avx2PreparedCoeff, SimdLevel,
    XorJitFlavor, XorJitPreparedCoeff,
};

const DEFAULT_INPUT_GROUPING: usize = 12;
const TRANSFER_BUFFER_COUNT: usize = 2;
const CREATE_SEGMENT_SIZE: usize = 256 * 1024;
// The prepared x1 PSHUFB path currently retires fewer instructions on the
// large-file create proxy than the x2/x4 packed kernels on this CPU.
const PSHUFB_PACKED_INPUTS: usize = 1;
const AVX2_ALIGNMENT: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateGf16Method {
    Auto,
    Avx2PshufbPrepared,
    Avx2XorJitPort,
    Avx2XorJitClean,
    Scalar,
}

impl CreateGf16Method {
    fn from_env() -> Self {
        match std::env::var("PAR2RS_CREATE_GF16") {
            Ok(value) => match value.to_ascii_lowercase().as_str() {
                "auto" => Self::Auto,
                "pshufb" | "avx2-pshufb" | "avx2_pshufb" => Self::Avx2PshufbPrepared,
                "xor-jit" | "xor_jit" | "xorit" | "avx2-xor-jit" | "xor-jit-port"
                | "xor_jit_port" | "avx2-xor-jit-port" => Self::Avx2XorJitPort,
                "xor-jit-clean" | "xor_jit_clean" | "avx2-xor-jit-clean" => Self::Avx2XorJitClean,
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
            Self::Avx2XorJitPort | Self::Avx2XorJitClean => {
                #[cfg(target_arch = "x86_64")]
                {
                    if matches!(detect_simd_support(), SimdLevel::Avx2)
                        && is_x86_feature_detected!("vpclmulqdq")
                    {
                        return self;
                    }
                }
                if xor_jit_fallback_is_error() {
                    panic!(
                        "forced XOR-JIT create backend requires x86_64 AVX2 and VPCLMULQDQ support"
                    );
                }
                Self::Scalar
            }
            Self::Scalar => Self::Scalar,
        }
    }

    #[inline]
    fn ideal_input_multiple(self) -> usize {
        match self {
            Self::Auto | Self::Avx2PshufbPrepared => 4,
            // Match par2cmdline-turbo's default CPU controller target: round
            // a 12-input staging batch to the method's ideal multiple. AVX2
            // XOR-JIT has an ideal multiple of 1, so turbo stages 12 inputs.
            Self::Avx2XorJitPort | Self::Avx2XorJitClean => 12,
            Self::Scalar => 1,
        }
    }

    #[inline]
    fn ideal_segment_len(self) -> usize {
        match self {
            Self::Auto | Self::Avx2PshufbPrepared => CREATE_SEGMENT_SIZE,
            // Turbo's AVX2 XOR-JIT method reports a 128KiB ideal chunk. Keep
            // the port/clean segmentation aligned with that until the full
            // transformed XOR-JIT kernel is ported.
            Self::Avx2XorJitPort | Self::Avx2XorJitClean => 128 * 1024,
            Self::Scalar => CREATE_SEGMENT_SIZE / 2,
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    fn xor_jit_flavor(self) -> Option<XorJitFlavor> {
        match self {
            Self::Avx2XorJitPort => Some(XorJitFlavor::Port),
            Self::Avx2XorJitClean => Some(XorJitFlavor::Clean),
            _ => None,
        }
    }
}

#[inline]
fn xor_jit_fallback_is_error() -> bool {
    std::env::var("PAR2RS_CREATE_XOR_JIT_FALLBACK")
        .map(|value| value.eq_ignore_ascii_case("error"))
        .unwrap_or(false)
}

/// Prepared coefficient for one `(recovery output, source input)` pair.
pub struct CreateCoeff {
    pub value: u16,
    pub split: SplitMulTable,
    #[cfg(target_arch = "x86_64")]
    pub avx2: Option<Avx2PreparedCoeff>,
    #[cfg(target_arch = "x86_64")]
    pub xor_jit: Option<XorJitPreparedCoeff>,
}

pub type Gf16Coeff = CreateCoeff;

impl CreateCoeff {
    #[inline]
    fn new(value: u16, prepare_pshufb: bool) -> Self {
        let split = build_split_mul_table(Galois16::new(value));
        #[cfg(target_arch = "x86_64")]
        let avx2 = prepare_pshufb.then(|| prepare_avx2_coeff(&split));
        #[cfg(target_arch = "x86_64")]
        let xor_jit = Some(XorJitPreparedCoeff::new(value));

        Self {
            value,
            split,
            #[cfg(target_arch = "x86_64")]
            avx2,
            #[cfg(target_arch = "x86_64")]
            xor_jit,
        }
    }
}

pub struct StagingArea {
    inputs: AlignedVec,
    source_indices: Vec<usize>,
    batch_len: usize,
}

impl StagingArea {
    fn new(input_grouping: usize, aligned_chunk_len: usize) -> Self {
        Self {
            inputs: AlignedVec::new_zeroed(input_grouping * aligned_chunk_len),
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

/// Create-side recovery backend with all hot-path storage owned up front.
pub struct CreateRecoveryBackend {
    pub source_count: usize,
    pub recovery_exponents: Vec<u16>,
    pub max_chunk_len: usize,
    pub chunk_len: usize,
    pub method: CreateGf16Method,
    pub input_grouping: usize,
    transfer_buffers: [AlignedVec; TRANSFER_BUFFER_COUNT],
    pub staging: Vec<StagingArea>,
    pub output_chunks: AlignedVec,
    pub coeffs: Vec<CreateCoeff>,
    workers: CreateWorkerPool,
    aligned_chunk_len: usize,
    active_staging: usize,
    compute_in_flight: bool,
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
        let aligned_chunk_len = align_up(max_chunk_len, AVX2_ALIGNMENT);
        let input_grouping = input_grouping(source_count, method);
        let recovery_exponents = (0..recovery_count)
            .map(|offset| (first_recovery_block + offset as u32) as u16)
            .collect::<Vec<_>>();
        let prepare_pshufb = matches!(method, CreateGf16Method::Avx2PshufbPrepared);
        let coeffs = recovery_exponents
            .iter()
            .flat_map(|&exponent| {
                base_values
                    .iter()
                    .map(move |&base| Galois16::new(base).pow(exponent).value())
            })
            .map(|value| CreateCoeff::new(value, prepare_pshufb))
            .collect::<Vec<_>>();
        let worker_count = thread_count.max(1);
        let max_job_count = max_compute_jobs(max_chunk_len, recovery_count, worker_count, method);

        Self {
            source_count,
            recovery_exponents,
            max_chunk_len,
            chunk_len: 0,
            method,
            input_grouping,
            transfer_buffers: [
                AlignedVec::new_zeroed(aligned_chunk_len),
                AlignedVec::new_zeroed(aligned_chunk_len),
            ],
            staging: vec![
                StagingArea::new(input_grouping, aligned_chunk_len),
                StagingArea::new(input_grouping, aligned_chunk_len),
            ],
            output_chunks: AlignedVec::new_zeroed(recovery_count * aligned_chunk_len),
            coeffs,
            workers: CreateWorkerPool::new(worker_count, max_job_count),
            aligned_chunk_len,
            active_staging: 0,
            compute_in_flight: false,
            job_storage: vec![ComputeJob::default(); max_job_count],
        }
    }

    #[inline]
    pub fn begin_chunk(&mut self, chunk_len: usize) {
        self.chunk_len = chunk_len;
        self.active_staging = 0;
        debug_assert!(!self.compute_in_flight);
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
        debug_assert!(
            self.output_chunks.len() >= self.recovery_exponents.len() * self.aligned_chunk_len
        );
        debug_assert!(self.workers.capacity() >= self.job_storage.len());
        #[cfg(target_arch = "x86_64")]
        debug_assert!(
            self.method != CreateGf16Method::Avx2PshufbPrepared
                || self.coeffs.iter().all(|c| c.avx2.is_some())
        );

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
        staging
            .slot_mut(slot, self.aligned_chunk_len, self.chunk_len)
            .copy_from_slice(input_chunk);
        staging.source_indices[slot] = source_idx;
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
        staging
            .slot_mut(slot, self.aligned_chunk_len, self.chunk_len)
            .copy_from_slice(&self.transfer_buffers[idx][..self.chunk_len]);
        staging.source_indices[slot] = source_idx;
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
    pub fn finish_chunk(&mut self, recovery_blocks: &mut [(u16, Vec<u8>)], block_size: usize) {
        self.end_input();

        recovery_blocks
            .iter_mut()
            .enumerate()
            .for_each(|(recovery_idx, (_, recovery_data))| {
                debug_assert!(recovery_data.capacity() >= block_size);
                debug_assert!(recovery_data.len() + self.chunk_len <= recovery_data.capacity());
                let start = recovery_idx * self.aligned_chunk_len;
                let end = start + self.chunk_len;
                debug_assert!(end <= self.output_chunks.len());
                recovery_data.extend_from_slice(&self.output_chunks[start..end]);
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
        let segment_len = align_down(
            self.method
                .ideal_segment_len()
                .min(self.chunk_len)
                .max(AVX2_ALIGNMENT),
            AVX2_ALIGNMENT,
        )
        .max(AVX2_ALIGNMENT);
        let segment_count = self.chunk_len.div_ceil(segment_len);
        let output_groups = worker_count.min(recovery_count).max(1);
        let outputs_per_group = recovery_count.div_ceil(output_groups);
        let staging = &self.staging[staging_idx];
        debug_assert!(staging.batch_len <= self.input_grouping);
        debug_assert!(self.coeffs.len() == recovery_count * self.source_count);

        let mut job_count = 0;
        for segment_idx in 0..segment_count {
            let start = segment_idx * segment_len;
            let len = (self.chunk_len - start).min(segment_len);
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
                    source_count: self.source_count,
                    batch_len: staging.batch_len,
                    aligned_chunk_len: self.aligned_chunk_len,
                    segment_start: start,
                    segment_len: len,
                    output_start,
                    output_end,
                };
                job_count += 1;
            }
        }
        job_count
    }
}

#[derive(Clone, Copy, Default)]
struct ComputeJob {
    method: CreateGf16Method,
    input_base: usize,
    output_base: usize,
    coeffs: usize,
    source_indices: usize,
    source_count: usize,
    batch_len: usize,
    aligned_chunk_len: usize,
    segment_start: usize,
    segment_len: usize,
    output_start: usize,
    output_end: usize,
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

        process_compute_job(job);

        let mut state = shared.state.lock().unwrap();
        state.remaining_jobs -= 1;
        if state.remaining_jobs == 0 {
            seen_generation = state.generation;
            shared.done.notify_one();
        }
    }
}

fn process_compute_job(job: ComputeJob) {
    #[cfg(target_arch = "x86_64")]
    if let Some(flavor) = job.method.xor_jit_flavor() {
        process_compute_job_xor_jit(job, flavor);
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

        #[cfg(target_arch = "x86_64")]
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
fn process_compute_job_xor_jit(job: ComputeJob, flavor: XorJitFlavor) {
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
            _ if xor_jit_fallback_is_error() => {
                panic!("forced XOR-JIT create backend missing prepared coefficient")
            }
            _ => {
                process_slice_multiply_add(input_a, output_a, &coeff_a0.split);
                process_slice_multiply_add(input_b, output_a, &coeff_b0.split);
                process_slice_multiply_add(input_c, output_a, &coeff_c0.split);
                process_slice_multiply_add(input_d, output_a, &coeff_d0.split);
                process_slice_multiply_add(input_a, output_b, &coeff_a1.split);
                process_slice_multiply_add(input_b, output_b, &coeff_b1.split);
                process_slice_multiply_add(input_c, output_b, &coeff_c1.split);
                process_slice_multiply_add(input_d, output_b, &coeff_d1.split);
                process_slice_multiply_add(input_a, output_c, &coeff_a2.split);
                process_slice_multiply_add(input_b, output_c, &coeff_b2.split);
                process_slice_multiply_add(input_c, output_c, &coeff_c2.split);
                process_slice_multiply_add(input_d, output_c, &coeff_d2.split);
                process_slice_multiply_add(input_a, output_d, &coeff_a3.split);
                process_slice_multiply_add(input_b, output_d, &coeff_b3.split);
                process_slice_multiply_add(input_c, output_d, &coeff_c3.split);
                process_slice_multiply_add(input_d, output_d, &coeff_d3.split);
            }
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
            _ if xor_jit_fallback_is_error() => {
                panic!("forced XOR-JIT create backend missing prepared coefficient")
            }
            _ => {
                process_slice_multiply_add(input_a, output_a, &coeff_a0.split);
                process_slice_multiply_add(input_b, output_a, &coeff_b0.split);
                process_slice_multiply_add(input_c, output_a, &coeff_c0.split);
                process_slice_multiply_add(input_d, output_a, &coeff_d0.split);
                process_slice_multiply_add(input_a, output_b, &coeff_a1.split);
                process_slice_multiply_add(input_b, output_b, &coeff_b1.split);
                process_slice_multiply_add(input_c, output_b, &coeff_c1.split);
                process_slice_multiply_add(input_d, output_b, &coeff_d1.split);
            }
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
            _ if xor_jit_fallback_is_error() => {
                panic!("forced XOR-JIT create backend missing prepared coefficient")
            }
            _ => {
                process_slice_multiply_add(input_a, output_a, &coeff_a0.split);
                process_slice_multiply_add(input_b, output_a, &coeff_b0.split);
                process_slice_multiply_add(input_a, output_b, &coeff_a1.split);
                process_slice_multiply_add(input_b, output_b, &coeff_b1.split);
            }
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
            _ if xor_jit_fallback_is_error() => {
                panic!("forced XOR-JIT create backend missing prepared coefficient")
            }
            _ => {
                process_slice_multiply_add(input, output_a, &coeff_a.split);
                process_slice_multiply_add(input, output_b, &coeff_b.split);
            }
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
            _ if xor_jit_fallback_is_error() => {
                panic!("forced XOR-JIT create backend missing prepared coefficient")
            }
            _ => {
                process_slice_multiply_add(input_a, output, &coeff_a.split);
                process_slice_multiply_add(input_b, output, &coeff_b.split);
                process_slice_multiply_add(input_c, output, &coeff_c.split);
                process_slice_multiply_add(input_d, output, &coeff_d.split);
            }
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
            _ if xor_jit_fallback_is_error() => {
                panic!("forced XOR-JIT create backend missing prepared coefficient")
            }
            _ => {
                process_slice_multiply_add(input_a, output, &coeff_a.split);
                process_slice_multiply_add(input_b, output, &coeff_b.split);
            }
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
            None if xor_jit_fallback_is_error() => {
                panic!("forced XOR-JIT create backend missing prepared coefficient")
            }
            None => process_slice_multiply_add(input, output, &coeff.split),
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
    let output_groups = worker_count.min(recovery_count).max(1);
    (segment_count * output_groups).max(1)
}

#[inline]
fn align_up(value: usize, alignment: usize) -> usize {
    if value == 0 {
        0
    } else {
        (value + alignment - 1) & !(alignment - 1)
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
    fn forced_pshufb_backend_matches_encoder_for_full_batch() {
        assert_forced_backend_matches_encoder("pshufb", CreateGf16Method::Avx2PshufbPrepared);
    }

    #[test]
    fn forced_xor_jit_backend_matches_encoder_for_full_batch() {
        assert_forced_backend_matches_encoder("xor-jit-port", CreateGf16Method::Avx2XorJitPort);
        assert_forced_backend_matches_encoder("xor-jit-clean", CreateGf16Method::Avx2XorJitClean);
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
}
