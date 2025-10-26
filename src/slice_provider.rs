//! Slice data provider abstraction
//!
//! This module provides abstractions for accessing PAR2 slice data without
//! loading everything into memory. This is critical for memory-efficient repair
//! of large files (e.g., 8GB+ files).
//!
//! The design follows par2cmdline's approach of loading data in small chunks
//! (default 64KB) rather than loading entire slices or files into memory.

use crate::domain::Crc32Value;
use crate::RecoverySliceMetadata;
use crc32fast::Hasher as Crc32;
use rustc_hash::FxHashMap as HashMap;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Default chunk size for reading data (64KB, same as par2cmdline)
pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;

/// Actual file data size (may be less than slice_size for last slice)
/// This is how many bytes are ACTUALLY in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActualDataSize(usize);

impl ActualDataSize {
    pub fn new(size: usize) -> Self {
        Self(size)
    }

    pub fn as_usize(&self) -> usize {
        self.0
    }
}

/// Logical slice size (always the full slice_size from PAR2, zero-padded if needed)
/// This is the size Reed-Solomon sees - always consistent across all slices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogicalSliceSize(usize);

impl LogicalSliceSize {
    pub fn new(size: usize) -> Self {
        Self(size)
    }

    pub fn as_usize(&self) -> usize {
        self.0
    }
}

/// Information about a slice location
#[derive(Debug, Clone)]
pub struct SliceLocation {
    /// Path to the file containing the slice
    pub file_path: PathBuf,
    /// Byte offset within the file
    pub offset: u64,
    /// Actual size of data in file (may be less than logical_size for last slice)
    pub actual_size: ActualDataSize,
    /// Logical size for Reed-Solomon (always slice_size, zero-padded)
    pub logical_size: LogicalSliceSize,
    /// Expected CRC32 checksum (if available)
    pub expected_crc: Option<Crc32Value>,
}

/// Result of reading a chunk of data
///
/// INVARIANT: valid_bytes == data.len() OR valid_bytes == 0 (for error case)
/// If valid_bytes > 0, data MUST contain exactly that many bytes
#[derive(Debug)]
pub struct ChunkData {
    /// The data read (may be less than requested if at end of slice)
    pub data: Vec<u8>,
    /// Number of valid bytes in data
    /// MUST equal data.len() if > 0
    pub valid_bytes: usize,
}

impl ChunkData {
    /// Create ChunkData, enforcing invariant
    pub fn new(data: Vec<u8>) -> Self {
        let valid_bytes = data.len();
        ChunkData { data, valid_bytes }
    }

    /// Create empty ChunkData (for errors)
    pub fn empty() -> Self {
        ChunkData {
            data: vec![],
            valid_bytes: 0,
        }
    }
}

/// Trait for providing slice data on-demand
pub trait SliceProvider {
    /// Read a chunk of data from a slice
    ///
    /// # Arguments
    /// * `slice_index` - Global slice index
    /// * `chunk_offset` - Byte offset within the slice
    /// * `chunk_size` - Number of bytes to read
    ///
    /// # Returns
    /// ChunkData with the requested data, or an error
    fn read_chunk(
        &mut self,
        slice_index: usize,
        chunk_offset: usize,
        chunk_size: usize,
    ) -> Result<ChunkData, Box<dyn std::error::Error>>;

    /// Get the size of a slice
    fn get_slice_size(&self, slice_index: usize) -> Option<usize>;

    /// Check if a slice is available (exists and is valid)
    fn is_slice_available(&self, slice_index: usize) -> bool;

    /// Get the list of available slice indices
    fn available_slices(&self) -> Vec<usize>;

    /// Verify a slice's checksum (if available)
    /// Returns true if valid, false if invalid, None if no checksum available
    fn verify_slice(
        &mut self,
        slice_index: usize,
    ) -> Result<Option<bool>, Box<dyn std::error::Error>>;
}

