//! High-level reconstruction interface
//!
//! This module provides the main interface for reconstructing missing file slices
//! using various Reed-Solomon implementations.

use crate::{RecoverySlicePacket};
use super::types::{ReconstructionConfig, ReconstructionResult};
use super::par2_reed_solomon::Par2ReedSolomon;
use std::collections::HashMap;

/// Main reconstruction engine that chooses the appropriate Reed-Solomon implementation
pub struct ReconstructionEngine {
    config: ReconstructionConfig,
    recovery_slices: Vec<RecoverySlicePacket>,
}

impl ReconstructionEngine {
    /// Create a new reconstruction engine
    pub fn new(
        slice_size: usize,
        total_input_slices: usize, 
        recovery_slices: Vec<RecoverySlicePacket>,
    ) -> Self {
        let config = ReconstructionConfig::new(
            slice_size, 
            total_input_slices, 
            recovery_slices.len()
        );
        
        Self {
            config,
            recovery_slices,
        }
    }

    /// Reconstruct missing slices
    pub fn reconstruct_missing_slices(
        &self,
        existing_slices: &HashMap<usize, Vec<u8>>,
        missing_slices: &[usize],
        global_slice_map: &HashMap<usize, usize>,
    ) -> ReconstructionResult {
        println!("Reconstruction engine: {} missing slices, {} recovery slices available", 
                missing_slices.len(), self.recovery_slices.len());

        // Use PAR2-specific Reed-Solomon implementation
        let par2_rs = Par2ReedSolomon::new(self.config.clone(), self.recovery_slices.clone());
        
        // Debug: Print recovery slice exponents
        let exponents = par2_rs.get_recovery_exponents();
        println!("Recovery slice exponents: {:?}", &exponents[..10.min(exponents.len())]);
        
        par2_rs.reconstruct_slices(existing_slices, missing_slices, global_slice_map)
    }

    /// Check if reconstruction is possible
    pub fn can_reconstruct(&self, missing_slices_count: usize) -> bool {
        self.config.is_feasible(missing_slices_count)
    }

    /// Get configuration info
    pub fn get_config(&self) -> &ReconstructionConfig {
        &self.config
    }
}
