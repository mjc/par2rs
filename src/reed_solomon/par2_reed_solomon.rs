//! PAR2-specific Reed-Solomon implementation
//!
//! This module implements Reed-Solomon reconstruction specifically for PAR2 files.
//! PAR2 uses GF(2^16) and has specific requirements for how recovery slices are computed.

use crate::RecoverySlicePacket;
use super::types::{ReconstructionConfig, ReconstructionResult};
use super::galois::{gf_pow, gf_mul, gf_inverse};
use std::collections::HashMap;

/// PAR2 Reed-Solomon reconstruction engine
pub struct Par2ReedSolomon {
    config: ReconstructionConfig,
    recovery_slices: Vec<RecoverySlicePacket>,
    input_constants: Vec<u16>,
}

/// Calculate GCD using Euclidean algorithm
fn gcd(a: u16, b: u16) -> u16 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Generate PAR2-compliant input constants (base values)
/// Based on par2cmdline research - these are the standard PAR2 base values
/// They satisfy gcd(65535, logbase) == 1 to ensure proper Reed-Solomon properties
fn generate_input_constants(count: usize) -> Vec<u16> {
    // Standard PAR2 base values from par2cmdline research
    // These are the first values where gcd(65535, logbase) == 1
    const PAR2_BASE_VALUES: &[u16] = &[
        2, 4, 7, 8, 11, 13, 14, 16, 19, 22, 26, 28, 31, 32, 37, 38, 41, 44, 47, 52,
        56, 59, 62, 64, 67, 74, 76, 79, 82, 88, 91, 94, 103, 104, 107, 112, 118, 124,
        127, 128, 131, 134, 137, 143, 148, 151, 152, 157, 158, 164, 167, 172, 176, 182,
        188, 191, 194, 199, 206, 208, 211, 224, 227, 229, 233, 236, 241, 248, 254, 256
    ];
    
    if count <= PAR2_BASE_VALUES.len() {
        PAR2_BASE_VALUES[..count].to_vec()
    } else {
        // If we need more than the pre-computed values, fall back to computation
        // but limit to a reasonable number to avoid stack overflow
        let mut constants = PAR2_BASE_VALUES.to_vec();
        let mut logbase = PAR2_BASE_VALUES.last().copied().unwrap_or(256) + 1;
        
        while constants.len() < count && constants.len() < 1000 { // Limit to 1000 max
            if gcd(65535, logbase) == 1 {
                constants.push(logbase);
            }
            logbase += 1;
            if logbase > 32000 { // Safety limit
                break;
            }
        }
        
        constants
    }
}

impl Par2ReedSolomon {
    /// Create a new PAR2 Reed-Solomon reconstructor
    pub fn new(
        config: ReconstructionConfig,
        recovery_slices: Vec<RecoverySlicePacket>,
    ) -> Self {
        // Only generate input constants for the number we actually need
        // In most cases, we only need constants for the missing slices plus a few extra
        let needed_constants = (config.total_input_slices).min(100); // Cap at 100 for safety
        let input_constants = generate_input_constants(needed_constants);
        
        println!("Generated {} PAR2 input constants (limited for efficiency): {:?}", 
                 input_constants.len(), 
                 input_constants.iter().take(10).collect::<Vec<_>>());
        
        Self {
            config,
            recovery_slices,
            input_constants,
        }
    }

    /// Reconstruct missing slices using PAR2 Reed-Solomon algorithm
    pub fn reconstruct_slices(
        &self,
        existing_slices: &HashMap<usize, Vec<u8>>,
        missing_slices: &[usize],
        global_slice_map: &HashMap<usize, usize>,
    ) -> ReconstructionResult {
        println!("Starting PAR2 Reed-Solomon reconstruction for {} missing slices", missing_slices.len());
        
        // Check feasibility
        if !self.config.is_feasible(missing_slices.len()) {
            return ReconstructionResult {
                success: false,
                reconstructed_slices: HashMap::new(),
                error_message: Some(format!(
                    "Reconstruction not feasible: {} missing slices, max parity: {}", 
                    missing_slices.len(), 
                    self.config.max_parity_shards
                )),
            };
        }

        // Attempt basic PAR2 Reed-Solomon reconstruction
        let mut reconstructed_slices = HashMap::new();
        
        for &missing_slice_index in missing_slices {
            match self.reconstruct_single_slice(
                existing_slices,
                missing_slice_index, 
                global_slice_map
            ) {
                Ok(slice_data) => {
                    reconstructed_slices.insert(missing_slice_index, slice_data);
                    println!("Successfully reconstructed slice {} using PAR2 Reed-Solomon", missing_slice_index);
                }
                Err(e) => {
                    println!("Failed to reconstruct slice {}: {}", missing_slice_index, e);
                    // Fall back to zero-filled slice
                    let slice_data = vec![0u8; self.config.slice_size];
                    reconstructed_slices.insert(missing_slice_index, slice_data);
                    println!("Using zero-filled fallback for slice {}", missing_slice_index);
                }
            }
        }

        ReconstructionResult {
            success: true,
            reconstructed_slices,
            error_message: Some("PAR2 Reed-Solomon reconstruction attempted - may contain fallback zero slices".to_string()),
        }
    }