/// A slice provider that reads data in chunks from files
///
/// This provider maintains file handles and reads data on-demand,
/// keeping only a small working set in memory.
pub struct ChunkedSliceProvider {
    /// Map of slice index to location info - BTreeMap maintains sorted order!
    slice_locations: BTreeMap<usize, SliceLocation>,
    /// Open file handles (cached for performance)
    file_handles: HashMap<PathBuf, BufReader<File>>,
    /// Logical slice size for Reed-Solomon (all slices appear this size, zero-padded)
    logical_slice_size: LogicalSliceSize,
    /// Cache of verified slices (to avoid re-verification)
    verified_slices: HashMap<usize, bool>,
    /// Read-ahead cache: (slice_index, chunk_offset) -> data
    /// Caches upcoming chunks to reduce I/O operations
    chunk_cache: HashMap<(usize, usize), Vec<u8>>,
    /// Maximum number of chunks to cache (limits memory usage)
    max_cache_size: usize,
    /// LRU tracking: tracks last access order for cache eviction
    cache_access_counter: usize,
    cache_access_times: HashMap<(usize, usize), usize>,
}

impl ChunkedSliceProvider {
    /// Create a new chunked slice provider
    pub fn new(slice_size: usize) -> Self {
        ChunkedSliceProvider {
            slice_locations: BTreeMap::new(),
            file_handles: HashMap::default(),
            logical_slice_size: LogicalSliceSize::new(slice_size),
            verified_slices: HashMap::default(),
            chunk_cache: HashMap::default(),
            // Cache up to 1000 chunks (64MB with 64KB chunks) - reasonable memory usage
            max_cache_size: 1000,
            cache_access_counter: 0,
            cache_access_times: HashMap::default(),
        }
    }

    /// Add a slice location
    pub fn add_slice(&mut self, slice_index: usize, location: SliceLocation) {
        self.slice_locations.insert(slice_index, location);
    }

    /// Get or open a file handle
    fn get_file_handle(
        &mut self,
        path: &Path,
    ) -> Result<&mut BufReader<File>, Box<dyn std::error::Error>> {
        if !self.file_handles.contains_key(path) {
            let file = File::open(path)?;
            let reader = BufReader::new(file);
            self.file_handles.insert(path.to_path_buf(), reader);
        }
        Ok(self
            .file_handles
            .get_mut(path)
            .expect("File handle must exist after insertion"))
    }

    /// Find the least recently used cache entry for eviction
    fn find_lru_cache_entry(&self) -> Option<(usize, usize)> {
        self.cache_access_times
            .iter()
            .min_by_key(|(_, &access_time)| access_time)
            .map(|(&key, _)| key)
    }

    /// Prefetch upcoming chunks from a slice to reduce I/O operations
    /// Reads ahead 3-5 chunks at a time in a single sequential read
    fn prefetch_chunks(
        &mut self,
        slice_index: usize,
        start_offset: usize,
        chunk_size: usize,
        location: &SliceLocation,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Don't prefetch if cache is getting full
        if self.chunk_cache.len() >= self.max_cache_size * 9 / 10 {
            return Ok(());
        }

        // Prefetch next 4 chunks (256KB read-ahead with 64KB chunks)
        const PREFETCH_COUNT: usize = 4;

        for i in 0..PREFETCH_COUNT {
            let prefetch_offset = start_offset + (i * chunk_size);

            // Stop if beyond actual file data
            if prefetch_offset >= location.actual_size.as_usize() {
                break;
            }

            let cache_key = (slice_index, prefetch_offset);

            // Skip if already cached
            if self.chunk_cache.contains_key(&cache_key) {
                continue;
            }

            // Evict LRU entry if cache is full
            if self.chunk_cache.len() >= self.max_cache_size {
                if let Some(lru_key) = self.find_lru_cache_entry() {
                    self.chunk_cache.remove(&lru_key);
                    self.cache_access_times.remove(&lru_key);
                } else {
                    break; // Can't evict, stop prefetching
                }
            }

            // Calculate bytes to read
            let bytes_to_read = (location.actual_size.as_usize() - prefetch_offset).min(chunk_size);
            let mut buffer = vec![0u8; bytes_to_read];

            // Read the chunk
            let reader = self.get_file_handle(&location.file_path)?;
            reader.seek(SeekFrom::Start(location.offset + prefetch_offset as u64))?;
            let bytes_read = reader.read(&mut buffer)?;

            buffer.truncate(bytes_read);

            // Add to cache with access time
            self.cache_access_counter += 1;
            self.chunk_cache.insert(cache_key, buffer);
            self.cache_access_times
                .insert(cache_key, self.cache_access_counter);
        }

        Ok(())
    }
}

