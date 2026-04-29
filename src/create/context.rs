//! CreateContext - Main context for PAR2 creation
//!
//! Reference: par2cmdline-turbo/src/par2creator.h Par2Creator class

use super::error::{CreateError, CreateResult};
use super::error_helpers::{
    create_file, create_new_output_file, get_metadata, open_for_reading, packet_write_error,
};
use super::packet_generator::generate_recovery_set_id;
use super::profile::{CreateProfile, CreateProfileCounters, CreateProfilePhase};
use super::progress::CreateReporter;
use super::source_file::{normalize_packet_path, packet_name_from_path, SourceFileInfo};
use super::types::CreateConfig;
use crate::create::backend::CreateRecoveryBackend;
use crate::domain::{BlockSize, ChunkSize, RecoverySetId, SourceBlockCount};
use std::borrow::Cow;
use std::path::{Path, PathBuf};

const DEFAULT_MEMORY_LIMIT: usize = 1024 * 1024 * 1024; // 1 GiB
const MAX_CREATE_CHUNK_SIZE: usize = 32 * 1024 * 1024;
const RECOVERY_PACKET_TYPE: &[u8; 16] = b"PAR 2.0\0RecvSlic";

fn default_output_base_path(output_name: &str) -> PathBuf {
    Path::new(output_name)
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

/// Serialize and write a single PAR2 recovery slice packet
///
/// Computes the packet MD5, builds the packet, and writes it to `writer`.
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp WriteRecoveryPackets()
fn write_recovery_slice_packet<W: std::io::Write>(
    writer: &mut W,
    exponent: u32,
    recovery_data: &[u8],
    recovery_set_id: RecoverySetId,
) -> std::io::Result<()> {
    let header = build_recovery_slice_header(exponent, recovery_data, recovery_set_id);
    writer.write_all(&header)?;
    writer.write_all(recovery_data)
}

fn build_recovery_slice_header(
    exponent: u32,
    recovery_data: &[u8],
    recovery_set_id: RecoverySetId,
) -> [u8; 68] {
    use md5::{Digest, Md5};

    let packet_length = 8 + 8 + 16 + 16 + 16 + 4 + recovery_data.len() as u64;

    // Compute MD5 over: set_id || type || exponent || data
    let mut hasher = Md5::new();
    hasher.update(recovery_set_id.as_bytes());
    hasher.update(RECOVERY_PACKET_TYPE);
    hasher.update(exponent.to_le_bytes());
    hasher.update(recovery_data);
    let computed_md5 = hasher.finalize();

    let mut header = [0u8; 68];
    header[0..8].copy_from_slice(crate::packets::MAGIC_BYTES);
    header[8..16].copy_from_slice(&packet_length.to_le_bytes());
    header[16..32].copy_from_slice(&computed_md5);
    header[32..48].copy_from_slice(recovery_set_id.as_bytes());
    header[48..64].copy_from_slice(RECOVERY_PACKET_TYPE);
    header[64..68].copy_from_slice(&exponent.to_le_bytes());
    header
}

/// Compute the chunk size for chunked processing.
///
/// Returns the number of bytes to process per chunk. Equal to `block_size` when
/// the full-block memory fits within the limit; otherwise a fraction thereof,
/// aligned to 4 bytes.
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp CalculateProcessBlockSize()
fn calculate_chunk_size_impl(
    block_size: usize,
    source_block_count: usize,
    recovery_block_count: usize,
    memory_limit: usize,
) -> usize {
    let block_overhead = 2 + (source_block_count + 1).min(24);
    let full_block_memory = block_size * (recovery_block_count + block_overhead);
    if full_block_memory <= memory_limit {
        return block_size.min(MAX_CREATE_CHUNK_SIZE);
    }
    let chunk_size = memory_limit / (recovery_block_count + block_overhead);
    let aligned = chunk_size & !3;
    aligned.clamp(4, block_size.min(MAX_CREATE_CHUNK_SIZE))
}

#[derive(Debug, Clone, Copy)]
struct CreateProcessConfig {
    chunk_size: ChunkSize,
    defer_hash_computation: bool,
}

impl CreateProcessConfig {
    fn new(
        block_size: BlockSize,
        source_block_count: u32,
        recovery_block_count: u32,
        memory_limit: usize,
    ) -> Self {
        let chunk_size = ChunkSize::new(calculate_chunk_size_impl(
            block_size.as_usize(),
            source_block_count as usize,
            recovery_block_count as usize,
            memory_limit,
        ));
        let defer_hash_computation = chunk_size.as_usize() >= block_size.as_usize();
        Self {
            chunk_size,
            defer_hash_computation,
        }
    }
}

/// Per-file hash data computed during `encode_and_hash_files`.
struct FileHashState {
    hash_16k: crate::domain::Md5Hash,
    full_md5: crate::domain::Md5Hash,
    file_id: crate::domain::FileId,
    block_count: u32,
    global_block_offset: u32,
    block_checksums: Vec<super::source_file::BlockChecksum>,
}

/// Recovery blocks paired with their exponents, as returned by `encode_and_hash_files`.
type RecoveryBlockVec = Vec<(u16, Vec<u8>)>;

struct EncodeProfile {
    counters: CreateProfileCounters,
    source_open_hash_prepass: std::time::Duration,
    recovery_chunk_processing: std::time::Duration,
}

/// Encode all source files into recovery blocks while simultaneously computing
/// file/block hashes in a single pass.
///
/// Returns `(recovery_blocks, per_file_hash_states)`.
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp ProcessData()
#[allow(clippy::too_many_arguments)] // All params are logically distinct; a param struct would add noise
fn encode_and_hash_files(
    source_files: &[SourceFileInfo],
    block_size: u64,
    chunk_size: usize,
    source_block_count: u32,
    base_values: &[u16],
    first_recovery_block: u32,
    recovery_count: usize,
    thread_count: usize,
    reporter: &dyn CreateReporter,
    profile_enabled: bool,
) -> CreateResult<(RecoveryBlockVec, Vec<FileHashState>, Option<EncodeProfile>)> {
    use crate::checksum::compute_file_id;
    use crate::create::source_file::BlockChecksum;
    use crate::parpar_hasher::hasher_input_dyn::HasherInputDyn;
    use crate::parpar_hasher::BlockHash;
    use md5::{Digest, Md5};
    use std::fs::File;
    use std::io::{Read, Seek};

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(thread_count)
        .build()
        .map_err(|err| CreateError::Other(format!("failed to create thread pool: {err}")))?;

    // True when the recovery loop traverses each file in strict
    // sequential byte order (chunk_len == block_size for every block),
    // letting us run HasherInputDyn inline with recovery generation —
    // mirroring par2cmdline-turbo's `deferhashcomputation = true` path.
    // Otherwise (slim mode), the recovery loop is chunk-offset-major
    // across blocks, so per-file bytes are non-sequential there and
    // we must hash each file in a separate sequential pass — that's
    // turbo's `deferhashcomputation = false` path
    // (`Par2CreatorSourceFile::Open` non-defer branch).
    let defer_hash_to_recovery_loop = chunk_size >= block_size as usize;

    let source_open_hash_start = profile_enabled.then(std::time::Instant::now);
    let mut file_handles: Vec<File> = Vec::with_capacity(source_files.len());
    let mut file_16k_buffers: Vec<Vec<u8>> = Vec::with_capacity(source_files.len());

    // One HasherInputDyn per source file when we're going to drive it
    // from the recovery loop. In slim mode we still allocate the slot
    // but don't use it — kept symmetric for indexing.
    let mut file_hashers: Vec<Option<HasherInputDyn>> = Vec::with_capacity(source_files.len());

    // Per-block outputs, populated as `get_block` calls retire each block.
    let mut block_hashes: Vec<Option<BlockHash>> = vec![None; source_block_count as usize];

    // Per-file file-MD5, populated either by the slim-mode pre-pass
    // (turbo's non-defer `Open()` branch) or by `hasher.end()` after
    // the recovery loop (defer branch).
    let mut file_full_md5: Vec<Option<crate::domain::Md5Hash>> = vec![None; source_files.len()];

    // Per-file metadata: (block_count, global_block_offset)
    let mut file_block_meta: Vec<(u32, u32)> = Vec::with_capacity(source_files.len());
    let mut global_block_offset = 0u32;

    for file in source_files {
        if defer_hash_to_recovery_loop {
            file_handles.push(open_for_reading(&file.path)?);
        }
        file_16k_buffers.push(vec![0u8; (file.size as usize).min(16 * 1024)]);
        file_hashers.push(if defer_hash_to_recovery_loop {
            Some(HasherInputDyn::new())
        } else {
            None
        });

        let block_count = file.calculate_block_count(block_size);
        file_block_meta.push((block_count, global_block_offset));
        global_block_offset += block_count;
    }

    // Slim mode (turbo's `deferhashcomputation = false`): turbo computes
    // file MD5, hash16k, and per-block (MD5, CRC32) in
    // `Par2CreatorSourceFile::Open()` itself, *before* the recovery
    // loop runs — because in slim mode the recovery loop visits each
    // file in chunk-offset-major order across blocks, which is not
    // sequential per-file. We mirror that here: a parallel-across-files
    // pre-pass that streams each file end-to-end through HasherInputDyn,
    // then reopen the recovery-loop handles afterwards. That avoids
    // keeping two live FDs per source file at once on large input sets.
    if !defer_hash_to_recovery_loop {
        use rayon::prelude::*;
        use std::io::SeekFrom;
        // Each task gets its own File handle (we can't share &mut File
        // across rayon tasks). Recovery-loop handles are opened only
        // after this pre-pass completes.
        let per_file_results: CreateResult<Vec<_>> = pool.install(|| {
            source_files
                .par_iter()
                .enumerate()
                .map(|(file_idx, file)| -> CreateResult<_> {
                    let (block_count, g_offset) = file_block_meta[file_idx];
                    let mut handle = open_for_reading(&file.path)?;
                    handle
                        .seek(SeekFrom::Start(0))
                        .map_err(|e| CreateError::FileReadError {
                            file: file.path.to_string_lossy().to_string(),
                            source: e,
                        })?;

                    let mut hasher = HasherInputDyn::new();
                    let mut hash16k_buf = vec![0u8; (file.size as usize).min(16 * 1024)];
                    let mut block_hashes_local: Vec<BlockHash> =
                        Vec::with_capacity(block_count as usize);

                    // 1 MiB I/O buffer, capped at min(blocksize, filesize),
                    // matching turbo's non-defer `Open()`.
                    let buffersize = (1024 * 1024)
                        .min(block_size as usize)
                        .min(file.size as usize)
                        .max(1);
                    let mut buf = vec![0u8; buffersize];

                    let mut offset: u64 = 0;
                    let mut blocknumber: u32 = 0;
                    let mut need: u64 = block_size;

                    while offset < file.size {
                        let want = (file.size - offset).min(buffersize as u64) as usize;
                        handle.read_exact(&mut buf[..want]).map_err(|e| {
                            CreateError::FileReadError {
                                file: file.path.to_string_lossy().to_string(),
                                source: e,
                            }
                        })?;

                        // Capture first 16 KiB for hash16k.
                        if offset < 16 * 1024 {
                            let cap_start = offset as usize;
                            let cap_end = (cap_start + want).min(16 * 1024);
                            hash16k_buf[cap_start..cap_end]
                                .copy_from_slice(&buf[..(cap_end - cap_start)]);
                        }

                        let mut used = 0usize;
                        while used < want {
                            let use_n = need.min((want - used) as u64) as usize;
                            hasher.update(&buf[used..used + use_n]);
                            used += use_n;
                            need -= use_n as u64;

                            if need == 0 {
                                let bh = hasher.get_block(0);
                                block_hashes_local.push(bh);
                                blocknumber += 1;
                                if blocknumber < block_count {
                                    need = block_size;
                                }
                            }
                        }

                        offset += want as u64;
                    }

                    // Final short block: feed zero pad to BLOCK lane only.
                    if need > 0 && block_count > 0 {
                        let bh = hasher.get_block(need);
                        block_hashes_local.push(bh);
                    }

                    let file_md5 = hasher.end();

                    Ok((
                        file_idx,
                        g_offset,
                        hash16k_buf,
                        file_md5,
                        block_hashes_local,
                    ))
                })
                .collect()
        });

        for (file_idx, g_offset, hash16k_buf, file_md5, blocks) in per_file_results? {
            file_16k_buffers[file_idx] = hash16k_buf;
            file_full_md5[file_idx] = Some(crate::domain::Md5Hash::new(file_md5));
            for (i, bh) in blocks.into_iter().enumerate() {
                block_hashes[(g_offset + i as u32) as usize] = Some(bh);
            }
        }

        file_handles = source_files
            .iter()
            .map(|file| open_for_reading(&file.path))
            .collect::<CreateResult<Vec<_>>>()?;
    }
    let source_hash_bytes_read = if !profile_enabled || defer_hash_to_recovery_loop {
        0
    } else {
        source_files.iter().map(|file| file.size).sum()
    };
    let source_hash_seek_count = if !profile_enabled || defer_hash_to_recovery_loop {
        0
    } else {
        source_files.len() as u64
    };
    let source_open_hash_prepass = source_open_hash_start
        .map(|start| start.elapsed())
        .unwrap_or_default();

    let recovery_processing_start = profile_enabled.then(std::time::Instant::now);
    let (recovery_blocks, recovery_counters) = pool.install(|| {
        let mut backend = CreateRecoveryBackend::new(
            base_values,
            first_recovery_block,
            recovery_count,
            chunk_size,
            thread_count,
        );
        let selected_backend = profile_enabled.then(|| format!("{:?}", backend.selected_method()));
        let mut recovery_blocks = backend.recovery_blocks(block_size as usize);
        let mut source_recovery_bytes_read = 0u64;
        let mut source_seek_count = 0u64;
        let mut recovery_chunk_count = 0u64;
        let mut file_positions = vec![0u64; source_files.len()];

        // Main chunk loop
        let mut block_offset = 0u64;
        while block_offset < block_size {
            let chunk_len = ((block_size - block_offset) as usize).min(chunk_size);
            if profile_enabled {
                recovery_chunk_count += 1;
            }
            backend.begin_chunk(chunk_len);

            let mut file_block_idx = 0usize;

            for (file_idx, file) in source_files.iter().enumerate() {
                let (block_count, g_offset) = file_block_meta[file_idx];
                for block_idx in 0..block_count {
                    let is_last = block_idx == block_count - 1;
                    let block_actual = if is_last && file.size % block_size != 0 {
                        (file.size % block_size) as usize
                    } else {
                        block_size as usize
                    };
                    let bytes_available = if block_offset >= block_actual as u64 {
                        0
                    } else {
                        block_actual - block_offset as usize
                    };
                    let bytes_to_read = bytes_available.min(chunk_len);
                    {
                        let chunk = backend.prepare_transfer_buffer(file_block_idx);
                        if bytes_to_read > 0 {
                            let file_pos = block_idx as u64 * block_size + block_offset;
                            if file_positions[file_idx] != file_pos {
                                file_handles[file_idx]
                                    .seek(std::io::SeekFrom::Start(file_pos))
                                    .map_err(|e| CreateError::FileReadError {
                                        file: file.path.to_string_lossy().to_string(),
                                        source: e,
                                    })?;
                                file_positions[file_idx] = file_pos;
                                if profile_enabled {
                                    source_seek_count += 1;
                                }
                            }
                            file_handles[file_idx]
                                .read_exact(&mut chunk[..bytes_to_read])
                                .map_err(|e| CreateError::FileReadError {
                                    file: file.path.to_string_lossy().to_string(),
                                    source: e,
                                })?;
                            if profile_enabled {
                                source_recovery_bytes_read += bytes_to_read as u64;
                            }
                            file_positions[file_idx] += bytes_to_read as u64;
                            if file_pos < 16 * 1024 {
                                let capture_start = file_pos as usize;
                                let capture_end = (capture_start + bytes_to_read).min(16 * 1024);
                                let capture_len = capture_end - capture_start;
                                file_16k_buffers[file_idx][capture_start..capture_end]
                                    .copy_from_slice(&chunk[..capture_len]);
                            }
                        }

                        // Defer mode: hash inline with recovery, mirroring
                        // par2cmdline-turbo's `Par2CreatorSourceFile::UpdateHashes`
                        // path. Each block is read in full this iteration
                        // (chunk_len == block_size), and we visit all blocks
                        // of file N before any block of file N+1, so per-file
                        // bytes arrive in strict sequential order — exactly
                        // what HasherInputDyn requires.
                        //
                        // Slim mode skips this; a separate per-file pass
                        // below drives HasherInputDyn for those files.
                        if defer_hash_to_recovery_loop {
                            let hasher = file_hashers[file_idx]
                                .as_mut()
                                .expect("hasher allocated in defer mode");
                            if bytes_to_read > 0 {
                                hasher.update(&chunk[..bytes_to_read]);
                            }
                            // PAR2 spec: BLOCK hashes include trailing zero
                            // pad up to block_size; FILE hash does not. The
                            // hasher's `get_block(zero_pad)` enforces both
                            // halves of that.
                            let zero_pad = (chunk_len - bytes_to_read) as u64;
                            let bh = hasher.get_block(zero_pad);
                            block_hashes[(g_offset + block_idx) as usize] = Some(bh);
                        }
                    }
                    backend.add_transfer_input(file_block_idx, file_block_idx);
                    file_block_idx += 1;
                }
            }

            if !backend.finish_chunk(&mut recovery_blocks, block_size as usize) {
                return Err(CreateError::XorJitChecksumValidationFailed);
            }

            block_offset += chunk_len as u64;
            let progress =
                ((block_offset as f64 / block_size as f64) * recovery_count as f64) as u32;
            reporter.report_recovery_generation(
                progress.min(recovery_count as u32),
                recovery_count as u32,
            );
        }

        let counters = CreateProfileCounters {
            source_recovery_bytes_read,
            source_seek_count,
            recovery_chunk_count,
            selected_backend,
            ..CreateProfileCounters::default()
        };
        Ok::<_, CreateError>((recovery_blocks, counters))
    })?;
    let recovery_chunk_processing = recovery_processing_start
        .map(|start| start.elapsed())
        .unwrap_or_default();
    let encode_profile = profile_enabled.then(|| {
        let mut counters = recovery_counters;
        counters.source_hash_bytes_read = source_hash_bytes_read;
        counters.source_seek_count += source_hash_seek_count;
        EncodeProfile {
            counters,
            source_open_hash_prepass,
            recovery_chunk_processing,
        }
    });

    // Finalize file MD5s. In defer mode each per-file hasher has been
    // streamed in block order through the recovery loop and we now
    // call `end()` to retrieve the file MD5 — turbo's
    // `Par2Creator::FinishFileHashComputation` path. In slim mode
    // `file_full_md5[i]` was already populated by the pre-pass above.
    if defer_hash_to_recovery_loop {
        for (i, slot) in file_hashers.into_iter().enumerate() {
            let h = slot.expect("hasher allocated in defer mode");
            file_full_md5[i] = Some(crate::domain::Md5Hash::new(h.end()));
        }
    }

    let full_file_hashes: Vec<crate::domain::Md5Hash> = file_full_md5
        .into_iter()
        .map(|o| o.expect("file MD5 populated by either defer or slim path"))
        .collect();

    let file_16k_hashes = file_16k_buffers
        .iter()
        .map(|bytes| crate::domain::Md5Hash::new(Md5::digest(bytes).into()))
        .collect::<Vec<_>>();

    let mut hash_states: Vec<FileHashState> = Vec::with_capacity(source_files.len());
    let mut global_block_idx = 0usize;

    for (file_idx, file) in source_files.iter().enumerate() {
        let (block_count, g_offset) = file_block_meta[file_idx];
        let full_md5 = full_file_hashes[file_idx];
        let hash_16k = file_16k_hashes[file_idx];
        let filename = file.packet_name().as_bytes();
        let file_id = compute_file_id(&hash_16k, file.size, filename);

        let mut checksums = Vec::with_capacity(block_count as usize);
        for block_idx in 0..block_count {
            let bh =
                block_hashes[global_block_idx].expect("block hash populated by defer or slim path");
            log::debug!(
                "Block {}: MD5={:02x}{:02x}..., CRC32={:08x}",
                global_block_idx,
                bh.md5[0],
                bh.md5[1],
                bh.crc32
            );

            checksums.push(BlockChecksum {
                crc32: bh.crc32,
                hash: crate::domain::Md5Hash::new(bh.md5),
                global_index: g_offset + block_idx,
            });
            global_block_idx += 1;
        }

        reporter.report_file_hashing(file.packet_name(), file.size, file.size);

        hash_states.push(FileHashState {
            hash_16k,
            full_md5,
            file_id,
            block_count,
            global_block_offset: g_offset,
            block_checksums: checksums,
        });
    }

    Ok((recovery_blocks, hash_states, encode_profile))
}

/// Populate `source_files` from the hash data computed during `encode_and_hash_files`.
fn finalize_file_hashes(
    hash_states: Vec<FileHashState>,
    source_files: &mut [SourceFileInfo],
) -> CreateResult<()> {
    for (file, state) in source_files.iter_mut().zip(hash_states) {
        file.hash_16k = state.hash_16k;
        file.hash = state.full_md5;
        file.file_id = state.file_id;
        file.block_count = state.block_count;
        file.global_block_offset = state.global_block_offset;
        file.block_checksums = state.block_checksums;
    }
    Ok(())
}

/// Main context for PAR2 creation
///
/// This structure manages the entire PAR2 creation process:
/// 1. Scanning and validating source files
/// 2. Computing optimal block size
/// 3. Hashing files and blocks
/// 4. Generating Reed-Solomon recovery blocks
/// 5. Writing PAR2 files
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp Par2Creator::Process()
pub struct CreateContext {
    /// Configuration
    config: CreateConfig,

    /// Progress reporter
    reporter: Box<dyn CreateReporter>,

    /// Recovery set ID (generated from source files)
    recovery_set_id: Option<RecoverySetId>,

    /// Source file information
    source_files: Vec<SourceFileInfo>,

    /// Calculated block size (bytes)
    block_size: BlockSize,

    /// Total number of source blocks across all files
    source_block_count: u32,

    /// Number of recovery blocks to generate
    recovery_block_count: u32,

    /// Generated recovery blocks (exponent, data)
    recovery_blocks: Vec<(u16, Vec<u8>)>,

    /// Output PAR2 files created
    output_files: Vec<String>,

    /// Opt-in create phase/counter profiler.
    profile: Option<CreateProfile>,
}

impl CreateContext {
    /// Create a new CreateContext
    ///
    /// Called by CreateContextBuilder after validation
    /// Performs initial setup: scans files, calculates block size and recovery count
    pub(super) fn new(
        config: CreateConfig,
        reporter: Box<dyn CreateReporter>,
    ) -> CreateResult<Self> {
        let profile = CreateProfile::from_env();
        let mut context = CreateContext {
            config,
            reporter,
            recovery_set_id: None,
            source_files: Vec::new(),
            block_size: BlockSize::new(0),
            source_block_count: 0,
            recovery_block_count: 0,
            recovery_blocks: Vec::new(),
            output_files: Vec::new(),
            profile,
        };

        // Perform initial setup
        let scan_start = std::time::Instant::now();
        context.scan_source_files()?;
        if let Some(profile) = &mut context.profile {
            profile.add_duration(CreateProfilePhase::SourceScanMetadata, scan_start.elapsed());
            let counters = profile.counters_mut();
            counters.source_file_count = context.source_files.len();
            counters.source_bytes = context.source_files.iter().map(|file| file.size).sum();
        }
        context.calculate_block_size()?;
        context.calculate_recovery_blocks()?;
        context.record_static_profile_counters();

        Ok(context)
    }

    /// Execute the PAR2 creation process
    ///
    /// This is the main entry point that orchestrates all creation steps
    /// Note: Initial setup (file scanning, block size calculation) is done during build()
    ///
    /// Reference: par2cmdline-turbo/src/par2creator.cpp Par2Creator::Process()
    pub fn create(&mut self) -> CreateResult<()> {
        // Step 1: Generate recovery blocks AND compute file hashes in single pass
        // This is the performance-critical optimization that eliminates dual file reads
        // Hashes and block checksums are computed during recovery generation
        self.generate_recovery_blocks()?;

        // Step 2: Generate recovery set ID (needs file IDs from hashes computed in step 1)
        self.generate_recovery_set_id()?;

        // Step 3: Write PAR2 files
        self.write_par2_files()?;

        // Report completion
        self.reporter.report_complete(&self.output_files);
        if let Some(profile) = &self.profile {
            profile.emit();
        }

        Ok(())
    }

    /// Scan source files and validate accessibility
    ///
    /// Reference: par2cmdline-turbo/src/par2creator.cpp OpenSourceFiles()
    fn scan_source_files(&mut self) -> CreateResult<()> {
        let total_files = self.config.source_files.len();

        for (index, path) in self.config.source_files.iter().enumerate() {
            self.reporter.report_scanning_files(
                index + 1,
                total_files,
                path.to_str().unwrap_or(""),
            );

            // Check file exists
            if !path.exists() {
                return Err(CreateError::FileNotFound(
                    path.to_string_lossy().to_string(),
                ));
            }

            // Get file metadata
            let metadata = get_metadata(path)?;

            let size = metadata.len();
            if size == 0 {
                continue;
            }

            let packet_name = self.packet_name_for_path(path)?;
            let source_info =
                SourceFileInfo::new_with_packet_name(path.clone(), packet_name, size, index);

            self.source_files.push(source_info);
        }

        // Validate we have at least some data
        let total_size: u64 = self.source_files.iter().map(|f| f.size).sum();
        if total_size == 0 {
            return Err(CreateError::EmptySourceFiles);
        }

        Ok(())
    }

    fn packet_base_path(&self) -> Cow<'_, Path> {
        match &self.config.base_path {
            Some(base_path) => Cow::Borrowed(base_path.as_path()),
            None => Cow::Owned(default_output_base_path(&self.config.output_name)),
        }
    }

    fn packet_name_for_path(&self, path: &Path) -> CreateResult<String> {
        let base_path = self.packet_base_path();
        let base_path = base_path.as_ref();

        if !base_path.as_os_str().is_empty() {
            if let Ok(relative) = path.strip_prefix(base_path) {
                return Ok(normalize_packet_path(relative));
            }

            if let (Ok(canonical_base), Ok(canonical_path)) = (
                std::fs::canonicalize(base_path),
                std::fs::canonicalize(path),
            ) {
                if let Ok(relative) = canonical_path.strip_prefix(&canonical_base) {
                    return Ok(normalize_packet_path(relative));
                }
            }
        }

        Ok(packet_name_from_path(path))
    }

    /// Calculate optimal block size based on source_block_count or total file size
    ///
    /// Reference: par2cmdline-turbo/src/commandline.cpp:1120-1245 ComputeBlockSize()
    fn calculate_block_size(&mut self) -> CreateResult<()> {
        // Reference: par2cmdline-turbo/src/commandline.cpp:1120-1123
        // If neither block_size nor source_block_count specified, default to 2000 blocks
        let target_block_count =
            if self.config.block_size.is_none() && self.config.source_block_count.is_none() {
                SourceBlockCount::new(2000)
            } else if let Some(count) = self.config.source_block_count {
                count
            } else {
                // block_size is specified, we'll use it directly below
                SourceBlockCount::new(0) // Won't be used
            };

        if let Some(block_size) = self.config.block_size {
            // User specified block size explicitly (-s option)
            // Reference: par2cmdline-turbo/src/par2creator.cpp:108
            self.block_size = BlockSize::new(block_size);
        } else {
            // Calculate block_size from target_block_count
            // Reference: par2cmdline-turbo/src/commandline.cpp:1147-1239
            let block_count = target_block_count.as_u64();
            let file_count = self.source_files.len() as u64;

            if block_count < file_count {
                return Err(CreateError::Other(format!(
                    "Block count ({}) cannot be smaller than the number of files ({})",
                    block_count, file_count
                )));
            }

            if block_count == file_count {
                // If block count equals file count, use size of largest file
                // Reference: par2cmdline-turbo/src/commandline.cpp:1158-1173
                let largest_filesize = self.source_files.iter().map(|f| f.size).max().unwrap_or(0);
                let block_size = (largest_filesize + 3) & !3; // Round up to multiple of 4
                self.block_size = BlockSize::new(block_size);
            } else {
                // Use binary search to find block size that results in target block count
                // Reference: par2cmdline-turbo/src/commandline.cpp:1175-1237

                // Calculate total size in 4-byte units (par2 uses 4-byte alignment)
                let total_size: u64 = self.source_files.iter().map(|f| f.size.div_ceil(4)).sum();

                if block_count > total_size {
                    // Too many blocks requested, use minimum size
                    self.block_size = BlockSize::new(4);
                } else {
                    // Binary search for block size
                    // Lower/upper bounds are in 4-byte units
                    let mut lower_bound = total_size / block_count;
                    let mut upper_bound =
                        (total_size + block_count - file_count - 1) / (block_count - file_count);

                    let mut size = 0u64;
                    let mut count = 0u64;

                    while lower_bound < upper_bound {
                        size = (lower_bound + upper_bound) / 2;

                        // Calculate how many blocks result from this size
                        count = 0;
                        for file in &self.source_files {
                            count += file.size.div_ceil(4).div_ceil(size);
                        }

                        if count > block_count {
                            lower_bound = size + 1;
                            if lower_bound >= upper_bound {
                                size = lower_bound;
                                // Recalculate count with final size
                                count = 0;
                                for file in &self.source_files {
                                    count += file.size.div_ceil(4).div_ceil(size);
                                }
                            }
                        } else {
                            upper_bound = size;
                        }
                    }

                    if count > 32768 {
                        return Err(CreateError::Other(format!(
                            "Error calculating block size. Block count cannot be higher than 32768 (got {})",
                            count
                        )));
                    } else if count == 0 {
                        return Err(CreateError::Other(
                            "Error calculating block size. Block count cannot be 0".to_string(),
                        ));
                    }

                    // Convert from 4-byte units to bytes
                    self.block_size = BlockSize::new(size * 4);
                }
            }
        }

        // Calculate total source block count with the determined block_size
        self.source_block_count = self
            .source_files
            .iter()
            .map(|f| f.calculate_block_count(self.block_size.as_u64()))
            .sum();

        Ok(())
    }

    /// Calculate number of recovery blocks to generate
    fn calculate_recovery_blocks(&mut self) -> CreateResult<()> {
        let recovery_blocks = if let Some(count) = self.config.recovery_block_count {
            // Explicit count specified
            count as u64
        } else if let Some(target_size) = self.config.recovery_target_size {
            self.calculate_recovery_blocks_for_target_size(target_size)?
        } else if let Some(percent) = self.config.redundancy_percentage {
            // Reference: par2cmdline-turbo/src/commandline.cpp ComputeRecoveryBlockCount()
            let count = (self.source_block_count as u64 * percent as u64 + 50) / 100;
            count.max(1)
        } else {
            return Err(CreateError::Other(
                "Must specify recovery block count, redundancy percentage, or target recovery size"
                    .to_string(),
            ));
        };

        self.recovery_block_count = self.checked_recovery_block_count(recovery_blocks)?;
        Ok(())
    }

    fn process_config(&self) -> CreateProcessConfig {
        CreateProcessConfig::new(
            self.block_size,
            self.source_block_count,
            self.recovery_block_count,
            self.config.memory_limit.unwrap_or(DEFAULT_MEMORY_LIMIT),
        )
    }

    fn record_static_profile_counters(&mut self) {
        let process_config = self.process_config();
        if let Some(profile) = &mut self.profile {
            let counters = profile.counters_mut();
            counters.block_size = self.block_size.as_u64();
            counters.source_block_count = self.source_block_count;
            counters.recovery_block_count = self.recovery_block_count;
            counters.chunk_size = process_config.chunk_size.as_usize();
            counters.defer_hash_computation = process_config.defer_hash_computation;
        }
    }

    fn checked_recovery_block_count(&self, recovery_blocks: u64) -> CreateResult<u32> {
        if recovery_blocks > 65536 {
            return Err(CreateError::Other(
                "Too many recovery blocks requested".to_string(),
            ));
        }

        if self.config.first_recovery_block as u64 + recovery_blocks >= 65536 {
            return Err(CreateError::Other(
                "First recovery block number is too high".to_string(),
            ));
        }

        Ok(recovery_blocks as u32)
    }

    fn calculate_recovery_blocks_for_target_size(&self, target_size: u64) -> CreateResult<u64> {
        use super::file_naming::default_recovery_file_count_for_scheme;

        let overhead_per_recovery_file = self.source_block_count as u64 * 21;
        let recovery_packet_size = self.block_size.as_u64() + 70;
        let largest_file_size = self.source_files.iter().map(|f| f.size).max().unwrap_or(0);

        let recovery_file_count = if let Some(count) = self.config.recovery_file_count {
            count
        } else {
            let estimated_file_count = 15u64;
            let overhead = estimated_file_count * overhead_per_recovery_file;
            let estimated_recovery_blocks = if overhead > target_size {
                1
            } else {
                ((target_size - overhead) / recovery_packet_size)
                    .max(1)
                    .min(u32::MAX as u64) as u32
            };
            default_recovery_file_count_for_scheme(
                self.config.recovery_file_scheme,
                estimated_recovery_blocks,
                largest_file_size,
                self.block_size.as_u64(),
            )
        };

        let overhead = recovery_file_count as u64 * overhead_per_recovery_file;
        let recovery_blocks = if overhead > target_size {
            1
        } else {
            ((target_size - overhead) / recovery_packet_size)
                .max(1)
                .min(u32::MAX as u64)
        };

        Ok(recovery_blocks)
    }

    /// Compute MD5 hashes and checksums for all source files
    ///
    /// Reference: par2cmdline-turbo/src/par2creator.cpp OpenSourceFiles() and
    /// FinishFileHashComputation()
    /// Generate recovery set ID
    fn generate_recovery_set_id(&mut self) -> CreateResult<()> {
        // Generate recovery set ID
        let set_id = generate_recovery_set_id(self.block_size.as_u64(), &self.source_files)?;
        self.recovery_set_id = Some(set_id);
        Ok(())
    }

    /// Generate Reed-Solomon recovery blocks AND compute file hashes in a single pass.
    ///
    /// Delegates to module-level helpers for each sub-step.
    ///
    /// Reference: par2cmdline-turbo/src/par2creator.cpp ProcessData()
    fn generate_recovery_blocks(&mut self) -> CreateResult<()> {
        use crate::reed_solomon::RecoveryBlockEncoder;

        if self.recovery_block_count == 0 {
            self.reporter.report_scanning_files(
                0,
                0,
                "No recovery blocks to generate (redundancy = 0%)",
            );
            return Ok(());
        }

        let encoder =
            RecoveryBlockEncoder::new(self.block_size.as_usize(), self.source_block_count as usize);
        let process_config = self.process_config();
        let chunk_size = process_config.chunk_size;

        self.reporter.report_scanning_files(
            0,
            self.recovery_block_count as usize,
            &format!(
                "Processing files (chunk size: {} bytes)...",
                chunk_size.as_usize()
            ),
        );

        let (recovery_blocks, hash_states, encode_profile) = encode_and_hash_files(
            &self.source_files,
            self.block_size.as_u64(),
            chunk_size.as_usize(),
            self.source_block_count,
            encoder.base_values(),
            self.config.first_recovery_block,
            self.recovery_block_count as usize,
            self.config.effective_threads(),
            self.reporter.as_ref(),
            self.profile.is_some(),
        )?;
        if let (Some(profile), Some(encode_profile)) = (&mut self.profile, encode_profile) {
            profile.add_duration(
                CreateProfilePhase::SourceOpenHashPrepass,
                encode_profile.source_open_hash_prepass,
            );
            profile.add_duration(
                CreateProfilePhase::RecoveryChunkProcessing,
                encode_profile.recovery_chunk_processing,
            );
            let counters = profile.counters_mut();
            counters.source_hash_bytes_read = encode_profile.counters.source_hash_bytes_read;
            counters.source_recovery_bytes_read =
                encode_profile.counters.source_recovery_bytes_read;
            counters.source_seek_count = encode_profile.counters.source_seek_count;
            counters.recovery_chunk_count = encode_profile.counters.recovery_chunk_count;
            counters.selected_backend = encode_profile.counters.selected_backend;
        }

        finalize_file_hashes(hash_states, &mut self.source_files)?;

        self.reporter.report_scanning_files(
            self.recovery_block_count as usize,
            self.recovery_block_count as usize,
            "Processing complete (hashes + recovery blocks)",
        );

        self.recovery_blocks = recovery_blocks;
        Ok(())
    }

    /// Write PAR2 files: index file (critical packets only) + volume files (critical + recovery)
    ///
    /// Reference: par2cmdline-turbo/src/par2creator.cpp WriteCriticalPackets() and
    /// WriteRecoveryPacketHeaders() / InitialiseOutputFiles()
    fn write_par2_files(&mut self) -> CreateResult<()> {
        use super::file_naming::plan_recovery_files;
        use super::packet_generator::{
            generate_creator_packet, generate_file_description_packet,
            generate_file_verification_packet, generate_main_packet, write_creator_packet,
            write_file_description_packet, write_file_verification_packet, write_main_packet,
        };
        use std::io::Write;

        let recovery_set_id = self
            .recovery_set_id
            .ok_or_else(|| CreateError::Other("Recovery set ID not generated".to_string()))?;

        // Generate all critical packets
        // Reference: par2cmdline-turbo/src/par2creator.cpp CreateMainPacket(), CreateCreatorPacket()
        let packet_serialization_start = std::time::Instant::now();
        let main_packet = generate_main_packet(
            recovery_set_id,
            self.block_size.as_u64(),
            &self.source_files,
        )?;
        let creator_packet = generate_creator_packet(recovery_set_id)?;

        let file_desc_packets: Vec<_> = self
            .source_files
            .iter()
            .map(|f| generate_file_description_packet(recovery_set_id, f))
            .collect::<CreateResult<_>>()?;

        let file_verif_packets: Vec<_> = self
            .source_files
            .iter()
            .map(|f| generate_file_verification_packet(recovery_set_id, f))
            .collect::<CreateResult<_>>()?;

        // Serialize critical packets to a byte buffer once, reuse for every output file
        // Reference: par2cmdline-turbo/src/par2creator.cpp WriteCriticalPackets()
        let mut critical_bytes: Vec<u8> = Vec::new();
        write_main_packet(&mut critical_bytes, &main_packet)
            .map_err(|e| packet_write_error("main packet", e))?;
        write_creator_packet(&mut critical_bytes, &creator_packet)
            .map_err(|e| packet_write_error("creator packet", e))?;
        for packet in &file_desc_packets {
            write_file_description_packet(&mut critical_bytes, packet)
                .map_err(|e| packet_write_error("file description packet", e))?;
        }
        for packet in &file_verif_packets {
            write_file_verification_packet(&mut critical_bytes, packet)
                .map_err(|e| packet_write_error("file verification packet", e))?;
        }
        if let Some(profile) = &mut self.profile {
            profile.add_duration(
                CreateProfilePhase::CriticalPacketSerialization,
                packet_serialization_start.elapsed(),
            );
        }

        // Determine output directory and base name
        let output_path = Path::new(&self.config.output_name);
        let output_dir = output_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let base_name = output_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output")
            .to_string();

        let index_path = output_dir.join(format!("{}.par2", base_name));
        let largest_file_size = self.source_files.iter().map(|f| f.size).max().unwrap_or(0);
        let file_count = {
            use super::file_naming::default_recovery_file_count_for_scheme;
            let default_count = default_recovery_file_count_for_scheme(
                self.config.recovery_file_scheme,
                self.recovery_block_count,
                largest_file_size,
                self.block_size.as_u64(),
            );
            self.config.recovery_file_count.unwrap_or(default_count)
        };
        let plan = plan_recovery_files(
            &base_name,
            file_count,
            self.recovery_block_count,
            self.config.first_recovery_block,
            self.config.recovery_file_scheme,
            largest_file_size,
            self.block_size.as_u64(),
        );

        let volume_paths: Vec<PathBuf> = plan
            .iter()
            .map(|entry| output_dir.join(&entry.filename))
            .collect();
        let output_paths: Vec<&Path> = std::iter::once(index_path.as_path())
            .chain(volume_paths.iter().map(PathBuf::as_path))
            .collect();

        if !self.config.overwrite_existing {
            for path in &output_paths {
                if path.exists() {
                    return Err(CreateError::FileCreateError {
                        file: path.to_string_lossy().to_string(),
                        source: std::io::Error::new(
                            std::io::ErrorKind::AlreadyExists,
                            "output file already exists",
                        ),
                    });
                }
            }
        }

        let open_output = |path: &Path, overwrite_existing: bool| {
            if overwrite_existing {
                create_file(path)
            } else {
                create_new_output_file(path)
            }
        };

        // Write index file: critical packets only, no recovery data
        // Reference: par2cmdline-turbo creates base.par2 with no recovery slices
        let output_write_start = std::time::Instant::now();
        let mut index_file = open_output(&index_path, self.config.overwrite_existing)?;
        index_file
            .write_all(&critical_bytes)
            .map_err(|e| CreateError::FileCreateError {
                file: index_path.to_string_lossy().to_string(),
                source: e,
            })?;
        index_file
            .flush()
            .map_err(|e| CreateError::FileCreateError {
                file: index_path.to_string_lossy().to_string(),
                source: e,
            })?;
        self.output_files
            .push(index_path.to_string_lossy().to_string());
        if let Some(profile) = &mut self.profile {
            profile.add_duration(
                CreateProfilePhase::OutputFileWrites,
                output_write_start.elapsed(),
            );
        }

        // Write each volume file: critical packets + its slice of recovery blocks
        // Reference: par2cmdline-turbo/src/par2creator.cpp WriteRecoveryPackets()
        for (entry, vol_path) in plan.iter().zip(volume_paths) {
            let output_write_start = std::time::Instant::now();
            let mut vol_file = open_output(&vol_path, self.config.overwrite_existing)?;

            vol_file
                .write_all(&critical_bytes)
                .map_err(|e| CreateError::FileCreateError {
                    file: vol_path.to_string_lossy().to_string(),
                    source: e,
                })?;

            if let Some(profile) = &mut self.profile {
                profile.add_duration(
                    CreateProfilePhase::OutputFileWrites,
                    output_write_start.elapsed(),
                );
            }

            // Write recovery slice packets for this volume
            for i in 0..entry.block_count {
                let packet_exponent = entry.first_exponent + i;
                let local_idx = (packet_exponent - self.config.first_recovery_block) as usize;
                let (recovery_exponent, recovery_data) = &self.recovery_blocks[local_idx];

                if self.profile.is_some() {
                    let packet_serialization_start = std::time::Instant::now();
                    let packet_header = build_recovery_slice_header(
                        *recovery_exponent as u32,
                        recovery_data,
                        recovery_set_id,
                    );
                    if let Some(profile) = &mut self.profile {
                        profile.add_duration(
                            CreateProfilePhase::RecoveryPacketSerialization,
                            packet_serialization_start.elapsed(),
                        );
                    }

                    let output_write_start = std::time::Instant::now();
                    vol_file
                        .write_all(&packet_header)
                        .and_then(|_| vol_file.write_all(recovery_data))
                        .map_err(|e| packet_write_error("recovery packet", e))?;
                    if let Some(profile) = &mut self.profile {
                        profile.add_duration(
                            CreateProfilePhase::OutputFileWrites,
                            output_write_start.elapsed(),
                        );
                    }
                } else {
                    write_recovery_slice_packet(
                        &mut vol_file,
                        *recovery_exponent as u32,
                        recovery_data,
                        recovery_set_id,
                    )
                    .map_err(|e| packet_write_error("recovery packet", e))?;
                }
            }

            let output_write_start = std::time::Instant::now();
            vol_file.flush().map_err(|e| CreateError::FileCreateError {
                file: vol_path.to_string_lossy().to_string(),
                source: e,
            })?;
            if let Some(profile) = &mut self.profile {
                profile.add_duration(
                    CreateProfilePhase::OutputFileWrites,
                    output_write_start.elapsed(),
                );
            }
            self.output_files
                .push(vol_path.to_string_lossy().to_string());
        }

        Ok(())
    }

    /// Get the list of created output files
    pub fn output_files(&self) -> &[String] {
        &self.output_files
    }

    /// Get block size
    pub fn block_size(&self) -> u64 {
        self.block_size.as_u64()
    }

    /// Get recovery block count
    pub fn recovery_block_count(&self) -> u32 {
        self.recovery_block_count
    }

    /// Get source block count
    pub fn source_block_count(&self) -> u32 {
        self.source_block_count
    }
}

