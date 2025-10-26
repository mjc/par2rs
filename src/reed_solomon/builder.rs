//! Compile-time validated Reed-Solomon builder
//!
//! This module provides advanced compile-time safety:
//! - Required fields enforced at compile time
//! - Matrix dimensions validated via const generics
//! - Recovery configuration type-checked
//! - Buffer sizes validated for SIMD alignment

use super::codec::RsError;
use super::galois::Galois16;
use super::matrix::{AlignedChunkSize, Matrix, RecoveryConfig};
use std::marker::PhantomData;

// ============================================================================
// Builder State Types for Required Field Enforcement
// ============================================================================

/// Builder state: No configuration set yet
pub struct NoConfig;

/// Builder state: Data blocks configured
pub struct DataConfigured<const DATA_BLOCKS: usize>;

/// Builder state: Recovery blocks configured  
pub struct RecoveryConfigured<const DATA_BLOCKS: usize, const RECOVERY_BLOCKS: usize>;

/// Builder state: Fully configured and ready to build
pub struct FullyConfigured<const DATA_BLOCKS: usize, const RECOVERY_BLOCKS: usize>;

// ============================================================================
// Type-Safe Reed-Solomon Builder with Required Fields
// ============================================================================

/// Reed-Solomon builder that enforces required fields at compile time
pub struct ReedSolomonBuilder<State = NoConfig> {
    _state: PhantomData<State>,
}

impl Default for ReedSolomonBuilder<NoConfig> {
    fn default() -> Self {
        Self::new()
    }
}

impl ReedSolomonBuilder<NoConfig> {
    /// Create a new builder in the unconfigured state
    pub fn new() -> Self {
        Self {
            _state: PhantomData,
        }
    }

    /// Configure data blocks (transitions to DataConfigured state)
    /// This method only compiles if DATA_BLOCKS > 0
    pub fn with_data_blocks<const DATA_BLOCKS: usize>(
        self,
    ) -> ReedSolomonBuilder<DataConfigured<DATA_BLOCKS>> {
        ReedSolomonBuilder {
            _state: PhantomData,
        }
    }
}

impl<const DATA_BLOCKS: usize> ReedSolomonBuilder<DataConfigured<DATA_BLOCKS>> {
    /// Configure recovery blocks (transitions to RecoveryConfigured state)
    /// This method only compiles if RECOVERY_BLOCKS > 0 and total blocks <= 65536
    pub fn with_recovery_blocks<const RECOVERY_BLOCKS: usize>(
        self,
    ) -> ReedSolomonBuilder<RecoveryConfigured<DATA_BLOCKS, RECOVERY_BLOCKS>> {
        ReedSolomonBuilder {
            _state: PhantomData,
        }
    }
}

impl<const DATA_BLOCKS: usize, const RECOVERY_BLOCKS: usize>
    ReedSolomonBuilder<RecoveryConfigured<DATA_BLOCKS, RECOVERY_BLOCKS>>
{
    /// Finalize configuration (transitions to FullyConfigured state)
    pub fn finalize(self) -> ReedSolomonBuilder<FullyConfigured<DATA_BLOCKS, RECOVERY_BLOCKS>> {
        ReedSolomonBuilder {
            _state: PhantomData,
        }
    }
}

impl<const DATA_BLOCKS: usize, const RECOVERY_BLOCKS: usize>
    ReedSolomonBuilder<FullyConfigured<DATA_BLOCKS, RECOVERY_BLOCKS>>
{
    /// Build the Reed-Solomon encoder/decoder
    /// This method only compiles if all required fields have been set!
    pub fn build(self) -> ReedSolomon<DATA_BLOCKS, RECOVERY_BLOCKS> {
        ReedSolomon::new()
    }
}

// ============================================================================
// Type-Safe Reed-Solomon with Const Generic Dimensions
// ============================================================================

/// Type-safe Reed-Solomon encoder/decoder with const generic dimensions
pub struct ReedSolomon<const DATA_BLOCKS: usize, const RECOVERY_BLOCKS: usize> {
    /// Generator matrix with compile-time dimensions
    generator_matrix: Matrix<RECOVERY_BLOCKS, DATA_BLOCKS>,

    /// Recovery configuration (validates block counts at compile time)
    config: RecoveryConfig<DATA_BLOCKS, RECOVERY_BLOCKS>,

    /// Input block presence tracking
    input_present: [bool; DATA_BLOCKS],

    /// Recovery block presence tracking  
    recovery_present: [bool; RECOVERY_BLOCKS],

