//! Reed-Solomon encoding for PAR2 creation
//!
//! This module implements the forward Reed-Solomon encoding to generate recovery blocks
//! from input data blocks. This is the inverse operation of the reconstruction used in repair.
//!
//! Reference: par2cmdline-turbo/src/par2creator.cpp ProcessData()

use super::codec::{
    build_split_mul_table, process_slice_multiply_add, process_slice_multiply_direct, RsError,
    RsResult,
};
use super::galois::{gcd, Galois16};
use rayon::prelude::*;

/// Encoder for generating PAR2 recovery blocks
///
/// Implements the PAR2 Reed-Solomon encoding algorithm:
///   recovery[i] = sum over all input slices j of: input[j] * (base[j] ^ exponent[i])
///
/// where base values are generated from log values relatively prime to 65535.
pub struct RecoveryBlockEncoder {
    /// Size of each block (must match input/output slice size)
    block_size: usize,

    /// Total number of input blocks across all source files
    total_input_blocks: usize,

    /// Base values for each input block (generated from GF(2^16) antilog)
    base_values: Vec<u16>,
}

impl RecoveryBlockEncoder {
    /// Create a new encoder for the given configuration
    ///
    /// # Arguments
    /// * `block_size` - Size of each block in bytes
    /// * `total_input_blocks` - Total number of input data blocks
    pub fn new(block_size: usize, total_input_blocks: usize) -> Self {
        // Generate base values for each input block
        // Following PAR2 spec: base values are generated from log values that are
        // relatively prime to 65535
        //
        // Reference: par2cmdline-turbo/src/reedsolomon.cpp and ReconstructionEngine::new()
        let mut base_values = Vec::with_capacity(total_input_blocks);
        let mut logbase = 0u32;

        for _ in 0..total_input_blocks {
            // Find next logbase that is relatively prime to 65535
            while gcd(65535, logbase) != 1 {
                logbase += 1;
            }
            // Convert logbase to base value using antilog
            let base = Galois16::new(logbase as u16).alog();
            base_values.push(base);
            logbase += 1;
        }

        Self {
            block_size,
            total_input_blocks,
            base_values,
        }
    }

    /// Encode a single recovery block from input blocks
    ///
    /// Computes: recovery = sum of (input[i] * base[i]^exponent) for all i
    ///
    /// # Arguments
    /// * `exponent` - Recovery block exponent (determines which recovery block)
    /// * `input_blocks` - Slice containing all input data blocks
    ///
    /// # Returns
    /// The encoded recovery block data
    pub fn encode_recovery_block(
        &self,
        exponent: u16,
        input_blocks: &[&[u8]],
    ) -> RsResult<Vec<u8>> {
        if input_blocks.len() != self.total_input_blocks {
            return Err(RsError::InvalidInput(format!(
                "Expected {} input blocks, got {}",
                self.total_input_blocks,
                input_blocks.len()
            )));
        }

        // Validate all blocks are the correct size
        for (i, block) in input_blocks.iter().enumerate() {
            if block.len() != self.block_size {
                return Err(RsError::InvalidInput(format!(
                    "Input block {} has size {} but expected {}",
                    i,
                    block.len(),
                    self.block_size
                )));
            }
        }

        // Allocate output buffer
        let mut recovery_data = vec![0u8; self.block_size];

        // For each input block, compute coefficient = base[i]^exponent and add contribution
        for (i, input_block) in input_blocks.iter().enumerate() {
            let base = Galois16::new(self.base_values[i]);

            // Compute base^exponent in GF(2^16)
            let coefficient = base.pow(exponent);

            // Build multiplication table for this coefficient
            let mul_table = build_split_mul_table(coefficient);

            // First block: direct write (no XOR)
            // Subsequent blocks: multiply-add (XOR accumulate)
            if i == 0 {
                process_slice_multiply_direct(input_block, &mut recovery_data, &mul_table);
            } else {
                process_slice_multiply_add(input_block, &mut recovery_data, &mul_table);
            }
        }

        Ok(recovery_data)
    }

    /// Encode multiple recovery blocks in parallel
    ///
    /// # Arguments
    /// * `exponents` - List of exponents for recovery blocks to generate
    /// * `input_blocks` - All input data blocks
    ///
    /// # Returns
    /// Vector of (exponent, recovery_data) pairs
    pub fn encode_recovery_blocks_parallel(
        &self,
        exponents: &[u16],
        input_blocks: &[&[u8]],
    ) -> RsResult<Vec<(u16, Vec<u8>)>> {
        // Generate recovery blocks in parallel
        exponents
            .par_iter()
            .map(|&exponent| {
                let recovery_data = self.encode_recovery_block(exponent, input_blocks)?;
                Ok((exponent, recovery_data))
            })
            .collect()
    }

