//! CreateContext - Main context for PAR2 creation
//!
//! Reference: par2cmdline-turbo/src/par2creator.h Par2Creator class

use super::error::{CreateError, CreateResult};
use super::error_helpers::{create_file, get_metadata, open_for_reading, packet_write_error};
use super::packet_generator::generate_recovery_set_id;
use super::progress::CreateReporter;
use super::source_file::SourceFileInfo;
use super::types::CreateConfig;
use crate::domain::{BlockSize, ChunkSize, RecoverySetId, SourceBlockCount};

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
    use crate::packets::RecoverySlicePacket;
    use binrw::BinWrite;

    let packet_length = 8 + 8 + 16 + 16 + 16 + 4 + recovery_data.len() as u64;

    // Compute MD5 over: set_id || type || exponent || data
    let mut md5_data = Vec::with_capacity(16 + 16 + 4 + recovery_data.len());
    md5_data.extend_from_slice(recovery_set_id.as_bytes());
    md5_data.extend_from_slice(b"PAR 2.0\0RecvSlic");
    md5_data.extend_from_slice(&exponent.to_le_bytes());
    md5_data.extend_from_slice(recovery_data);
    let computed_md5 = crate::checksum::compute_md5_bytes(&md5_data);

    let packet = RecoverySlicePacket {
        length: packet_length,
        md5: crate::domain::Md5Hash::new(computed_md5),
        set_id: recovery_set_id,
        type_of_packet: *b"PAR 2.0\0RecvSlic",
        exponent,
        recovery_data: recovery_data.to_vec(),
    };

    let mut buf = Vec::with_capacity(packet_length as usize);
    packet
        .write_le(&mut std::io::Cursor::new(&mut buf))
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    writer.write_all(&buf)
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
) -> usize {
    const DEFAULT_MEMORY_LIMIT: usize = 1024 * 1024 * 1024; // 1 GB
    let block_overhead = source_block_count;
    let full_block_memory = block_size * (recovery_block_count + block_overhead);
    if full_block_memory <= DEFAULT_MEMORY_LIMIT {
        return block_size;
    }
    let chunk_size = DEFAULT_MEMORY_LIMIT / (recovery_block_count + block_overhead);
    let aligned = chunk_size & !3;
    aligned.min(block_size)
}

