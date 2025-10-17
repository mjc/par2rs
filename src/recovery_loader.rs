//! Pluggable recovery data loading system
//!
//! This module provides a trait-based approach to loading recovery slice data,
//! allowing for different strategies like filesystem reads, memory mapping, etc.

use std::io;
use std::path::PathBuf;

/// Trait for loading recovery data from various sources
/// 
/// Implementations can use different strategies:
/// - FileSystemLoader: Standard filesystem reads (current implementation)
/// - MmapLoader: Memory-mapped files (future implementation)
/// - CachedLoader: LRU cache with on-demand loading (future implementation)
pub trait RecoveryDataLoader: Send + Sync {
    /// Load the full recovery data
    fn load_data(&self) -> io::Result<Vec<u8>>;
    
    /// Load a chunk of recovery data (memory-efficient)
    /// 
    /// # Arguments
    /// * `chunk_offset` - Byte offset within the recovery data (not file offset)
    /// * `chunk_size` - Number of bytes to read
    /// 
    /// # Returns
    /// Vector containing the requested chunk (may be smaller if at end of data)
    fn load_chunk(&self, chunk_offset: usize, chunk_size: usize) -> io::Result<Vec<u8>>;
    
    /// Get the size of the recovery data
    fn data_size(&self) -> usize;
}

/// Standard filesystem-based loader
/// Reads recovery data from files on demand
#[derive(Debug, Clone)]
pub struct FileSystemLoader {
    pub file_path: PathBuf,
    pub data_offset: u64,  // Byte offset in file where recovery_data starts
    pub data_size: usize,  // Length of recovery_data
}

impl RecoveryDataLoader for FileSystemLoader {
    fn load_data(&self) -> io::Result<Vec<u8>> {
        use std::fs::File;
        use std::io::{Read, Seek, SeekFrom};
        
        let mut file = File::open(&self.file_path)?;
        file.seek(SeekFrom::Start(self.data_offset))?;
        
        let mut data = vec![0u8; self.data_size];
        file.read_exact(&mut data)?;
        
        Ok(data)
    }
    
    fn load_chunk(&self, chunk_offset: usize, chunk_size: usize) -> io::Result<Vec<u8>> {
        use std::fs::File;
        use std::io::{Read, Seek, SeekFrom};
        
        // Return empty if offset is beyond data
        if chunk_offset >= self.data_size {
            return Ok(Vec::new());
        }
        
        // Calculate actual bytes to read (don't go past end of data)
        let bytes_to_read = (self.data_size - chunk_offset).min(chunk_size);
        
        // Open file and seek to the chunk position
        let mut file = File::open(&self.file_path)?;
        let absolute_offset = self.data_offset + chunk_offset as u64;
        file.seek(SeekFrom::Start(absolute_offset))?;
        
        // Read only the requested chunk
        let mut chunk = vec![0u8; bytes_to_read];
        file.read_exact(&mut chunk)?;
        
        Ok(chunk)
    }
    
    fn data_size(&self) -> usize {
        self.data_size
    }
}

// Future: MmapLoader implementation
// #[derive(Debug)]
// pub struct MmapLoader {
//     mmap: memmap2::Mmap,
//     data_offset: usize,
//     data_size: usize,
// }
//
// impl RecoveryDataLoader for MmapLoader {
//     fn load_data(&self) -> io::Result<Vec<u8>> {
//         Ok(self.mmap[self.data_offset..self.data_offset + self.data_size].to_vec())
//     }
//     
//     fn load_chunk(&self, chunk_offset: usize, chunk_size: usize) -> io::Result<Vec<u8>> {
//         let start = self.data_offset + chunk_offset;
//         let end = (start + chunk_size).min(self.data_offset + self.data_size);
//         Ok(self.mmap[start..end].to_vec())
//     }
//     
//     fn data_size(&self) -> usize {
//         self.data_size
//     }
// }