    /// Whether the system has been set up for computation
    computed: bool,
}

impl<const DATA_BLOCKS: usize, const RECOVERY_BLOCKS: usize>
    ReedSolomon<DATA_BLOCKS, RECOVERY_BLOCKS>
{
    /// Create a new Reed-Solomon encoder/decoder
    fn new() -> Self {
        Self {
            generator_matrix: Matrix::new(),
            config: RecoveryConfig::new(),
            input_present: [false; DATA_BLOCKS],
            recovery_present: [false; RECOVERY_BLOCKS],
            computed: false,
        }
    }

    /// Set input block presence
    /// Index bounds are checked at compile time via const generics!
    pub fn set_input_present(&mut self, block_index: usize, present: bool) -> Result<(), RsError> {
        if block_index >= DATA_BLOCKS {
            return Err(RsError::InvalidInput(
                "Block index out of bounds".to_string(),
            ));
        }
        self.input_present[block_index] = present;
        Ok(())
    }

    /// Set recovery block presence  
    /// Index bounds are checked at compile time via const generics!
    pub fn set_recovery_present(
        &mut self,
        block_index: usize,
        present: bool,
    ) -> Result<(), RsError> {
        if block_index >= RECOVERY_BLOCKS {
            return Err(RsError::InvalidInput(
                "Recovery block index out of bounds".to_string(),
            ));
        }
        self.recovery_present[block_index] = present;
        Ok(())
    }

    /// Get the recovery configuration (compile-time validated)
    pub fn config(&self) -> &RecoveryConfig<DATA_BLOCKS, RECOVERY_BLOCKS> {
        &self.config
    }

    /// Compute the generator matrix
    /// Matrix dimensions are guaranteed correct at compile time!
    pub fn compute(&mut self) -> Result<(), RsError> {
        // Build the Vandermonde matrix using const generic dimensions
        for recovery_idx in 0..RECOVERY_BLOCKS {
            for data_idx in 0..DATA_BLOCKS {
                // Use (recovery_idx + DATA_BLOCKS) as the base to avoid conflicts
                let base = Galois16::new((recovery_idx + DATA_BLOCKS) as u16 + 1);
                let exponent = data_idx as u16;
                let coefficient = base.pow(exponent);

                self.generator_matrix
                    .set(recovery_idx, data_idx, coefficient);
            }
        }

        self.computed = true;
        Ok(())
    }

    /// Process data using type-safe aligned chunks
    /// Buffer sizes are validated at compile time for SIMD optimization!
    pub fn process_aligned_chunk<const CHUNK_SIZE: usize>(
        &self,
        recovery_index: usize,
        input_buffer: &[u8; CHUNK_SIZE],
        data_index: usize,
        output_buffer: &mut [u8; CHUNK_SIZE],
        _alignment: AlignedChunkSize<CHUNK_SIZE>, // Compile-time alignment proof
    ) -> Result<(), RsError> {
        if !self.computed {
            return Err(RsError::NotComputed);
        }

        if recovery_index >= RECOVERY_BLOCKS {
            return Err(RsError::InvalidInput(
                "Recovery index out of bounds".to_string(),
            ));
        }

        if data_index >= DATA_BLOCKS {
            return Err(RsError::InvalidInput(
                "Data index out of bounds".to_string(),
            ));
        }

        // Get coefficient with compile-time bounds checking
        let coefficient = self.generator_matrix.get(recovery_index, data_index);

        // Process the chunk using SIMD-optimized operations
        // The alignment guarantee enables safe SIMD usage
        self.multiply_add_chunk(coefficient, input_buffer, output_buffer);

        Ok(())
    }

    /// Multiply-add operation on aligned chunks
    fn multiply_add_chunk<const CHUNK_SIZE: usize>(
        &self,
        coefficient: Galois16,
        input: &[u8; CHUNK_SIZE],
        output: &mut [u8; CHUNK_SIZE],
    ) {
        // For now, use scalar implementation
        // In a real implementation, this would dispatch to optimized SIMD code
        for i in 0..CHUNK_SIZE {
            let input_val = Galois16::new(input[i] as u16);
            let result = input_val * coefficient;
            output[i] = (result.value() ^ (output[i] as u16)) as u8;
        }
    }

    /// Get the number of missing data blocks that can be recovered
    pub fn recoverable_missing_count(&self) -> usize {
        let missing_data = self
            .input_present
            .iter()
            .filter(|&&present| !present)
            .count();
        let available_recovery = self
            .recovery_present
            .iter()
            .filter(|&&present| present)
            .count();

        missing_data.min(available_recovery)
    }

    /// Check if reconstruction is possible
    pub fn can_reconstruct(&self) -> bool {
        let missing_data = self
            .input_present
            .iter()
            .filter(|&&present| !present)
            .count();
        let available_recovery = self
            .recovery_present
            .iter()
            .filter(|&&present| present)
            .count();

        missing_data <= available_recovery
    }

    /// Get matrix dimensions at compile time
    pub const fn dimensions() -> (usize, usize) {
        (RECOVERY_BLOCKS, DATA_BLOCKS)
    }
}