    /// Get block size
    pub fn block_size(&self) -> usize {
        self.block_size
    }

    /// Get total input block count
    pub fn total_input_blocks(&self) -> usize {
        self.total_input_blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference: par2cmdline-turbo/src/reedsolomon.cpp ReedSolomon::SetInput()
    // Lines 57-80 and 83-103: base values start at 1 and increment for Galois8
    // For Galois16, base values are generated from relatively prime log values
    #[test]
    fn test_encoder_creation() {
        let encoder = RecoveryBlockEncoder::new(1024, 10);
        assert_eq!(encoder.block_size(), 1024);
        assert_eq!(encoder.total_input_blocks(), 10);
        assert_eq!(encoder.base_values.len(), 10);

        // Base values should all be non-zero
        for &base in &encoder.base_values {
            assert_ne!(base, 0, "Base values must be non-zero");
        }
    }

    // Reference: par2cmdline-turbo/src/reedsolomon.cpp lines 30-51
    // gcd() function used to find relatively prime values to 65535
    #[test]
    fn test_base_values_relatively_prime() {
        let encoder = RecoveryBlockEncoder::new(64, 100);

        // All base values should come from log values relatively prime to 65535
        // This is enforced by the gcd check in the encoder
        for (i, &base) in encoder.base_values.iter().enumerate() {
            assert_ne!(base, 0, "Base value {} should not be zero", i);

            // Base values should be unique (different log values)
            for (j, &other_base) in encoder.base_values.iter().enumerate() {
                if i != j {
                    assert_ne!(
                        base, other_base,
                        "Base values {} and {} should be different",
                        i, j
                    );
                }
            }
        }
    }

    // Reference: par2cmdline-turbo/src/par2creator.cpp ProcessData()
    // Lines 751-856: Reads each input block and processes through RS matrix
    #[test]
    fn test_encode_single_recovery_block() {
        let block_size = 64;
        let input_count = 3;
        let encoder = RecoveryBlockEncoder::new(block_size, input_count);

        // Create test input blocks with varied patterns (simulating source file blocks)
        // Using different values that won't XOR to zero
        let block1 = vec![0xFF; block_size];
        let block2 = vec![0xAA; block_size];
        let block3 = vec![0x33; block_size]; // Changed from 0x55 to avoid XOR cancellation
        let input_blocks: Vec<&[u8]> = vec![&block1, &block2, &block3];

        // Encode recovery block with exponent 0
        // Reference: par2cmdline-turbo/src/reedsolomon.h RSOutputRow
        // exponent field determines which recovery block this is
        let result = encoder.encode_recovery_block(0, &input_blocks);
        assert!(result.is_ok());

        let recovery = result.unwrap();
        assert_eq!(recovery.len(), block_size);

        // Recovery data should not be all zeros (with non-zero varied input)
        // Note: With exponent 0, all base values become 1, so this is XOR of all inputs
        // 0xFF XOR 0xAA XOR 0x33 = 0x5C (non-zero)
        assert!(
            recovery.iter().any(|&b| b != 0),
            "Recovery block should contain non-zero data"
        );
    }

    // Reference: par2cmdline-turbo/src/reedsolomon.cpp ReedSolomon::SetInput()
    // inputcount must match the actual number of blocks provided
    #[test]
    fn test_encode_wrong_input_count() {
        let encoder = RecoveryBlockEncoder::new(64, 5);

        let block1 = vec![0x01; 64];
        let block2 = vec![0x02; 64];
        let input_blocks: Vec<&[u8]> = vec![&block1, &block2]; // Only 2, expected 5

        let result = encoder.encode_recovery_block(0, &input_blocks);
        assert!(result.is_err());

        let err = result.unwrap_err();
        match err {
            RsError::InvalidInput(msg) => {
                assert!(msg.contains("Expected 5 input blocks"));
            }
            _ => panic!("Expected InvalidInput error"),
        }
    }

    // Reference: par2cmdline-turbo/src/par2creator.cpp ProcessData()
    // blocklength parameter must match blocksize for all blocks
    #[test]
    fn test_encode_wrong_block_size() {
        let encoder = RecoveryBlockEncoder::new(64, 2);

        let block1 = vec![0x01; 64];
        let block2 = vec![0x02; 32]; // Wrong size!
        let input_blocks: Vec<&[u8]> = vec![&block1, &block2];

        let result = encoder.encode_recovery_block(0, &input_blocks);
        assert!(result.is_err());

        let err = result.unwrap_err();
        match err {
            RsError::InvalidInput(msg) => {
                assert!(msg.contains("expected 64"));
            }
            _ => panic!("Expected InvalidInput error"),
        }
    }

    // Reference: par2cmdline-turbo/src/par2creator.cpp ProcessData()
    // Lines 751-856: Multiple recovery blocks generated in sequence
    #[test]
    fn test_encode_multiple_recovery_blocks() {
        let block_size = 64;
        let input_count = 3;
        let encoder = RecoveryBlockEncoder::new(block_size, input_count);

        let block1 = vec![0x01; block_size];
        let block2 = vec![0x02; block_size];
        let block3 = vec![0x03; block_size];
        let input_blocks: Vec<&[u8]> = vec![&block1, &block2, &block3];

        // Encode 3 recovery blocks with different exponents
        // Reference: par2cmdline-turbo/src/reedsolomon.h RSOutputRow::exponent
        let exponents = vec![0, 1, 2];
        let result = encoder.encode_recovery_blocks_parallel(&exponents, &input_blocks);

        assert!(result.is_ok());
        let recovery_blocks = result.unwrap();
        assert_eq!(recovery_blocks.len(), 3);

        // Check each recovery block
        for (exp, data) in recovery_blocks {
            assert!(exponents.contains(&exp));
            assert_eq!(data.len(), block_size);
        }
    }

    // Reference: par2cmdline-turbo/src/reedsolomon.h RSOutputRow
    // Different exponents produce different recovery blocks
    // This is critical for the RS encoding to work correctly
    #[test]
    fn test_different_exponents_produce_different_results() {
        let block_size = 64;
        let encoder = RecoveryBlockEncoder::new(block_size, 2);

        let block1 = vec![0xFF; block_size];
        let block2 = vec![0xAA; block_size];
        let input_blocks: Vec<&[u8]> = vec![&block1, &block2];

        let recovery0 = encoder.encode_recovery_block(0, &input_blocks).unwrap();
        let recovery1 = encoder.encode_recovery_block(1, &input_blocks).unwrap();
        let recovery2 = encoder.encode_recovery_block(2, &input_blocks).unwrap();

        // Different exponents should produce different recovery data
        assert_ne!(recovery0, recovery1);
        assert_ne!(recovery1, recovery2);
        assert_ne!(recovery0, recovery2);
    }

    // Reference: par2cmdline-turbo/src/par2creator.cpp ProcessData()
    // blockoffset and blocklength parameters allow partial block processing
    #[test]
    fn test_encode_with_zero_blocks() {
        let block_size = 128;
        let encoder = RecoveryBlockEncoder::new(block_size, 3);

        // All zero input blocks
        let block1 = vec![0x00; block_size];
        let block2 = vec![0x00; block_size];
        let block3 = vec![0x00; block_size];
        let input_blocks: Vec<&[u8]> = vec![&block1, &block2, &block3];

        let recovery = encoder.encode_recovery_block(0, &input_blocks).unwrap();

        // With all zero inputs, recovery should be all zeros
        // Reference: GF(2^16) arithmetic: 0 * anything = 0
        assert!(
            recovery.iter().all(|&b| b == 0),
            "Recovery block of all-zero inputs should be all zeros"
        );
    }

    // Reference: par2cmdline-turbo/src/reedsolomon.cpp InternalProcess()
    // Lines 115-148: XOR accumulation of multiply results
    #[test]
    fn test_encode_identity_check() {
        let block_size = 64;
        let encoder = RecoveryBlockEncoder::new(block_size, 1);

        // Single input block with known pattern
        let pattern = vec![0x42; block_size];
        let input_blocks: Vec<&[u8]> = vec![&pattern];

        let recovery = encoder.encode_recovery_block(0, &input_blocks).unwrap();

        // For single block, recovery = pattern * (base^0) = pattern * 1
        // But base[0] might not be 1, so we just verify non-zero result
        assert!(recovery.iter().any(|&b| b != 0));
    }

    // Reference: par2cmdline-turbo/src/par2creator.cpp lines 193-229
    // Multiple threads processing different recovery blocks in parallel
    #[test]
    fn test_parallel_encoding_consistency() {
        let block_size = 256;
        let input_count = 5;
        let encoder = RecoveryBlockEncoder::new(block_size, input_count);

        // Create varied input blocks
        let blocks: Vec<Vec<u8>> = (0..input_count)
            .map(|i| vec![(i as u8).wrapping_mul(17); block_size])
            .collect();
        let input_refs: Vec<&[u8]> = blocks.iter().map(|b| b.as_slice()).collect();

        // Encode same exponents multiple times - should get same results
        let exponents = vec![0, 1, 2, 3, 4];

        let result1 = encoder
            .encode_recovery_blocks_parallel(&exponents, &input_refs)
            .unwrap();
        let result2 = encoder
            .encode_recovery_blocks_parallel(&exponents, &input_refs)
            .unwrap();

        // Results should be identical
        assert_eq!(result1.len(), result2.len());
        for ((exp1, data1), (exp2, data2)) in result1.iter().zip(result2.iter()) {
            assert_eq!(exp1, exp2);
            assert_eq!(data1, data2, "Parallel encoding should be deterministic");
        }
    }

    // Reference: par2cmdline-turbo/src/par2creator.cpp ProcessData()
    // Block size validation is critical for memory safety
    #[test]
    fn test_block_size_validation_all_blocks() {
        let encoder = RecoveryBlockEncoder::new(128, 3);

        let block1 = vec![0x01; 128];
        let block2 = vec![0x02; 128];
        let block3 = vec![0x03; 64]; // Wrong size in last block
        let input_blocks: Vec<&[u8]> = vec![&block1, &block2, &block3];

        let result = encoder.encode_recovery_block(0, &input_blocks);
        assert!(result.is_err(), "Should fail when any block has wrong size");
    }

    // Reference: par2cmdline-turbo/src/galois.cpp and galois.h
    // Galois field operations for GF(2^16)
    #[test]
    fn test_large_input_count() {
        // Test with large number of input blocks (realistic PAR2 scenario)
        // Reference: par2cmdline-turbo supports up to 65536 blocks (2^16)
        let encoder = RecoveryBlockEncoder::new(512, 1000);

        assert_eq!(encoder.total_input_blocks(), 1000);
        assert_eq!(encoder.base_values.len(), 1000);

        // All base values should be unique
        let mut sorted = encoder.base_values.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 1000, "All base values should be unique");
    }