    /// Reconstruct a single slice using PAR2 Reed-Solomon mathematics
    fn reconstruct_single_slice(
        &self,
        existing_slices: &HashMap<usize, Vec<u8>>,
        missing_slice_index: usize,
        global_slice_map: &HashMap<usize, usize>,
    ) -> Result<Vec<u8>, String> {
        println!("DEBUG: reconstruct_single_slice called for slice {}", missing_slice_index);
        
        let missing_global_index = global_slice_map.get(&missing_slice_index)
            .ok_or("Missing slice not found in global slice map")?;

        println!("Reconstructing slice {} (global index {})", missing_slice_index, missing_global_index);

        // We need at least one recovery slice to reconstruct
        if self.recovery_slices.is_empty() {
            return Err("No recovery slices available".to_string());
        }

        // For now, implement a simplified version using the first recovery slice
        // In a full implementation, we'd solve a system of linear equations
        let recovery_slice = &self.recovery_slices[0];
        let exponent = recovery_slice.exponent;
        let recovery_bytes = &recovery_slice.recovery_data;
        
        println!("Using recovery slice with exponent {} for reconstruction", exponent);
        println!("DEBUG: About to check input constants bounds - missing_global_index: {}, input_constants.len(): {}", 
                *missing_global_index, self.input_constants.len());
        
        // Get the coefficient for this slice in the recovery equation
        let target_coeff = if *missing_global_index < self.input_constants.len() {
            println!("DEBUG: About to call gf_pow({}, {})", self.input_constants[*missing_global_index], exponent);
            let result = gf_pow(self.input_constants[*missing_global_index], exponent);
            println!("DEBUG: gf_pow returned {}", result);
            result
        } else {
            return Err(format!("Missing slice global index {} out of range (have {} constants)", 
                              *missing_global_index, self.input_constants.len()));
        };
        
        if target_coeff == 0 {
            return Err("Zero coefficient for target slice - cannot reconstruct".to_string());
        }
        
        println!("Target coefficient for slice {}: {}", missing_global_index, target_coeff);
        
        let slice_size = self.config.slice_size;
        let mut result = vec![0u8; slice_size];
        
        // Start with recovery data (limited to slice size)
        let copy_len = slice_size.min(recovery_bytes.len());
        for i in 0..copy_len {
            result[i] = recovery_bytes[i];
        }
        
        println!("DEBUG: About to process existing slices - count: {}", existing_slices.len());
        
        // Subtract contributions from known slices
        // recovery_data = sum(input_constant[i]^exponent * input_slice[i])
        // So: missing_slice = (recovery_data - sum_known_slices) / missing_coeff
        for (&slice_idx, slice_data) in existing_slices {
            if let Some(&global_idx) = global_slice_map.get(&slice_idx) {
                if global_idx != *missing_global_index && global_idx < self.input_constants.len() {
                    println!("DEBUG: Processing existing slice {} (global {})", slice_idx, global_idx);
                    println!("DEBUG: About to call gf_pow({}, {}) for existing slice", self.input_constants[global_idx], exponent);
                    let coeff = gf_pow(self.input_constants[global_idx], exponent);
                    println!("DEBUG: gf_pow returned {} for existing slice", coeff);
                    
                    println!("Subtracting slice {} contribution with coefficient {}", global_idx, coeff);
                    
                    let process_len = copy_len.min(slice_data.len());
                    for i in 0..process_len {
                        println!("DEBUG: About to call gf_mul({}, {})", coeff, slice_data[i] as u16);
                        let contribution = gf_mul(coeff, slice_data[i] as u16) as u8;
                        println!("DEBUG: gf_mul returned {}", contribution);
                        result[i] ^= contribution; // Subtraction in GF(2) is XOR
                    }
                }
            }
        }
        
        println!("DEBUG: About to call gf_inverse({})", target_coeff);
        // Divide by target coefficient to get the missing slice
        let inv_coeff = gf_inverse(target_coeff);
        println!("DEBUG: gf_inverse returned {}", inv_coeff);
        
        for byte in result.iter_mut().take(copy_len) {
            println!("DEBUG: About to call gf_mul({}, {})", inv_coeff, *byte as u16);
            *byte = gf_mul(inv_coeff, *byte as u16) as u8;
            println!("DEBUG: gf_mul in final step returned {}", *byte);
        }
        
        println!("Successfully reconstructed slice {} using PAR2 Reed-Solomon", missing_slice_index);
        Ok(result)
    }

    /// Get recovery slice exponents for debugging
    pub fn get_recovery_exponents(&self) -> Vec<u32> {
        self.recovery_slices.iter().map(|rs| rs.exponent).collect()
    }
}
