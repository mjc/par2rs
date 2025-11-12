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
    #[allow(dead_code)] // Will be used when implementing packet generation
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
                let total_size: u64 = self.source_files.iter().map(|f| (f.size + 3) / 4).sum();

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
                            count += ((file.size + 3) / 4 + size - 1) / size;
                        }

                        if count > block_count {
                            lower_bound = size + 1;
                            if lower_bound >= upper_bound {
                                size = lower_bound;
                                // Recalculate count with final size
                                count = 0;
                                for file in &self.source_files {
                                    count += ((file.size + 3) / 4 + size - 1) / size;
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

    /// Generate Reed-Solomon recovery blocks AND compute file hashes in single pass
    ///
    /// This eliminates the dual-pass bottleneck by computing:
    /// - Full file MD5 hashes (via Md5Reader wrapper)
    /// - Block MD5 + CRC32 checksums (after each complete block is read)
    /// - Reed-Solomon recovery blocks (existing chunked processing)
    ///
    /// Reference: par2cmdline-turbo/src/par2creator.cpp ProcessData()
    fn generate_recovery_blocks(&mut self) -> CreateResult<()> {
        use crate::checksum::{calculate_file_md5_16k, compute_file_id, Md5Reader};
        use crate::create::source_file::BlockChecksum;
        use crate::reed_solomon::RecoveryBlockEncoder;
        use std::fs::File;
        use std::io::Read;

        if self.recovery_block_count == 0 {
            self.reporter.report_scanning_files(
                0,
                0,
                "No recovery blocks to generate (redundancy = 0%)",
            );
            return Ok(());
        }

        // Create encoder
        let encoder =
            RecoveryBlockEncoder::new(self.block_size.as_usize(), self.source_block_count as usize);

        // Determine chunk size based on memory constraints
        // Reference: par2cmdline-turbo CalculateProcessBlockSize()
        let chunk_size = self.calculate_chunk_size();

        self.reporter.report_scanning_files(
            0,
            self.recovery_block_count as usize,
            &format!(
                "Processing files (chunk size: {} bytes)...",
                chunk_size.as_usize()
            ),
        );

        // Initialize recovery blocks (without pre-allocating capacity to save memory)
        // They will grow as we append chunks during processing
        let exponents: Vec<u16> = (0..self.recovery_block_count as u16).collect();
        let mut recovery_blocks: Vec<(u16, Vec<u8>)> =
            exponents.iter().map(|&exp| (exp, Vec::new())).collect();

        // Track MD5 readers and incremental block checksum state for each file
        // We'll open files once and keep them open for the entire chunked loop
        let mut file_readers: Vec<Md5Reader<File>> = Vec::new();

        // Track incremental MD5 and CRC32 state for each block (not full data!)
        // This is memory-efficient: only state objects, not 10GB of block data
        use crc32fast::Hasher as Crc32Hasher;
        use md5::{Digest, Md5};

        let mut block_md5_states: Vec<Md5> = Vec::new();
        let mut block_crc32_states: Vec<Crc32Hasher> = Vec::new();

        // Calculate global block offsets and open files
        let mut global_block_offset = 0u32;
        for file in &mut self.source_files {
            // Compute 16KB hash for FileId (need to read first 16KB)
            let hash_16k =
                calculate_file_md5_16k(&file.path).map_err(|e| CreateError::FileReadError {
                    file: file.path.to_string_lossy().to_string(),
                    source: e,
                })?;
            file.hash_16k = hash_16k;

            // Open file for processing with MD5 tracking
            let f = open_for_reading(&file.path)?;
            file_readers.push(Md5Reader::new(f));

            let block_count = file.calculate_block_count(self.block_size.as_u64());
            file.block_count = block_count;
            file.global_block_offset = global_block_offset;

            // Initialize incremental checksum state for each block
            for _ in 0..block_count {
                block_md5_states.push(Md5::new());
                block_crc32_states.push(Crc32Hasher::new());
            }

            global_block_offset += block_count;
        }

        // Process data in chunks (like par2cmdline-turbo ProcessData loop)
        // OPTIMIZATION: Compute hashes during this pass instead of separate hash pass

        // Import Reed-Solomon functions
        use crate::reed_solomon::codec::{
            build_split_mul_table, process_slice_multiply_add, process_slice_multiply_direct,
        };
        use crate::reed_solomon::galois::Galois16;

        // PARPAR OPTIMIZATION: Pre-compute GF(2^16) coefficients as u16 values
        // Build SIMD tables just-in-time in the hot loop (stays in registers)
        // This matches parpar's approach and saves memory bandwidth vs pre-built heap tables
        // Reference: par2cmdline-turbo/parpar/gf16/gf16_shuffle2x_x86.h:134-184
        let recovery_coefficients: Vec<Vec<u16>> = exponents
            .iter()
            .map(|&exp| {
                (0..self.source_block_count as usize)
                    .map(|src_idx| {
                        let base = Galois16::new(encoder.base_values()[src_idx]);
                        let coefficient = base.pow(exp);
                        coefficient.value() // Store just the 16-bit GF value
                    })
                    .collect()
            })
            .collect();

        let mut block_offset = 0u64;
        while block_offset < self.block_size.as_u64() {
            // Calculate how many bytes to read for this chunk
            // Don't exceed the block size
            let chunk_len =
                ((self.block_size.as_u64() - block_offset) as usize).min(chunk_size.as_usize());

            // Allocate buffers for input chunks (reused for each source block)
            let mut input_buffers: Vec<Vec<u8>> =
                Vec::with_capacity(self.source_block_count as usize);

            // Read chunks from all source blocks into input buffers
            // Use Md5Reader to accumulate hash during reads
            let mut file_block_idx = 0usize; // Global index across all files' blocks

            for (file_idx, file) in self.source_files.iter().enumerate() {
                let block_count = file.block_count;

                for block_idx in 0..block_count {
                    let is_last_block = block_idx == block_count - 1;
                    let block_size_actual =
                        if is_last_block && file.size % self.block_size.as_u64() != 0 {
                            (file.size % self.block_size.as_u64()) as usize
                        } else {
                            self.block_size.as_usize()
                        };

                    let bytes_available = if block_offset >= block_size_actual as u64 {
                        0
                    } else {
                        block_size_actual - block_offset as usize
                    };

                    let bytes_to_read = bytes_available.min(chunk_len);

                    let mut chunk = vec![0u8; chunk_len];
                    if bytes_to_read > 0 {
                        file_readers[file_idx]
                            .read_exact(&mut chunk[..bytes_to_read])
                            .map_err(|e| CreateError::FileReadError {
                                file: file.path.to_string_lossy().to_string(),
                                source: e,
                            })?;
                    }

                    // Update incremental checksums with actual bytes read
                    // Padding will be added once at the end, not in chunks
                    block_md5_states[file_block_idx].update(&chunk[..bytes_to_read]);
                    block_crc32_states[file_block_idx].update(&chunk[..bytes_to_read]);

                    input_buffers.push(chunk);
                    file_block_idx += 1;
                }
            }

            // Process this chunk through Reed-Solomon using just-in-time coefficient tables
            // Reference: RecoveryBlockEncoder::encode_recovery_block internals

            // Process recovery blocks - parallelize if thread_count > 1
            if self.config.thread_count == 1 {
                // Sequential processing - reuse single temp buffer
                let mut temp_buffer = vec![0u8; chunk_len];

                for (recovery_idx, (_exp, recovery_data)) in recovery_blocks.iter_mut().enumerate()
                {
                    for (src_idx, input_chunk) in input_buffers.iter().enumerate() {
                        // Build SIMD table just-in-time from coefficient (stays in registers/stack)
                        // This is faster than loading pre-built heap tables due to memory bandwidth
                        let coefficient =
                            Galois16::new(recovery_coefficients[recovery_idx][src_idx]);
                        let mul_table = build_split_mul_table(coefficient);

                        // Get or initialize the recovery block buffer for this chunk
                        if src_idx == 0 {
                            // First source: direct multiply into temp buffer
                            process_slice_multiply_direct(
                                input_chunk,
                                &mut temp_buffer,
                                &mul_table,
                            );
                            recovery_data.extend_from_slice(&temp_buffer);
                        } else {
                            // Subsequent sources: multiply-add into existing recovery data
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
                // Parallel processing of recovery blocks
                use rayon::prelude::*;

                recovery_blocks.par_iter_mut().enumerate().for_each(
                    |(recovery_idx, (_exp, recovery_data))| {
                        // Each thread gets its own temp buffer
                        let mut temp_buffer = vec![0u8; chunk_len];

                        for (src_idx, input_chunk) in input_buffers.iter().enumerate() {
                            // Build SIMD table just-in-time from coefficient (stays in registers/stack)
                            // This is faster than loading pre-built heap tables due to memory bandwidth
                            let coefficient =
                                Galois16::new(recovery_coefficients[recovery_idx][src_idx]);
                            let mul_table = build_split_mul_table(coefficient);

                            if src_idx == 0 {
                                // First source: direct multiply into temp buffer
                                process_slice_multiply_direct(
                                    input_chunk,
                                    &mut temp_buffer,
                                    &mul_table,
                                );
                                recovery_data.extend_from_slice(&temp_buffer);
                            } else {
                                // Subsequent sources: multiply-add into existing recovery data
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

            // Report progress
            let progress = ((block_offset as f64 / self.block_size.as_u64() as f64)
                * self.recovery_block_count as f64) as u32;
            self.reporter.report_recovery_generation(
                progress.min(self.recovery_block_count),
                self.recovery_block_count,
            );
        }

        // Finalize file hashes and compute block checksums
        // Now that we've read all the data through Md5Readers, finalize to get MD5 hashes
        let mut file_block_idx = 0usize;

        // Take ownership of file_readers to consume them
        let finalized_hashes: Vec<[u8; 16]> = file_readers
            .into_iter()
            .map(|reader| reader.finalize().1)
            .collect();

        for (file_idx, file) in self.source_files.iter_mut().enumerate() {
            // Get finalized MD5 hash for this file
            file.hash = crate::domain::Md5Hash::new(finalized_hashes[file_idx]);

            // Compute FileId from [Hash16k, Length, Name]
            let filename = file
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .as_bytes();
            file.file_id = compute_file_id(&file.hash_16k, file.size, filename);

            // Finalize block checksums from incremental state
            // Reference: Par2CreatorSourceFile::UpdateHashes and HasherGetBlock
            for block_idx in 0..file.block_count {
                let is_last_block = block_idx == file.block_count - 1;

                // Par2cmdline approach: hash actual data, then add zeropad to finalize
                // We've hashed the actual bytes in the chunk loop above
                // Now we need to add padding for partial blocks
                if is_last_block && file.size % self.block_size.as_u64() != 0 {
                    let actual_size = (file.size % self.block_size.as_u64()) as usize;
                    let padding_size = self.block_size.as_usize() - actual_size;

                    log::debug!(
                        "Block {}: Partial block - actual_size={}, padding_size={}, total={}",
                        file_block_idx,
                        actual_size,
                        padding_size,
                        actual_size + padding_size
                    );

                    // Add padding by hashing zeros
                    // Reference: parpar/hasher/hasher_input_base.h md5_final_block with zeroPad
                    let padding = vec![0u8; padding_size];
                    block_md5_states[file_block_idx].update(&padding);
                    block_crc32_states[file_block_idx].update(&padding);
                }

                // Finalize MD5
                let md5_hash = block_md5_states[file_block_idx].clone().finalize();
                let mut md5_bytes = [0u8; 16];
                md5_bytes.copy_from_slice(&md5_hash);

                // Finalize CRC32
                let crc32_value = block_crc32_states[file_block_idx].clone().finalize();

                log::debug!(
                    "Block {}: MD5={:02x}{:02x}{:02x}{:02x}..., CRC32={:08x}",
                    file_block_idx,
                    md5_bytes[0],
                    md5_bytes[1],
                    md5_bytes[2],
                    md5_bytes[3],
                    crc32_value
                );

                file.block_checksums.push(BlockChecksum {
                    crc32: crc32_value,
                    hash: crate::domain::Md5Hash::new(md5_bytes),
                    global_index: file.global_block_offset + block_idx,
                });

                file_block_idx += 1;
            }

            // Report file completion
            self.reporter
                .report_file_hashing(&file.filename(), file.size, file.size);
        }

        self.reporter.report_scanning_files(
            self.recovery_block_count as usize,
            self.recovery_block_count as usize,
            "Processing complete (hashes + recovery blocks)",
        );

        println!(); // Newline after progress line

        // Store recovery blocks for writing
        self.recovery_blocks = recovery_blocks;

        Ok(())
    }

    /// Calculate optimal chunk size for processing
    /// Reference: par2cmdline-turbo/src/par2creator.cpp:329-360 CalculateProcessBlockSize()
    fn calculate_chunk_size(&self) -> ChunkSize {
        // Always process full blocks at once
        // TODO: Implement proper seeking to support memory-limited chunking like par2cmdline-turbo
        // par2cmdline-turbo seeks to (block_offset + chunk_offset) for each block on each read
        // Our current sequential reader approach doesn't support this
        ChunkSize::new(self.block_size.as_usize())
    }

    /// Write PAR2 files (index + volume files)
    ///
    /// Reference: par2cmdline-turbo/src/par2creator.cpp WriteCriticalPackets() and
    /// WriteRecoveryPacketHeaders()
    fn write_par2_files(&mut self) -> CreateResult<()> {
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

        // Write index file (base.par2)
        let index_path = output_dir.join(format!("{}.par2", base_name));
        let mut index_file = create_file(&index_path)?;

        // Write packets using proper serialization functions that compute MD5
        // Reference: par2cmdline-turbo/src/par2creator.cpp WriteCriticalPackets()
        write_main_packet(&mut index_file, &main_packet)
            .map_err(|e| packet_write_error("main packet", e))?;

        write_creator_packet(&mut index_file, &creator_packet)
            .map_err(|e| packet_write_error("creator packet", e))?;

        for packet in &file_desc_packets {
            write_file_description_packet(&mut index_file, packet)
                .map_err(|e| packet_write_error("file description packet", e))?;
        }

        for packet in &file_verif_packets {
            write_file_verification_packet(&mut index_file, packet)
                .map_err(|e| packet_write_error("file verification packet", e))?;
        }

        // Write recovery slice packets
        // Reference: par2cmdline-turbo/src/par2creator.cpp WriteRecoveryPackets()
        for (exponent, recovery_data) in &self.recovery_blocks {
            use crate::packets::RecoverySlicePacket;
            use binrw::BinWrite;

            // Calculate packet length
            let packet_length = 8 + 8 + 16 + 16 + 16 + 4 + recovery_data.len() as u64;

            // Build packet
            let packet = RecoverySlicePacket {
                length: packet_length,
                md5: crate::domain::Md5Hash::new([0u8; 16]), // Will be computed
                set_id: recovery_set_id,
                type_of_packet: *b"PAR 2.0\0RecvSlic",
                exponent: *exponent as u32,
                recovery_data: recovery_data.clone(),
            };

            // Compute MD5 of packet body
            let mut md5_data = Vec::new();
            md5_data.extend_from_slice(recovery_set_id.as_bytes());
            md5_data.extend_from_slice(b"PAR 2.0\0RecvSlic");
            md5_data.extend_from_slice(&(*exponent as u32).to_le_bytes());
            md5_data.extend_from_slice(recovery_data);

            let computed_md5 = crate::checksum::compute_md5_bytes(&md5_data);

            let packet_with_md5 = RecoverySlicePacket {
                md5: crate::domain::Md5Hash::new(computed_md5),
                ..packet
            };

            // Write packet
            packet_with_md5
                .write_le(&mut index_file)
                .map_err(|e| packet_write_error("recovery packet", e))?;
        }

        index_file
            .flush()
            .map_err(|e| CreateError::FileCreateError {
                file: index_path.to_string_lossy().to_string(),
                source: e,
            })?;

        self.output_files
            .push(index_path.to_string_lossy().to_string());

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