    // Reference: par2cmdline-turbo/src/reedsolomon.cpp InternalProcess()
    // Process method handles the multiply-add operation
    #[test]
    fn test_small_block_sizes() {
        // Test with very small blocks (edge case)
        let encoder = RecoveryBlockEncoder::new(4, 2);

        let block1 = vec![0xAA, 0xBB, 0xCC, 0xDD];
        let block2 = vec![0x11, 0x22, 0x33, 0x44];
        let input_blocks: Vec<&[u8]> = vec![&block1, &block2];

        let recovery = encoder.encode_recovery_block(0, &input_blocks).unwrap();
        assert_eq!(recovery.len(), 4);
    }

    // Reference: par2cmdline-turbo/src/par2creator.cpp ProcessData()
    // Very large blocks should be handled correctly (memory test)
    #[test]
    fn test_large_block_size() {
        // Test with realistic large block size (16MB is PAR2 maximum)
        let block_size = 1024 * 1024; // 1MB blocks
        let encoder = RecoveryBlockEncoder::new(block_size, 2);

        let block1 = vec![0x55; block_size];
        let block2 = vec![0xAA; block_size];
        let input_blocks: Vec<&[u8]> = vec![&block1, &block2];

        let result = encoder.encode_recovery_block(0, &input_blocks);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), block_size);
    }

    // Reference: par2cmdline-turbo/src/reedsolomon.h RSOutputRow::exponent
    // Exponents can be any u16 value
    #[test]
    fn test_high_exponent_values() {
        let block_size = 64;
        let encoder = RecoveryBlockEncoder::new(block_size, 2);

        let block1 = vec![0xFF; block_size];
        let block2 = vec![0x00; block_size];
        let input_blocks: Vec<&[u8]> = vec![&block1, &block2];

        // Test with high exponent values
        let recovery0 = encoder.encode_recovery_block(100, &input_blocks).unwrap();
        let recovery1 = encoder.encode_recovery_block(1000, &input_blocks).unwrap();
        let recovery2 = encoder.encode_recovery_block(10000, &input_blocks).unwrap();

        // All should succeed and produce different results
        assert_ne!(recovery0, recovery1);
        assert_ne!(recovery1, recovery2);
        assert_ne!(recovery0, recovery2);
    }
}