#[cfg(test)]
mod tests {
    use super::super::progress::SilentCreateReporter;
    use super::super::types::CreateConfig;
    use super::*;
    use proptest::prelude::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn test_context(output_name: &str, base_path: Option<std::path::PathBuf>) -> CreateContext {
        CreateContext {
            config: CreateConfig {
                output_name: output_name.to_string(),
                base_path,
                ..CreateConfig::default()
            },
            reporter: Box::new(SilentCreateReporter),
            recovery_set_id: None,
            source_files: Vec::new(),
            block_size: BlockSize::new(0),
            source_block_count: 0,
            recovery_block_count: 0,
            recovery_blocks: Vec::new(),
            output_files: Vec::new(),
            profile: None,
        }
    }

    fn source_info(path: impl Into<std::path::PathBuf>, size: u64, index: usize) -> SourceFileInfo {
        SourceFileInfo::new(path.into(), size, index)
    }

    #[test]
    fn default_output_base_path_uses_parent_or_current_directory() {
        assert!(default_output_base_path("out.par2").as_os_str().is_empty());
        assert_eq!(
            default_output_base_path("nested/out.par2"),
            std::path::PathBuf::from("nested")
        );
    }

    #[test]
    fn recovery_slice_header_contains_expected_md5_and_layout() {
        let set_id = RecoverySetId::new([0xCD; 16]);
        let recovery_data = [1u8, 2, 3, 4, 5, 6];
        let header = build_recovery_slice_header(9, &recovery_data, set_id);

        assert_eq!(&header[0..8], crate::packets::MAGIC_BYTES);
        assert_eq!(
            u64::from_le_bytes(header[8..16].try_into().unwrap()),
            8 + 8 + 16 + 16 + 16 + 4 + recovery_data.len() as u64
        );
        assert_eq!(&header[32..48], set_id.as_bytes());
        assert_eq!(&header[48..64], RECOVERY_PACKET_TYPE);
        assert_eq!(u32::from_le_bytes(header[64..68].try_into().unwrap()), 9);

        use md5::{Digest, Md5};
        let mut hasher = Md5::new();
        hasher.update(set_id.as_bytes());
        hasher.update(RECOVERY_PACKET_TYPE);
        hasher.update(9u32.to_le_bytes());
        hasher.update(recovery_data);
        let expected_md5 = hasher.finalize();
        assert_eq!(&header[16..32], &expected_md5[..]);
    }