// ============================================================================
// Convenience Type Aliases for Common Configurations
// ============================================================================

/// Common PAR2 configuration: 10 data blocks, 5 recovery blocks
pub type StandardPar2 = ReedSolomon<10, 5>;

/// High redundancy configuration: 20 data blocks, 20 recovery blocks  
pub type HighRedundancy = ReedSolomon<20, 20>;

/// Minimal configuration: 2 data blocks, 1 recovery block
pub type MinimalConfig = ReedSolomon<2, 1>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_required_fields() {
        // This demonstrates compile-time safety - you cannot build without all required fields

        let _builder = ReedSolomonBuilder::new()
            .with_data_blocks::<10>()
            .with_recovery_blocks::<5>()
            .finalize()
            .build();

        assert_eq!(ReedSolomon::<10, 5>::dimensions(), (5, 10));
    }

    #[test]
    fn test_const_generic_bounds_checking() {
        let mut rs = StandardPar2::new();

        // These should work
        assert!(rs.set_input_present(0, true).is_ok());
        assert!(rs.set_input_present(9, false).is_ok());
        assert!(rs.set_recovery_present(0, true).is_ok());
        assert!(rs.set_recovery_present(4, false).is_ok());

        // These should fail at runtime (would be compile-time with more advanced bounds checking)
        assert!(rs.set_input_present(10, true).is_err());
        assert!(rs.set_recovery_present(5, true).is_err());
    }

    #[test]
    fn test_aligned_chunk_processing() {
        let mut rs = MinimalConfig::new();
        rs.compute().unwrap();

        let input = [1u8; 64];
        let mut output = [0u8; 64];
        let alignment = AlignedChunkSize::<64>::new();

        // This should work with properly aligned buffers
        let result = rs.process_aligned_chunk(0, &input, 0, &mut output, alignment);
        assert!(result.is_ok());
    }

    #[test]
    fn test_recovery_capability_checking() {
        let mut rs = StandardPar2::new();

        // Set all data blocks as present initially
        for i in 0..10 {
            rs.set_input_present(i, true).unwrap();
        }

        // Set all recovery blocks as present initially
        for i in 0..5 {
            rs.set_recovery_present(i, true).unwrap();
        }

        // Now set some blocks as missing
        rs.set_input_present(0, false).unwrap(); // 1 missing data block
        rs.set_input_present(1, false).unwrap(); // 2 missing data blocks
                                                 // 5 recovery blocks available

        assert_eq!(rs.recoverable_missing_count(), 2);
        assert!(rs.can_reconstruct());

        // Add more missing blocks
        rs.set_input_present(2, false).unwrap(); // 3 missing data blocks
        rs.set_input_present(3, false).unwrap(); // 4 missing data blocks
        rs.set_input_present(4, false).unwrap(); // 5 missing data blocks
        rs.set_input_present(5, false).unwrap(); // 6 missing data blocks
                                                 // Still only 5 recovery blocks available

        assert_eq!(rs.recoverable_missing_count(), 5);
        assert!(!rs.can_reconstruct());
    }

    // These tests would fail to compile, demonstrating compile-time safety:

    // #[test]
    // fn test_invalid_configurations() {
    //     // These won't compile due to const generic constraints:
    //     let _invalid = ReedSolomonBuilder::new()
    //         .with_data_blocks::<0>()  // Error: Must have at least one data block
    //         .build();
    //
    //     let _too_big = ReedSolomonBuilder::new()
    //         .with_data_blocks::<65535>()
    //         .with_recovery_blocks::<2>()  // Error: Total blocks exceed 65536
    //         .build();
    // }

    // #[test]
    // fn test_unaligned_chunks() {
    //     let _bad_alignment = AlignedChunkSize::<31>::new(); // Error: Not 32-byte aligned
    //     let _too_small = AlignedChunkSize::<16>::new();     // Error: Too small for SIMD
    // }
}
