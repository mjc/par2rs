//! Types and structures for Reed-Solomon operations

use std::collections::HashMap;

/// Result of a Reed-Solomon reconstruction operation
#[derive(Debug)]
pub struct ReconstructionResult {
    pub success: bool,
    pub reconstructed_slices: HashMap<usize, Vec<u8>>,
    pub error_message: Option<String>,
}

/// Configuration for Reed-Solomon reconstruction
#[derive(Debug, Clone)]
pub struct ReconstructionConfig {
    pub slice_size: usize,
    pub total_input_slices: usize,
    pub recovery_slices_count: usize,
    pub max_data_shards: usize,
    pub max_parity_shards: usize,
}

impl ReconstructionConfig {
    /// Create a new reconstruction config with sensible defaults
    pub fn new(slice_size: usize, total_input_slices: usize, recovery_slices_count: usize) -> Self {
        // Limit the number of shards to prevent excessive memory usage and computation
        let max_data_shards = 1000.min(total_input_slices);
        let max_parity_shards = 100.min(recovery_slices_count);
        
        Self {
            slice_size,
            total_input_slices,
            recovery_slices_count,
            max_data_shards,
            max_parity_shards,
        }
    }
    
    /// Check if reconstruction is feasible with current config
    pub fn is_feasible(&self, missing_slices_count: usize) -> bool {
        missing_slices_count <= self.max_parity_shards && 
        missing_slices_count <= self.recovery_slices_count
    }
}