    // --- calculate_chunk_size_impl ---

    #[test]
    fn chunk_size_returns_full_block_when_fits_in_memory() {
        // Small scenario: easily fits in 1GB
        let result = calculate_chunk_size_impl(4096, 10, 5, DEFAULT_MEMORY_LIMIT);
        assert_eq!(result, 4096);
    }

    #[test]
    fn chunk_size_reduces_when_exceeds_memory_limit() {
        // block_size=512MB, 4 source blocks, 4 recovery blocks
        // full_block_memory = 512MB * 8 = 4GB > 1GB
        let block_size = 512 * 1024 * 1024;
        let result = calculate_chunk_size_impl(block_size, 4, 4, DEFAULT_MEMORY_LIMIT);
        assert!(result < block_size);
        assert!(result > 0);
        // must be multiple of 4
        assert_eq!(result % 4, 0);
    }

    #[test]
    fn chunk_size_is_at_most_block_size() {
        let block_size = 1024;
        let result = calculate_chunk_size_impl(block_size, 10, 5, DEFAULT_MEMORY_LIMIT);
        assert!(result <= block_size);
    }

    #[test]
    fn chunk_size_aligned_to_4_bytes() {
        // Force chunking by using a giant block size
        let block_size = 768 * 1024 * 1024; // 768MB
        let result = calculate_chunk_size_impl(block_size, 10, 10, DEFAULT_MEMORY_LIMIT);
        assert_eq!(result % 4, 0);
    }