impl SliceProvider for ChunkedSliceProvider {
    fn read_chunk(
        &mut self,
        slice_index: usize,
        chunk_offset: usize,
        chunk_size: usize,
    ) -> Result<ChunkData, Box<dyn std::error::Error>> {
        // Check cache first
        let cache_key = (slice_index, chunk_offset);
        if let Some(cached_data) = self.chunk_cache.get(&cache_key) {
            self.cache_access_counter += 1;
            self.cache_access_times
                .insert(cache_key, self.cache_access_counter);
            return Ok(ChunkData::new(cached_data.clone()));
        }

        let location = self
            .slice_locations
            .get(&slice_index)
            .ok_or_else(|| format!("Slice {} not found", slice_index))?
            .clone();

        // CRITICAL: Chunk beyond logical slice = ERROR (caller screwed up)
        if chunk_offset >= location.logical_size.as_usize() {
            return Err(format!(
                "BUG: Requested chunk at offset {} but logical slice size is only {}",
                chunk_offset,
                location.logical_size.as_usize()
            )
            .into());
        }

        // Calculate how much of the chunk is real data vs zero padding
        let actual_data_end = location.actual_size.as_usize();

        if chunk_offset >= actual_data_end {
            // ENTIRELY in padding region - return chunk_size zeros
            // Reed-Solomon MUST see these zeros, not skip them!
            let padding_size = chunk_size.min(location.logical_size.as_usize() - chunk_offset);
            return Ok(ChunkData::new(vec![0u8; padding_size]));
        }

        // Some real data, possibly some padding
        let bytes_to_read = (actual_data_end - chunk_offset).min(chunk_size);
        let mut buffer = vec![0u8; bytes_to_read];

        let reader = self.get_file_handle(&location.file_path)?;
        reader.seek(SeekFrom::Start(location.offset + chunk_offset as u64))?;
        let bytes_read = reader.read(&mut buffer)?;
        buffer.truncate(bytes_read);

        // If this chunk extends into padding, add zeros
        let chunk_end = chunk_offset + chunk_size;
        if chunk_end > actual_data_end && chunk_end <= location.logical_size.as_usize() {
            let padding_needed = chunk_end - actual_data_end.max(chunk_offset + bytes_read);
            buffer.resize(bytes_read + padding_needed, 0);
        }

        // Cache with LRU eviction
        self.cache_access_counter += 1;
        if self.chunk_cache.len() >= self.max_cache_size {
            if let Some(lru_key) = self.find_lru_cache_entry() {
                self.chunk_cache.remove(&lru_key);
                self.cache_access_times.remove(&lru_key);
            }
        }
        self.chunk_cache.insert(cache_key, buffer.clone());
        self.cache_access_times
            .insert(cache_key, self.cache_access_counter);

        // Prefetch next chunks for sequential access optimization
        self.prefetch_chunks(
            slice_index,
            chunk_offset + chunk_size,
            chunk_size,
            &location,
        )?;

        Ok(ChunkData::new(buffer))
    }

    fn get_slice_size(&self, slice_index: usize) -> Option<usize> {
        // Return LOGICAL size (what Reed-Solomon sees), not actual size
        self.slice_locations
            .get(&slice_index)
            .map(|loc| loc.logical_size.as_usize())
    }

    fn is_slice_available(&self, slice_index: usize) -> bool {
        self.slice_locations.contains_key(&slice_index)
    }

    fn available_slices(&self) -> Vec<usize> {
        self.slice_locations.keys().copied().collect()
    }

    fn verify_slice(
        &mut self,
        slice_index: usize,
    ) -> Result<Option<bool>, Box<dyn std::error::Error>> {
        // Check cache first
        if let Some(&verified) = self.verified_slices.get(&slice_index) {
            return Ok(Some(verified));
        }

        let location = self
            .slice_locations
            .get(&slice_index)
            .ok_or_else(|| format!("Slice {} not found", slice_index))?
            .clone();

        // If no expected CRC, can't verify
        let expected_crc = match location.expected_crc {
            Some(crc) => crc,
            None => return Ok(None),
        };

        // Read entire slice and compute CRC32
        // Note: PAR2 spec requires CRC32 on padded data (full logical size)
        let mut buffer = vec![0u8; self.logical_slice_size.as_usize()];
        let reader = self.get_file_handle(&location.file_path)?;
        reader.seek(SeekFrom::Start(location.offset))?;
        let bytes_read = reader.read(&mut buffer[..location.actual_size.as_usize()])?;

        if bytes_read != location.actual_size.as_usize() {
            // Couldn't read full actual data from file
            self.verified_slices.insert(slice_index, false);
            return Ok(Some(false));
        }

        // Compute CRC32 on padded buffer
        let mut hasher = Crc32::new();
        hasher.update(&buffer);
        let computed_crc = Crc32Value::new(hasher.finalize());

        let is_valid = computed_crc == expected_crc;
        self.verified_slices.insert(slice_index, is_valid);

        Ok(Some(is_valid))
    }
}