/// Pre-compute GF(2^16) coefficient matrix for Reed-Solomon encoding.
///
/// Returns `coefficients[recovery_idx][src_idx]` = base_values[src_idx] ^ recovery_idx.
///
/// Reference: parpar/gf16 coefficient tables
fn compute_gf_coefficients(base_values: &[u16], recovery_count: usize) -> Vec<Vec<u16>> {
    use crate::reed_solomon::galois::Galois16;
    (0..recovery_count as u16)
        .map(|exp| {
            base_values
                .iter()
                .map(|&b| Galois16::new(b).pow(exp).value())
                .collect()
        })
        .collect()
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
    coefficients: &[Vec<u16>],
    recovery_count: usize,
    thread_count: u32,
    reporter: &dyn CreateReporter,
) -> CreateResult<(RecoveryBlockVec, Vec<FileHashState>)> {
    use crate::checksum::{calculate_file_md5_16k, compute_file_id};
    use crate::create::source_file::BlockChecksum;
    use crc32fast::Hasher as Crc32Hasher;
    use md5::{Digest, Md5};
    use std::fs::File;
    use std::io::{Read, Seek};

    use crate::reed_solomon::codec::{
        build_split_mul_table, process_slice_multiply_add, process_slice_multiply_direct,
    };
    use crate::reed_solomon::galois::Galois16;

    let exponents: Vec<u16> = (0..recovery_count as u16).collect();
    let mut recovery_blocks: Vec<(u16, Vec<u8>)> =
        exponents.iter().map(|&e| (e, Vec::new())).collect();

    let mut file_handles: Vec<File> = Vec::with_capacity(source_files.len());
    let mut file_md5_states: Vec<Md5> = Vec::with_capacity(source_files.len());
    let mut file_16k_hashes: Vec<crate::domain::Md5Hash> = Vec::with_capacity(source_files.len());

    let mut block_md5_states: Vec<Md5> = Vec::with_capacity(source_block_count as usize);
    let mut block_crc32_states: Vec<Crc32Hasher> = Vec::with_capacity(source_block_count as usize);

    // Per-file metadata: (block_count, global_block_offset)
    let mut file_block_meta: Vec<(u32, u32)> = Vec::with_capacity(source_files.len());
    let mut global_block_offset = 0u32;

    for file in source_files {
        let hash_16k =
            calculate_file_md5_16k(&file.path).map_err(|e| CreateError::FileReadError {
                file: file.path.to_string_lossy().to_string(),
                source: e,
            })?;
        file_16k_hashes.push(hash_16k);
        file_handles.push(open_for_reading(&file.path)?);
        file_md5_states.push(Md5::new());

        let block_count = file.calculate_block_count(block_size);
        file_block_meta.push((block_count, global_block_offset));
        for _ in 0..block_count {
            block_md5_states.push(Md5::new());
            block_crc32_states.push(Crc32Hasher::new());
        }
        global_block_offset += block_count;
    }

    // Main chunk loop
    let mut block_offset = 0u64;
    while block_offset < block_size {
        let chunk_len = ((block_size - block_offset) as usize).min(chunk_size);

        let mut input_buffers: Vec<Vec<u8>> = Vec::with_capacity(source_block_count as usize);
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
                let mut chunk = vec![0u8; chunk_len];
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
                    file_md5_states[file_idx].update(&chunk[..bytes_to_read]);
                }
                block_md5_states[file_block_idx].update(&chunk[..bytes_to_read]);
                block_crc32_states[file_block_idx].update(&chunk[..bytes_to_read]);
                input_buffers.push(chunk);
                file_block_idx += 1;
            }
        }

        // RS encode this chunk
        if thread_count == 1 {
            let mut temp_buffer = vec![0u8; chunk_len];
            for (recovery_idx, (_exp, recovery_data)) in recovery_blocks.iter_mut().enumerate() {
                for (src_idx, input_chunk) in input_buffers.iter().enumerate() {
                    let coeff = Galois16::new(coefficients[recovery_idx][src_idx]);
                    let mul_table = build_split_mul_table(coeff);
                    if src_idx == 0 {
                        process_slice_multiply_direct(input_chunk, &mut temp_buffer, &mul_table);
                        recovery_data.extend_from_slice(&temp_buffer);
                    } else {
                        let start = recovery_data.len() - chunk_len;
                        process_slice_multiply_add(
                            input_chunk,
                            &mut recovery_data[start..],
                            &mul_table,
                        );
                    }
                }
            }
        } else {
            use rayon::prelude::*;
            recovery_blocks.par_iter_mut().enumerate().for_each(
                |(recovery_idx, (_exp, recovery_data))| {
                    let mut temp_buffer = vec![0u8; chunk_len];
                    for (src_idx, input_chunk) in input_buffers.iter().enumerate() {
                        let coeff = Galois16::new(coefficients[recovery_idx][src_idx]);
                        let mul_table = build_split_mul_table(coeff);
                        if src_idx == 0 {
                            process_slice_multiply_direct(
                                input_chunk,
                                &mut temp_buffer,
                                &mul_table,
                            );
                            recovery_data.extend_from_slice(&temp_buffer);
                        } else {
                            let start = recovery_data.len() - chunk_len;
                            process_slice_multiply_add(
                                input_chunk,
                                &mut recovery_data[start..],
                                &mul_table,
                            );
                        }
                    }
                },
            );
        }

        block_offset += chunk_len as u64;
        let progress = ((block_offset as f64 / block_size as f64) * recovery_count as f64) as u32;
        reporter
            .report_recovery_generation(progress.min(recovery_count as u32), recovery_count as u32);
    }

    // Finalize file MD5s and block checksums
    let finalized_file_md5s: Vec<[u8; 16]> = file_md5_states
        .into_iter()
        .map(|s| {
            let h = s.finalize();
            let mut b = [0u8; 16];
            b.copy_from_slice(&h);
            b
        })
        .collect();

    let mut hash_states: Vec<FileHashState> = Vec::with_capacity(source_files.len());
    let mut global_block_idx = 0usize;

    for (file_idx, file) in source_files.iter().enumerate() {
        let (block_count, g_offset) = file_block_meta[file_idx];
        let full_md5 = crate::domain::Md5Hash::new(finalized_file_md5s[file_idx]);
        let hash_16k = file_16k_hashes[file_idx];
        let filename = file
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .as_bytes();
        let file_id = compute_file_id(&hash_16k, file.size, filename);

        let mut checksums = Vec::with_capacity(block_count as usize);
        for block_idx in 0..block_count {
            let is_last = block_idx == block_count - 1;
            if is_last && file.size % block_size != 0 {
                let actual = (file.size % block_size) as usize;
                let padding = block_size as usize - actual;
                let zeros = vec![0u8; padding];
                block_md5_states[global_block_idx].update(&zeros);
                block_crc32_states[global_block_idx].update(&zeros);
            }
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

        reporter.report_file_hashing(&file.filename(), file.size, file.size);

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
            let source_info = SourceFileInfo::new(path.clone(), size, index);

            self.source_files.push(source_info);
        }

        // Validate we have at least some data
        let total_size: u64 = self.source_files.iter().map(|f| f.size).sum();
        if total_size == 0 {
            return Err(CreateError::EmptySourceFiles);
        }

        Ok(())
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
        if let Some(count) = self.config.recovery_block_count {
            // Explicit count specified
            self.recovery_block_count = count;
        } else if let Some(percent) = self.config.redundancy_percentage {
            // Calculate from percentage
            let count = (self.source_block_count as f64 * (percent as f64 / 100.0)).ceil() as u32;
            self.recovery_block_count = count.max(1); // At least 1 recovery block
        } else {
            return Err(CreateError::Other(
                "Must specify either recovery_block_count or redundancy_percentage".to_string(),
            ));
        }

        Ok(())
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

        let coefficients =
            compute_gf_coefficients(encoder.base_values(), self.recovery_block_count as usize);

        let (recovery_blocks, hash_states) = encode_and_hash_files(
            &self.source_files,
            self.block_size.as_u64(),
            chunk_size.as_usize(),
            self.source_block_count,
            &coefficients,
            self.recovery_block_count as usize,
            self.config.thread_count,
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
        let output_path = std::path::Path::new(&self.config.output_name);
        let output_dir = output_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();
        let base_name = output_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output")
            .to_string();

        // Write index file: critical packets only, no recovery data
        // Reference: par2cmdline-turbo creates base.par2 with no recovery slices
        let index_path = output_dir.join(format!("{}.par2", base_name));
        let mut index_file = create_file(&index_path)?;
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

        // Determine volume file plan
        let largest_file_size = self.source_files.iter().map(|f| f.size).max().unwrap_or(0);
        let file_count = {
            use super::file_naming::default_recovery_file_count;
            let default_count = default_recovery_file_count(self.recovery_block_count);
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

        // Write each volume file: critical packets + its slice of recovery blocks
        // Reference: par2cmdline-turbo/src/par2creator.cpp WriteRecoveryPackets()
        for entry in &plan {
            let vol_path = output_dir.join(&entry.filename);
            let mut vol_file = create_file(&vol_path)?;

            vol_file
                .write_all(&critical_bytes)
                .map_err(|e| CreateError::FileCreateError {
                    file: vol_path.to_string_lossy().to_string(),
                    source: e,
                })?;

            // Write recovery slice packets for this volume
            for i in 0..entry.block_count {
                let packet_exponent = entry.first_exponent + i;
                // Recovery blocks are indexed from first_recovery_block
                let local_idx = (packet_exponent - self.config.first_recovery_block) as usize;
                let (_, recovery_data) = &self.recovery_blocks[local_idx];

                write_recovery_slice_packet(
                    &mut vol_file,
                    packet_exponent,
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
        let result = calculate_chunk_size_impl(4096, 10, 5);
        assert_eq!(result, 4096);
    }

    #[test]
    fn chunk_size_reduces_when_exceeds_memory_limit() {
        // block_size=512MB, 4 source blocks, 4 recovery blocks
        // full_block_memory = 512MB * 8 = 4GB > 1GB
        let block_size = 512 * 1024 * 1024;
        let result = calculate_chunk_size_impl(block_size, 4, 4);
        assert!(result < block_size);
        assert!(result > 0);
        // must be multiple of 4
        assert_eq!(result % 4, 0);
    }

    #[test]
    fn chunk_size_is_at_most_block_size() {
        let block_size = 1024;
        let result = calculate_chunk_size_impl(block_size, 10, 5);
        assert!(result <= block_size);
    }

    #[test]
    fn chunk_size_aligned_to_4_bytes() {
        // Force chunking by using a giant block size
        let block_size = 768 * 1024 * 1024; // 768MB
        let result = calculate_chunk_size_impl(block_size, 10, 10);
        assert_eq!(result % 4, 0);
    }

    // --- compute_gf_coefficients ---

    #[test]
    fn gf_coefficients_zero_recovery_blocks_returns_empty() {
        let base_values = vec![1u16, 2u16, 3u16];
        let result = compute_gf_coefficients(&base_values, 0);
        assert!(result.is_empty());
    }

    #[test]
    fn gf_coefficients_one_recovery_block_exp0_gives_all_ones() {
        // x^0 = 1 in any field
        let base_values = vec![3u16, 7u16, 255u16];
        let result = compute_gf_coefficients(&base_values, 1);
        assert_eq!(result.len(), 1);
        // exp=0 means every base^0 = 1
        for &coeff in &result[0] {
            assert_eq!(coeff, 1, "x^0 should always be 1 in GF(2^16)");
        }
    }

    #[test]
    fn gf_coefficients_shape_is_recovery_x_source() {
        let base_values = vec![1u16, 2u16, 3u16, 4u16];
        let recovery_count = 5;
        let result = compute_gf_coefficients(&base_values, recovery_count);
        assert_eq!(result.len(), recovery_count);
        for row in &result {
            assert_eq!(row.len(), base_values.len());
        }
    }

    #[test]
    fn gf_coefficients_exp1_equals_base_values() {
        use crate::reed_solomon::galois::Galois16;
        let base_values = vec![2u16, 5u16, 10u16];
        let result = compute_gf_coefficients(&base_values, 2);
        // exp=1: base^1 = base
        for (i, &base) in base_values.iter().enumerate() {
            assert_eq!(result[1][i], Galois16::new(base).pow(1).value());
        }
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