    #[test]
    fn chunk_size_uses_turbo_style_bounded_overhead() {
        let block_size = 8 * 1024 * 1024;
        let recovery_count = 2;
        let source_count = 1_000;
        let memory_limit = block_size * (recovery_count + 26);

        let result =
            calculate_chunk_size_impl(block_size, source_count, recovery_count, memory_limit);

        assert_eq!(result, block_size);
    }

    #[test]
    fn chunk_size_is_capped_like_turbo() {
        let block_size = 64 * 1024 * 1024;
        let recovery_count = 1;
        let source_count = 4;
        let memory_limit = block_size * (recovery_count + 30);

        let result =
            calculate_chunk_size_impl(block_size, source_count, recovery_count, memory_limit);

        assert_eq!(result, MAX_CREATE_CHUNK_SIZE);
    }

    #[test]
    fn chunk_size_keeps_large_full_blocks_capped_like_turbo() {
        let block_size = 64 * 1024 * 1024;
        let recovery_count = 2;
        let source_count = 10;
        let memory_limit = block_size * (recovery_count + 26);

        let result =
            calculate_chunk_size_impl(block_size, source_count, recovery_count, memory_limit);

        assert_eq!(result, MAX_CREATE_CHUNK_SIZE);
    }

    #[test]
    fn process_config_marks_large_capped_blocks_non_deferred() {
        let block_size = BlockSize::new(64 * 1024 * 1024);
        let process_config =
            CreateProcessConfig::new(block_size, 10, 2, block_size.as_usize() * 28);

        assert_eq!(process_config.chunk_size.as_usize(), MAX_CREATE_CHUNK_SIZE);
        assert!(!process_config.defer_hash_computation);
    }

