//! CreateContext - Main context for PAR2 creation
//!
//! Reference: par2cmdline-turbo/src/par2creator.h Par2Creator class

use super::error::{CreateError, CreateResult};
use super::error_helpers::{
    create_file, create_new_output_file, get_metadata, open_for_reading, packet_write_error,
};
use super::packet_generator::generate_recovery_set_id;
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
    use md5::{Digest, Md5};

    let packet_length = 8 + 8 + 16 + 16 + 16 + 4 + recovery_data.len() as u64;

    // Compute MD5 over: set_id || type || exponent || data
    let mut hasher = Md5::new();
    hasher.update(recovery_set_id.as_bytes());
    hasher.update(RECOVERY_PACKET_TYPE);
    hasher.update(exponent.to_le_bytes());
    hasher.update(recovery_data);
    let computed_md5 = hasher.finalize();

    writer.write_all(crate::packets::MAGIC_BYTES)?;
    writer.write_all(&packet_length.to_le_bytes())?;
    writer.write_all(&computed_md5)?;
    writer.write_all(recovery_set_id.as_bytes())?;
    writer.write_all(RECOVERY_PACKET_TYPE)?;
    writer.write_all(&exponent.to_le_bytes())?;
    writer.write_all(recovery_data)
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
) -> CreateResult<(RecoveryBlockVec, Vec<FileHashState>)> {
    use crate::checksum::{calculate_file_md5, compute_file_id};
    use crate::create::source_file::BlockChecksum;
    use crc32fast::Hasher as Crc32Hasher;
    use md5::{Digest, Md5};
    use std::fs::File;
    use std::io::{Read, Seek};

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(thread_count)
        .build()
        .map_err(|err| CreateError::Other(format!("failed to create thread pool: {err}")))?;

    let mut file_handles: Vec<File> = Vec::with_capacity(source_files.len());
    let mut file_md5_states: Vec<Md5> = Vec::with_capacity(source_files.len());
    let mut file_16k_buffers: Vec<Vec<u8>> = Vec::with_capacity(source_files.len());
    let full_file_hash_in_block_order = chunk_size >= block_size as usize;

    let mut block_md5_states: Vec<Md5> = Vec::with_capacity(source_block_count as usize);
    let mut block_crc32_states: Vec<Crc32Hasher> = Vec::with_capacity(source_block_count as usize);

    // Per-file metadata: (block_count, global_block_offset)
    let mut file_block_meta: Vec<(u32, u32)> = Vec::with_capacity(source_files.len());
    let mut global_block_offset = 0u32;

    for file in source_files {
        file_handles.push(open_for_reading(&file.path)?);
        file_md5_states.push(Md5::new());
        file_16k_buffers.push(vec![0u8; (file.size as usize).min(16 * 1024)]);

        let block_count = file.calculate_block_count(block_size);
        file_block_meta.push((block_count, global_block_offset));
        for _ in 0..block_count {
            block_md5_states.push(Md5::new());
            block_crc32_states.push(Crc32Hasher::new());
        }
        global_block_offset += block_count;
    }

    let recovery_blocks = pool.install(|| {
        let mut backend = CreateRecoveryBackend::new(
            base_values,
            first_recovery_block,
            recovery_count,
            chunk_size,
            thread_count,
        );
        let mut recovery_blocks = backend.recovery_blocks(block_size as usize);

        // Main chunk loop
        let mut block_offset = 0u64;
        while block_offset < block_size {
            let chunk_len = ((block_size - block_offset) as usize).min(chunk_size);
            backend.begin_chunk(chunk_len);

            let mut file_block_idx = 0usize;

            for (file_idx, file) in source_files.iter().enumerate() {
                let (block_count, _) = file_block_meta[file_idx];
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
                            file_handles[file_idx]
                                .seek(std::io::SeekFrom::Start(file_pos))
                                .map_err(|e| CreateError::FileReadError {
                                    file: file.path.to_string_lossy().to_string(),
                                    source: e,
                                })?;
                            file_handles[file_idx]
                                .read_exact(&mut chunk[..bytes_to_read])
                                .map_err(|e| CreateError::FileReadError {
                                    file: file.path.to_string_lossy().to_string(),
                                    source: e,
                                })?;
                            if file_pos < 16 * 1024 {
                                let capture_start = file_pos as usize;
                                let capture_end = (capture_start + bytes_to_read).min(16 * 1024);
                                let capture_len = capture_end - capture_start;
                                file_16k_buffers[file_idx][capture_start..capture_end]
                                    .copy_from_slice(&chunk[..capture_len]);
                            }
                            if full_file_hash_in_block_order {
                                file_md5_states[file_idx].update(&chunk[..bytes_to_read]);
                            }
                        }
                        block_md5_states[file_block_idx].update(&chunk[..chunk_len]);
                        block_crc32_states[file_block_idx].update(&chunk[..chunk_len]);
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

        Ok::<_, CreateError>(recovery_blocks)
    })?;

    // Finalize file MD5s and block checksums
    let full_file_hashes = if full_file_hash_in_block_order {
        file_md5_states
            .into_iter()
            .map(|s| {
                let h = s.finalize();
                let mut b = [0u8; 16];
                b.copy_from_slice(&h);
                crate::domain::Md5Hash::new(b)
            })
            .collect::<Vec<_>>()
    } else {
        source_files
            .iter()
            .map(|file| {
                calculate_file_md5(&file.path).map_err(|e| CreateError::FileReadError {
                    file: file.path.to_string_lossy().to_string(),
                    source: e,
                })
            })
            .collect::<CreateResult<Vec<_>>>()?
    };

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
            let md5_raw = block_md5_states[global_block_idx].clone().finalize();
            let mut md5_bytes = [0u8; 16];
            md5_bytes.copy_from_slice(&md5_raw);
            let crc32 = block_crc32_states[global_block_idx].clone().finalize();

            log::debug!(
                "Block {}: MD5={:02x}{:02x}..., CRC32={:08x}",
                global_block_idx,
                md5_bytes[0],
                md5_bytes[1],
                crc32
            );

            checksums.push(BlockChecksum {
                crc32,
                hash: crate::domain::Md5Hash::new(md5_bytes),
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

    Ok((recovery_blocks, hash_states))
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
        };

        // Perform initial setup
        context.scan_source_files()?;
        context.calculate_block_size()?;
        context.calculate_recovery_blocks()?;

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
        let chunk_size = self.calculate_chunk_size();

        self.reporter.report_scanning_files(
            0,
            self.recovery_block_count as usize,
            &format!(
                "Processing files (chunk size: {} bytes)...",
                chunk_size.as_usize()
            ),
        );

        let (recovery_blocks, hash_states) = encode_and_hash_files(
            &self.source_files,
            self.block_size.as_u64(),
            chunk_size.as_usize(),
            self.source_block_count,
            encoder.base_values(),
            self.config.first_recovery_block,
            self.recovery_block_count as usize,
            self.config.effective_threads(),
            self.reporter.as_ref(),
        )?;

        finalize_file_hashes(hash_states, &mut self.source_files)?;

        self.reporter.report_scanning_files(
            self.recovery_block_count as usize,
            self.recovery_block_count as usize,
            "Processing complete (hashes + recovery blocks)",
        );

        self.recovery_blocks = recovery_blocks;
        Ok(())
    }

    /// Calculate optimal chunk size for processing.
    /// Reference: par2cmdline-turbo/src/par2creator.cpp:329-360 CalculateProcessBlockSize()
    fn calculate_chunk_size(&self) -> ChunkSize {
        ChunkSize::new(calculate_chunk_size_impl(
            self.block_size.as_usize(),
            self.source_block_count as usize,
            self.recovery_block_count as usize,
            self.config.memory_limit.unwrap_or(DEFAULT_MEMORY_LIMIT),
        ))
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

        // Write each volume file: critical packets + its slice of recovery blocks
        // Reference: par2cmdline-turbo/src/par2creator.cpp WriteRecoveryPackets()
        for (entry, vol_path) in plan.iter().zip(volume_paths) {
            let mut vol_file = open_output(&vol_path, self.config.overwrite_existing)?;

            vol_file
                .write_all(&critical_bytes)
                .map_err(|e| CreateError::FileCreateError {
                    file: vol_path.to_string_lossy().to_string(),
                    source: e,
                })?;

            // Write recovery slice packets for this volume
            for i in 0..entry.block_count {
                let packet_exponent = entry.first_exponent + i;
                let local_idx = (packet_exponent - self.config.first_recovery_block) as usize;
                let (recovery_exponent, recovery_data) = &self.recovery_blocks[local_idx];

                write_recovery_slice_packet(
                    &mut vol_file,
                    *recovery_exponent as u32,
                    recovery_data,
                    recovery_set_id,
                )
                .map_err(|e| packet_write_error("recovery packet", e))?;
            }

            vol_file.flush().map_err(|e| CreateError::FileCreateError {
                file: vol_path.to_string_lossy().to_string(),
                source: e,
            })?;
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
    use super::*;

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
    fn chunk_size_respects_explicit_memory_limit() {
        let block_size = 1024;
        let result = calculate_chunk_size_impl(block_size, 2, 2, 128);
        assert_eq!(result, 16);
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
}
