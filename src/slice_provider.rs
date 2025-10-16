//! Slice data provider abstraction
//!
//! This module provides abstractions for accessing PAR2 slice data without
//! loading everything into memory. This is critical for memory-efficient repair
//! of large files (e.g., 8GB+ files).
//!
//! The design follows par2cmdline's approach of loading data in small chunks
//! (default 64KB) rather than loading entire slices or files into memory.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use crc32fast::Hasher as Crc32;
use rustc_hash::FxHashMap as HashMap;
use crate::repair::Crc32Value;

/// Default chunk size for reading data (64KB, same as par2cmdline)
pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;

/// Information about a slice location
#[derive(Debug, Clone)]
pub struct SliceLocation {
    /// Path to the file containing the slice
    pub file_path: PathBuf,
    /// Byte offset within the file
    pub offset: u64,
    /// Size of the slice (actual data, not including padding)
    pub size: usize,
    /// Expected CRC32 checksum (if available)
    pub expected_crc: Option<Crc32Value>,
}

/// Result of reading a chunk of data
#[derive(Debug)]
pub struct ChunkData {
    /// The data read (may be less than requested if at end of slice)
    pub data: Vec<u8>,
    /// Number of valid bytes in data
    pub valid_bytes: usize,
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
    fn verify_slice(&mut self, slice_index: usize) -> Result<Option<bool>, Box<dyn std::error::Error>>;
}

/// A slice provider that reads data in chunks from files
///
/// This provider maintains file handles and reads data on-demand,
/// keeping only a small working set in memory.
pub struct ChunkedSliceProvider {
    /// Map of slice index to location info
    slice_locations: HashMap<usize, SliceLocation>,
    /// Open file handles (cached for performance)
    file_handles: HashMap<PathBuf, BufReader<File>>,
    /// Slice size (for padding calculations)
    slice_size: usize,
    /// Cache of verified slices (to avoid re-verification)
    verified_slices: HashMap<usize, bool>,
}

impl ChunkedSliceProvider {
    /// Create a new chunked slice provider
    pub fn new(slice_size: usize) -> Self {
        ChunkedSliceProvider {
            slice_locations: HashMap::default(),
            file_handles: HashMap::default(),
            slice_size,
            verified_slices: HashMap::default(),
        }
    }

    /// Add a slice location
    pub fn add_slice(&mut self, slice_index: usize, location: SliceLocation) {
        self.slice_locations.insert(slice_index, location);
    }

    /// Get or open a file handle
    fn get_file_handle(&mut self, path: &Path) -> Result<&mut BufReader<File>, Box<dyn std::error::Error>> {
        if !self.file_handles.contains_key(path) {
            let file = File::open(path)?;
            let reader = BufReader::new(file);
            self.file_handles.insert(path.to_path_buf(), reader);
        }
        Ok(self.file_handles.get_mut(path).unwrap())
    }
}

impl SliceProvider for ChunkedSliceProvider {
    fn read_chunk(
        &mut self,
        slice_index: usize,
        chunk_offset: usize,
        chunk_size: usize,
    ) -> Result<ChunkData, Box<dyn std::error::Error>> {
        let location = self.slice_locations.get(&slice_index)
            .ok_or_else(|| format!("Slice {} not found", slice_index))?
            .clone();

        // Check if chunk_offset is beyond the slice
        if chunk_offset >= location.size {
            return Ok(ChunkData {
                data: vec![],
                valid_bytes: 0,
            });
        }

        // Calculate actual bytes to read (may be less than chunk_size at end)
        let bytes_to_read = (location.size - chunk_offset).min(chunk_size);
        
        // Allocate buffer
        let mut buffer = vec![0u8; bytes_to_read];

        // Get file handle and read data
        let reader = self.get_file_handle(&location.file_path)?;
        reader.seek(SeekFrom::Start(location.offset + chunk_offset as u64))?;
        let bytes_read = reader.read(&mut buffer)?;

        buffer.truncate(bytes_read);

        Ok(ChunkData {
            data: buffer,
            valid_bytes: bytes_read,
        })
    }

    fn get_slice_size(&self, slice_index: usize) -> Option<usize> {
        self.slice_locations.get(&slice_index).map(|loc| loc.size)
    }

    fn is_slice_available(&self, slice_index: usize) -> bool {
        self.slice_locations.contains_key(&slice_index)
    }

    fn available_slices(&self) -> Vec<usize> {
        self.slice_locations.keys().copied().collect()
    }

    fn verify_slice(&mut self, slice_index: usize) -> Result<Option<bool>, Box<dyn std::error::Error>> {
        // Check cache first
        if let Some(&verified) = self.verified_slices.get(&slice_index) {
            return Ok(Some(verified));
        }

        let location = self.slice_locations.get(&slice_index)
            .ok_or_else(|| format!("Slice {} not found", slice_index))?
            .clone();

        // If no expected CRC, can't verify
        let expected_crc = match location.expected_crc {
            Some(crc) => crc,
            None => return Ok(None),
        };

        // Read entire slice and compute CRC32
        // Note: PAR2 spec requires CRC32 on padded data
        let mut buffer = vec![0u8; self.slice_size];
        let reader = self.get_file_handle(&location.file_path)?;
        reader.seek(SeekFrom::Start(location.offset))?;
        let bytes_read = reader.read(&mut buffer[..location.size])?;

        if bytes_read != location.size {
            // Couldn't read full slice
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

/// A slice provider that reads recovery slice data in chunks
pub struct RecoverySliceProvider {
    /// Map of recovery slice index to data
    /// We still keep the full recovery data since they're typically much smaller
    /// and are accessed many times during reconstruction
    recovery_slices: HashMap<usize, Vec<u8>>,
}

impl RecoverySliceProvider {
    /// Create a new recovery slice provider
    pub fn new(_slice_size: usize) -> Self {
        RecoverySliceProvider {
            recovery_slices: HashMap::default(),
        }
    }

    /// Add a recovery slice
    pub fn add_recovery_slice(&mut self, exponent: usize, data: Vec<u8>) {
        self.recovery_slices.insert(exponent, data);
    }

    /// Get recovery slice data for a specific chunk
    pub fn get_recovery_chunk(
        &self,
        exponent: usize,
        chunk_offset: usize,
        chunk_size: usize,
    ) -> Result<ChunkData, Box<dyn std::error::Error>> {
        let data = self.recovery_slices.get(&exponent)
            .ok_or_else(|| format!("Recovery slice {} not found", exponent))?;

        if chunk_offset >= data.len() {
            return Ok(ChunkData {
                data: vec![],
                valid_bytes: 0,
            });
        }

        let bytes_to_read = (data.len() - chunk_offset).min(chunk_size);
        let chunk = data[chunk_offset..chunk_offset + bytes_to_read].to_vec();

        Ok(ChunkData {
            data: chunk,
            valid_bytes: bytes_to_read,
        })
    }

    /// Get all available recovery slice exponents
    pub fn available_exponents(&self) -> Vec<usize> {
        self.recovery_slices.keys().copied().collect()
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
        provider.add_slice(0, SliceLocation {
            file_path: temp_file.path().to_path_buf(),
            offset: 0,
            size: 1000,
            expected_crc: None,
        });

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
        let mut provider = RecoverySliceProvider::new(1024);
        let recovery_data = vec![0x55u8; 1024];
        provider.add_recovery_slice(0, recovery_data.clone());

        // Read chunk from recovery slice
        let chunk = provider.get_recovery_chunk(0, 0, 64).unwrap();
        assert_eq!(chunk.valid_bytes, 64);
        assert!(chunk.data.iter().all(|&b| b == 0x55));
    }
}