/// Provider for recovery slice data with memory-efficient lazy loading
///
/// Uses metadata to load recovery data on-demand from disk in chunks,
/// avoiding loading all recovery data into memory (saves ~1.8GB for large PAR2 sets)
pub struct RecoverySliceProvider {
    /// Map of recovery slice index to metadata (lazy loading)
    recovery_metadata: HashMap<usize, RecoverySliceMetadata>,
}

impl RecoverySliceProvider {
    /// Create a new recovery slice provider
    pub fn new(_slice_size: usize) -> Self {
        RecoverySliceProvider {
            recovery_metadata: HashMap::default(),
        }
    }

    /// Add recovery slice metadata for lazy loading
    pub fn add_recovery_metadata(&mut self, exponent: usize, metadata: RecoverySliceMetadata) {
        self.recovery_metadata.insert(exponent, metadata);
    }

    /// Get recovery slice data for a specific chunk (loads from disk on-demand)
    pub fn get_recovery_chunk(
        &self,
        exponent: usize,
        chunk_offset: usize,
        chunk_size: usize,
    ) -> Result<ChunkData, Box<dyn std::error::Error>> {
        // Load only the requested chunk from disk (memory-efficient!)
        let metadata = self
            .recovery_metadata
            .get(&exponent)
            .ok_or_else(|| format!("Recovery slice {} not found", exponent))?;

        let chunk = metadata
            .load_chunk(chunk_offset, chunk_size)
            .map_err(|e| format!("Failed to load chunk: {}", e))?;

        Ok(ChunkData::new(chunk))
    }

    /// Get all available recovery slice exponents
    pub fn available_exponents(&self) -> Vec<usize> {
        let mut exponents: Vec<usize> = self.recovery_metadata.keys().copied().collect();
        exponents.sort_unstable();
        exponents
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_chunked_slice_provider() {
        // Create a temporary file with test data
        let mut temp_file = NamedTempFile::new().unwrap();
        let test_data = vec![0x42u8; 1000]; // 1000 bytes of 0x42
        temp_file.write_all(&test_data).unwrap();
        temp_file.flush().unwrap();

        let mut provider = ChunkedSliceProvider::new(1024);
        provider.add_slice(
            0,
            SliceLocation {
                file_path: temp_file.path().to_path_buf(),
                offset: 0,
                actual_size: ActualDataSize::new(1000),
                logical_size: LogicalSliceSize::new(1000),
                expected_crc: None,
            },
        );

        // Read first chunk
        let chunk = provider.read_chunk(0, 0, 64).unwrap();
        assert_eq!(chunk.valid_bytes, 64);
        assert_eq!(chunk.data.len(), 64);
        assert!(chunk.data.iter().all(|&b| b == 0x42));

        // Read chunk at end
        let chunk = provider.read_chunk(0, 950, 64).unwrap();
        assert_eq!(chunk.valid_bytes, 50); // Only 50 bytes left
        assert_eq!(chunk.data.len(), 50);
    }

    #[test]
    fn test_recovery_slice_provider() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create a temporary file with recovery data
        let mut temp_file = NamedTempFile::new().unwrap();
        let recovery_data = vec![0x55u8; 1024];
        temp_file.write_all(&recovery_data).unwrap();
        temp_file.flush().unwrap();

        // Create metadata for lazy loading
        let metadata = crate::RecoverySliceMetadata::from_file(
            0, // exponent
            crate::domain::RecoverySetId::new([0u8; 16]),
            temp_file.path().to_path_buf(),
            0,    // offset
            1024, // size
        );

        let mut provider = RecoverySliceProvider::new(1024);
        provider.add_recovery_metadata(0, metadata);

        // Read chunk from recovery slice (should load from disk on-demand)
        let chunk = provider.get_recovery_chunk(0, 0, 64).unwrap();
        assert_eq!(chunk.valid_bytes, 64);
        assert!(chunk.data.iter().all(|&b| b == 0x55));
    }
}