    #[test]
    fn process_config_marks_full_block_chunks_deferred() {
        let block_size = BlockSize::new(1024 * 1024);
        let process_config = CreateProcessConfig::new(block_size, 4, 2, DEFAULT_MEMORY_LIMIT);

        assert_eq!(process_config.chunk_size.as_usize(), block_size.as_usize());
        assert!(process_config.defer_hash_computation);
    }

    #[test]
    fn chunk_size_respects_explicit_memory_limit() {
        let block_size = 1024;
        let result = calculate_chunk_size_impl(block_size, 2, 2, 128);
        assert_eq!(result, 16);
    }

    #[test]
    fn packet_name_for_path_uses_relative_path_when_base_path_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base");
        let nested = base.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("file.bin");
        std::fs::write(&file, b"hello").unwrap();

        let context = test_context("out.par2", Some(base.clone()));
        assert_eq!(
            context.packet_name_for_path(&file).unwrap(),
            normalize_packet_path(std::path::Path::new("nested/file.bin"))
        );
    }

    #[test]
    fn packet_name_for_path_uses_canonical_base_when_literal_prefix_differs() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base");
        let nested = base.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("file.bin");
        std::fs::write(&file, b"hello").unwrap();

        let base_with_dotdots = tmp.path().join("base").join("..").join("base");
        let context = test_context("out.par2", Some(base_with_dotdots));
        assert_eq!(
            context.packet_name_for_path(&file).unwrap(),
            normalize_packet_path(std::path::Path::new("nested/file.bin"))
        );
    }

    #[test]
    fn packet_name_for_path_falls_back_when_outside_base_path() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base");
        let other = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let file = other.join("file.bin");
        std::fs::write(&file, b"hello").unwrap();

        let context = test_context("out.par2", Some(base));
        assert_eq!(
            context.packet_name_for_path(&file).unwrap(),
            packet_name_from_path(&file)
        );
    }

    #[test]
    fn packet_base_path_prefers_explicit_base_path_and_otherwise_uses_output_parent() {
        let explicit = test_context(
            "nested/out.par2",
            Some(std::path::PathBuf::from("/tmp/base")),
        );
        assert_eq!(
            explicit.packet_base_path().as_ref(),
            std::path::Path::new("/tmp/base")
        );

        let derived = test_context("nested/out.par2", None);
        assert_eq!(
            derived.packet_base_path().as_ref(),
            std::path::Path::new("nested")
        );
    }

    #[test]
    fn scan_source_files_skips_empty_files_and_uses_output_directory_for_packet_names() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let empty = tmp.path().join("empty.bin");
        let data = nested.join("data.bin");
        let output = tmp.path().join("archive.par2");
        std::fs::write(&empty, []).unwrap();
        std::fs::write(&data, b"hello world").unwrap();

        let mut context = test_context(output.to_str().unwrap(), None);
        context.config.source_files = vec![empty, data.clone()];

        context.scan_source_files().unwrap();

        assert_eq!(context.source_files.len(), 1);
        assert_eq!(context.source_files[0].path, data);
        assert_eq!(context.source_files[0].packet_name(), "nested/data.bin");
        assert_eq!(context.source_files[0].size, 11);
    }

    #[test]
    fn scan_source_files_rejects_missing_and_all_empty_inputs() {
        let tmp = tempfile::tempdir().unwrap();
        let empty = tmp.path().join("empty.bin");
        std::fs::write(&empty, []).unwrap();

        let mut empty_only = test_context(tmp.path().join("out.par2").to_str().unwrap(), None);
        empty_only.config.source_files = vec![empty];
        assert!(matches!(
            empty_only.scan_source_files(),
            Err(CreateError::EmptySourceFiles)
        ));

        let missing = tmp.path().join("missing.bin");
        let mut missing_ctx = test_context(tmp.path().join("out.par2").to_str().unwrap(), None);
        missing_ctx.config.source_files = vec![missing.clone()];
        assert!(matches!(
            missing_ctx.scan_source_files(),
            Err(CreateError::FileNotFound(path)) if path == missing.to_string_lossy()
        ));
    }

    #[test]
    fn calculate_block_size_rejects_target_smaller_than_file_count() {
        let mut context = test_context("out.par2", None);
        context.config.source_block_count = Some(SourceBlockCount::new(1));
        context.source_files = vec![source_info("a.bin", 8, 0), source_info("b.bin", 12, 1)];

        let error = context.calculate_block_size().unwrap_err();
        assert!(error
            .to_string()
            .contains("cannot be smaller than the number of files"));
    }

    #[test]
    fn calculate_block_size_uses_largest_file_when_target_equals_file_count() {
        let mut context = test_context("out.par2", None);
        context.config.source_block_count = Some(SourceBlockCount::new(2));
        context.source_files = vec![source_info("a.bin", 5, 0), source_info("b.bin", 10, 1)];

        context.calculate_block_size().unwrap();

        assert_eq!(context.block_size.as_u64(), 12);
        assert_eq!(context.source_block_count, 2);
    }

    #[test]
    fn calculate_block_size_uses_minimum_size_when_requested_blocks_exceed_total_units() {
        let mut context = test_context("out.par2", None);
        context.config.source_block_count = Some(SourceBlockCount::new(20));
        context.source_files = vec![source_info("a.bin", 8, 0), source_info("b.bin", 8, 1)];

        context.calculate_block_size().unwrap();

        assert_eq!(context.block_size.as_u64(), 4);
        assert_eq!(context.source_block_count, 4);
    }

    #[test]
    fn calculate_block_size_honors_explicit_block_size() {
        let mut context = test_context("out.par2", None);
        context.config.block_size = Some(16);
        context.source_files = vec![source_info("a.bin", 5, 0), source_info("b.bin", 20, 1)];

        context.calculate_block_size().unwrap();

        assert_eq!(context.block_size.as_u64(), 16);
        assert_eq!(context.source_block_count, 3);
    }

    #[test]
    fn calculate_block_size_recomputes_final_count_when_lower_bound_meets_upper_bound() {
        let mut context = test_context("out.par2", None);
        context.config.source_block_count = Some(SourceBlockCount::new(3));
        context.source_files = vec![source_info("a.bin", 4, 0), source_info("b.bin", 12, 1)];

        context.calculate_block_size().unwrap();

        assert_eq!(context.block_size.as_u64(), 8);
        assert_eq!(context.source_block_count, 3);
    }

    #[test]
    fn checked_recovery_block_count_rejects_impossible_requests() {
        let mut context = test_context("out.par2", None);
        context.config.first_recovery_block = 65_535;

        assert!(context.checked_recovery_block_count(65_537).is_err());
        assert!(context.checked_recovery_block_count(1).is_err());
    }

    #[test]
    fn calculate_recovery_blocks_for_target_size_never_returns_zero() {
        let mut context = test_context("out.par2", None);
        context.block_size = BlockSize::new(4);
        context.source_block_count = 2;
        context.source_files = vec![source_info("a.bin", 4, 0)];
        context.config.recovery_file_scheme = crate::create::RecoveryFileScheme::Uniform;

        assert_eq!(
            context
                .calculate_recovery_blocks_for_target_size(1)
                .unwrap(),
            1
        );
    }

    #[test]
    fn recovery_slice_packet_writer_streams_borrowed_data() {
        let set_id = RecoverySetId::new([0xAB; 16]);
        let recovery_data = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut bytes = Vec::new();

        write_recovery_slice_packet(&mut bytes, 7, &recovery_data, set_id).unwrap();

        assert_eq!(&bytes[0..8], crate::packets::MAGIC_BYTES);
        let length = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        assert_eq!(length as usize, bytes.len());
        assert_eq!(&bytes[32..48], set_id.as_bytes());
        assert_eq!(&bytes[48..64], RECOVERY_PACKET_TYPE);
        assert_eq!(u32::from_le_bytes(bytes[64..68].try_into().unwrap()), 7);
        assert_eq!(&bytes[68..], recovery_data.as_slice());

        let expected_md5 = crate::checksum::compute_md5_bytes(&bytes[32..]);
        assert_eq!(&bytes[16..32], expected_md5.as_slice());
    }

    #[test]
    fn create_uses_actual_first_recovery_block_exponent_for_parity_data() {
        let tmp = tempfile::tempdir().unwrap();
        let source_path = tmp.path().join("data.bin");
        let par2_path = tmp.path().join("data.par2");
        let data = vec![1u8, 0, 2, 0, 3, 0, 4, 0];
        std::fs::write(&source_path, &data).unwrap();

        let mut ctx = crate::create::CreateContextBuilder::new()
            .output_name(par2_path.to_str().unwrap())
            .source_files(vec![source_path])
            .block_size(4)
            .recovery_block_count(1)
            .first_recovery_block(7)
            .quiet(true)
            .build()
            .unwrap();

        ctx.create().unwrap();

        let par2_files: Vec<std::path::PathBuf> = ctx
            .output_files()
            .iter()
            .map(std::path::PathBuf::from)
            .collect();
        let packet_set = crate::par2_files::load_par2_packets(&par2_files, true, false);
        let recovery_packet = packet_set
            .packets
            .iter()
            .find_map(|packet| match packet {
                crate::Packet::RecoverySlice(recovery) => Some(recovery),
                _ => None,
            })
            .expect("recovery packet should be written");

        let encoder = crate::reed_solomon::RecoveryBlockEncoder::new(4, 2);
        let expected = encoder
            .encode_recovery_block(7, &[&data[0..4], &data[4..8]])
            .unwrap();

        assert_eq!(recovery_packet.exponent, 7);
        assert_eq!(recovery_packet.recovery_data, expected);
    }

    #[test]
    fn encode_and_hash_files_slim_mode_profiles_prepass_and_hashes_files() {
        use crate::checksum::{compute_block_checksums_padded, compute_file_id, compute_md5_only};

        let tmp = tempfile::tempdir().unwrap();
        let file_a_path = tmp.path().join("a.bin");
        let file_b_path = tmp.path().join("b.bin");
        let file_a = b"abcdefghijkl".to_vec();
        let file_b = b"xyz12".to_vec();
        std::fs::write(&file_a_path, &file_a).unwrap();
        std::fs::write(&file_b_path, &file_b).unwrap();

        let source_files = vec![
            SourceFileInfo::new(file_a_path.clone(), file_a.len() as u64, 0),
            SourceFileInfo::new(file_b_path.clone(), file_b.len() as u64, 1),
        ];
        let block_size = 8u64;
        let source_block_count: u32 = source_files
            .iter()
            .map(|file| file.calculate_block_count(block_size))
            .sum();
        let encoder = crate::reed_solomon::RecoveryBlockEncoder::new(
            block_size as usize,
            source_block_count as usize,
        );

        let (_recovery_blocks, hash_states, encode_profile) = encode_and_hash_files(
            &source_files,
            block_size,
            4,
            source_block_count,
            encoder.base_values(),
            0,
            1,
            1,
            &SilentCreateReporter,
            true,
        )
        .unwrap();

        let profile = encode_profile.expect("profile data should be returned when enabled");
        assert_eq!(
            profile.counters.source_hash_bytes_read,
            (file_a.len() + file_b.len()) as u64
        );
        assert!(profile.counters.source_seek_count >= source_files.len() as u64);
        assert!(profile.source_open_hash_prepass > std::time::Duration::ZERO);

        let expected_blocks = [&file_a[..8], &file_a[8..], &file_b[..]];
        let expected_offsets = [0u32, 2u32];
        let expected_full_hashes = [compute_md5_only(&file_a), compute_md5_only(&file_b)];
        let expected_16k_hashes = expected_full_hashes;
        let expected_file_ids = [
            compute_file_id(
                &expected_16k_hashes[0],
                file_a.len() as u64,
                source_files[0].packet_name().as_bytes(),
            ),
            compute_file_id(
                &expected_16k_hashes[1],
                file_b.len() as u64,
                source_files[1].packet_name().as_bytes(),
            ),
        ];

        assert_eq!(hash_states.len(), 2);
        assert_eq!(hash_states[0].hash_16k, expected_16k_hashes[0]);
        assert_eq!(hash_states[0].full_md5, expected_full_hashes[0]);
        assert_eq!(hash_states[0].file_id, expected_file_ids[0]);
        assert_eq!(hash_states[0].block_count, 2);
        assert_eq!(hash_states[0].global_block_offset, expected_offsets[0]);
        assert_eq!(hash_states[1].hash_16k, expected_16k_hashes[1]);
        assert_eq!(hash_states[1].full_md5, expected_full_hashes[1]);
        assert_eq!(hash_states[1].file_id, expected_file_ids[1]);
        assert_eq!(hash_states[1].block_count, 1);
        assert_eq!(hash_states[1].global_block_offset, expected_offsets[1]);

        let mut flattened = hash_states
            .into_iter()
            .flat_map(|state| state.block_checksums)
            .collect::<Vec<_>>();
        flattened.sort_by_key(|block| block.global_index);
        assert_eq!(flattened.len(), expected_blocks.len());

        for (idx, (actual, expected_data)) in flattened.iter().zip(expected_blocks).enumerate() {
            let (expected_md5, expected_crc32) =
                compute_block_checksums_padded(expected_data, block_size as usize);
            assert_eq!(actual.global_index, idx as u32);
            assert_eq!(actual.hash, expected_md5);
            assert_eq!(actual.crc32, expected_crc32.as_u32());
        }
    }

    #[test]
    fn create_with_profile_enabled_records_counters_and_outputs() {
        let _guard = env_lock();
        std::env::set_var("PAR2RS_CREATE_PROFILE", "1");

        let tmp = tempfile::tempdir().unwrap();
        let source_path = tmp.path().join("profile.bin");
        let par2_path = tmp.path().join("profile.par2");
        std::fs::write(&source_path, b"hello profile").unwrap();

        let mut context = crate::create::CreateContextBuilder::new()
            .output_name(par2_path.to_str().unwrap())
            .source_files(vec![source_path.clone()])
            .block_size(4)
            .recovery_block_count(1)
            .quiet(true)
            .build()
            .unwrap();

        context.create().unwrap();
        std::env::remove_var("PAR2RS_CREATE_PROFILE");

        assert!(context.output_files().len() >= 2);
        let profile = context.profile.as_ref().expect("profile should be enabled");
        let counters = profile.counters();
        assert_eq!(counters.source_file_count, 1);
        assert_eq!(counters.source_bytes, 13);
        assert_eq!(counters.block_size, 4);
        assert_eq!(counters.source_block_count, 4);
        assert_eq!(counters.recovery_block_count, 1);
        assert!(counters.chunk_size >= 4);
        assert!(counters.source_recovery_bytes_read >= 13);
        assert!(counters.recovery_chunk_count >= 1);
        assert!(counters.selected_backend.is_some());
    }

    // --- calculate_chunk_size (method) via CreateContextBuilder ---

    #[test]
    fn calculate_chunk_size_method_respects_block_size() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.dat");
        std::fs::write(&path, b"hello").unwrap();

        let ctx = crate::create::CreateContextBuilder::new()
            .output_name("out.par2")
            .source_files(vec![path])
            .block_size(4096)
            .recovery_block_count(2)
            .build()
            .unwrap();

        // 4096 * (2 + small_source_count) << 1GB, so chunk = full block
        assert_eq!(ctx.block_size(), 4096);
    }

    proptest! {
        #[test]
        fn chunk_size_impl_preserves_alignment_and_bounds(
            block_size in 4usize..(8 * 1024 * 1024),
            source_block_count in 1usize..512,
            recovery_block_count in 0usize..64,
            memory_limit in 4usize..(256 * 1024 * 1024),
        ) {
            let block_size = block_size & !3;
            prop_assume!(block_size >= 4);

            let result = calculate_chunk_size_impl(
                block_size,
                source_block_count,
                recovery_block_count,
                memory_limit,
            );
            let max_allowed = block_size.min(MAX_CREATE_CHUNK_SIZE);
            let block_overhead = 2 + (source_block_count + 1).min(24);
            let full_block_memory = block_size * (recovery_block_count + block_overhead);

            prop_assert!(result >= 4);
            prop_assert!(result <= max_allowed);
            prop_assert_eq!(result % 4, 0);

            if full_block_memory <= memory_limit {
                prop_assert_eq!(result, max_allowed);
            }
        }
    }
}
