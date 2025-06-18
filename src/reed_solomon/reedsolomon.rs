//! Reed-Solomon implementation ported from par2cmdline
//!
//! This module provides the ReedSolomon struct and methods for PAR2-compatible
//! Reed-Solomon encoding and decoding operations.

use crate::reed_solomon::galois::{gcd, Galois16};
use crate::RecoverySlicePacket;
use std::collections::HashMap;

/// Output row specification for Reed-Solomon matrix
#[derive(Debug, Clone)]
pub struct RsOutputRow {
    pub present: bool,
    pub exponent: u16,
}

impl RsOutputRow {
    pub fn new(present: bool, exponent: u16) -> Self {
        Self { present, exponent }
    }
}

/// Result type for Reed-Solomon operations
pub type RsResult<T> = Result<T, RsError>;

/// Errors that can occur during Reed-Solomon operations
#[derive(Debug, Clone)]
pub enum RsError {
    TooManyInputBlocks,
    NotEnoughRecoveryBlocks,
    NoOutputBlocks,
    ComputationError,
    InvalidMatrix,
}

impl std::fmt::Display for RsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RsError::TooManyInputBlocks => {
                write!(f, "Too many input blocks for Reed Solomon matrix")
            }
            RsError::NotEnoughRecoveryBlocks => write!(f, "Not enough recovery blocks"),
            RsError::NoOutputBlocks => write!(f, "No output blocks"),
            RsError::ComputationError => write!(f, "RS computation error"),
            RsError::InvalidMatrix => write!(f, "Invalid Reed-Solomon matrix"),
        }
    }
}

impl std::error::Error for RsError {}

/// Reed-Solomon encoder/decoder following par2cmdline approach
pub struct ReedSolomon {
    // Input tracking
    input_count: u32,
    data_present: u32,
    data_missing: u32,
    data_present_index: Vec<u32>,
    data_missing_index: Vec<u32>,
    database: Vec<u16>, // Base values for Vandermonde matrix

    // Output tracking
    output_count: u32,
    par_present: u32,
    par_missing: u32,
    output_rows: Vec<RsOutputRow>,

    // Matrix
    left_matrix: Vec<Galois16>,
}

impl ReedSolomon {
    pub fn new() -> Self {
        Self {
            input_count: 0,
            data_present: 0,
            data_missing: 0,
            data_present_index: Vec::new(),
            data_missing_index: Vec::new(),
            database: Vec::new(),
            output_count: 0,
            par_present: 0,
            par_missing: 0,
            output_rows: Vec::new(),
            left_matrix: Vec::new(),
        }
    }

    /// Set which input blocks are present or missing
    /// Following par2cmdline's SetInput logic for Galois16
    pub fn set_input(&mut self, present: &[bool]) -> RsResult<()> {
        self.input_count = present.len() as u32;

        self.data_present_index.clear();
        self.data_missing_index.clear();
        self.database.clear();

        self.data_present_index.reserve(present.len());
        self.data_missing_index.reserve(present.len());
        self.database.reserve(present.len());

        self.data_present = 0;
        self.data_missing = 0;

        let mut logbase = 0u32;

        for (index, &is_present) in present.iter().enumerate() {
            if is_present {
                self.data_present_index.push(index as u32);
                self.data_present += 1;
            } else {
                self.data_missing_index.push(index as u32);
                self.data_missing += 1;
            }

            // Determine the next useable base value.
            // Its log must be relatively prime to 65535 (following par2cmdline)
            while gcd(65535, logbase) != 1 {
                logbase += 1;
            }
            if logbase >= 65535 {
                return Err(RsError::TooManyInputBlocks);
            }

            // Use ALog to get the base value (following par2cmdline)
            let base = Galois16::new(logbase as u16).alog();
            self.database.push(base);
            logbase += 1;
        }

        Ok(())
    }

    /// Set all input blocks as present
    pub fn set_input_all_present(&mut self, count: u32) -> RsResult<()> {
        let present: Vec<bool> = vec![true; count as usize];
        self.set_input(&present)
    }

    /// Record whether a recovery block with the specified exponent is present or missing
    pub fn set_output(&mut self, present: bool, exponent: u16) -> RsResult<()> {
        self.output_rows.push(RsOutputRow::new(present, exponent));
        self.output_count += 1;

        if present {
            self.par_present += 1;
        } else {
            self.par_missing += 1;
        }

        Ok(())
    }

    /// Record whether recovery blocks with the specified range of exponents are present or missing
    pub fn set_output_range(
        &mut self,
        present: bool,
        low_exponent: u16,
        high_exponent: u16,
    ) -> RsResult<()> {
        for exponent in low_exponent..=high_exponent {
            self.set_output(present, exponent)?;
        }
        Ok(())
    }

    /// Compute the Reed-Solomon matrix (following par2cmdline approach)
    pub fn compute(&mut self) -> RsResult<()> {
        let out_count = self.data_missing + self.par_missing;
        let in_count = self.data_present + self.data_missing;

        if self.data_missing > self.par_present {
            return Err(RsError::NotEnoughRecoveryBlocks);
        } else if out_count == 0 {
            return Err(RsError::NoOutputBlocks);
        }

        // Allocate the left matrix
        let matrix_size = (out_count * in_count) as usize;
        self.left_matrix = vec![Galois16::new(0); matrix_size];

        // Allocate right matrix for solving if needed
        let mut right_matrix = if self.data_missing > 0 {
            Some(vec![Galois16::new(0); (out_count * out_count) as usize])
        } else {
            None
        };

        // Build Vandermonde matrix following par2cmdline logic
        self.build_matrix(out_count, in_count, right_matrix.as_mut())?;

        // Solve if recovering data
        if self.data_missing > 0 {
            if let Some(ref mut right_mat) = right_matrix {
                self.gauss_eliminate(out_count, in_count, right_mat)?;
            }
        }

        Ok(())
    }

    /// Process a block of data through the Reed-Solomon matrix
    pub fn process(
        &self,
        input_index: u32,
        input_data: &[u8],
        output_index: u32,
        output_data: &mut [u8],
    ) -> RsResult<()> {
        if input_data.len() != output_data.len() {
            return Err(RsError::ComputationError);
        }

        let in_count = self.data_present + self.data_missing;
        let factor_index = (output_index * in_count + input_index) as usize;

        if factor_index >= self.left_matrix.len() {
            return Err(RsError::ComputationError);
        }

        let factor = self.left_matrix[factor_index];

        // Skip if factor is zero
        if factor.value() == 0 {
            return Ok(());
        }

        // Process data using Galois field arithmetic
        for (i, &input_byte) in input_data.iter().enumerate() {
            let input_val = Galois16::new(input_byte as u16);
            let result = input_val * factor;
            let output_val = Galois16::new(output_data[i] as u16);
            let new_output = output_val + result;
            output_data[i] = new_output.value() as u8;
        }

        Ok(())
    }

    fn build_matrix(
        &mut self,
        out_count: u32,
        in_count: u32,
        mut right_matrix: Option<&mut Vec<Galois16>>,
    ) -> RsResult<()> {
        let mut output_row_iter = 0;

        // Build matrix for present recovery blocks used for missing data blocks
        for row in 0..self.data_missing {
            // Find next present recovery block
            while output_row_iter < self.output_rows.len()
                && !self.output_rows[output_row_iter].present
            {
                output_row_iter += 1;
            }

            if output_row_iter >= self.output_rows.len() {
                return Err(RsError::InvalidMatrix);
            }

            let exponent = self.output_rows[output_row_iter].exponent;

            // Fill columns for present data blocks
            for col in 0..self.data_present {
                let base_idx = self.data_present_index[col as usize] as usize;
                let base = Galois16::new(self.database[base_idx]);
                let factor = base.pow(exponent);

                let matrix_idx = (row * in_count + col) as usize;
                self.left_matrix[matrix_idx] = factor;
            }

            // Fill columns for missing data blocks (identity for this row)
            for col in 0..self.data_missing {
                let factor = if row == col {
                    Galois16::new(1)
                } else {
                    Galois16::new(0)
                };
                let matrix_idx = (row * in_count + col + self.data_present) as usize;
                self.left_matrix[matrix_idx] = factor;
            }

            // Fill right matrix if present
            if let Some(ref mut right_mat) = right_matrix {
                // One column for each missing data block
                for col in 0..self.data_missing {
                    let base_idx = self.data_missing_index[col as usize] as usize;
                    let base = Galois16::new(self.database[base_idx]);
                    let factor = base.pow(exponent);

                    let matrix_idx = (row * out_count + col) as usize;
                    right_mat[matrix_idx] = factor;
                }
                // One column for each missing recovery block
                for col in 0..self.par_missing {
                    let matrix_idx = (row * out_count + col + self.data_missing) as usize;
                    right_mat[matrix_idx] = Galois16::new(0);
                }
            }

            output_row_iter += 1;
        }

        // Build matrix for missing recovery blocks
        output_row_iter = 0;
        for row in 0..self.par_missing {
            // Find next missing recovery block
            while output_row_iter < self.output_rows.len()
                && self.output_rows[output_row_iter].present
            {
                output_row_iter += 1;
            }

            if output_row_iter >= self.output_rows.len() {
                return Err(RsError::InvalidMatrix);
            }

            let exponent = self.output_rows[output_row_iter].exponent;

            // Fill columns for present data blocks
            for col in 0..self.data_present {
                let base_idx = self.data_present_index[col as usize] as usize;
                let base = Galois16::new(self.database[base_idx]);
                let factor = base.pow(exponent);

                let matrix_idx = ((row + self.data_missing) * in_count + col) as usize;
                self.left_matrix[matrix_idx] = factor;
            }

            // Fill columns for missing data blocks
            for col in 0..self.data_missing {
                let matrix_idx =
                    ((row + self.data_missing) * in_count + col + self.data_present) as usize;
                self.left_matrix[matrix_idx] = Galois16::new(0);
            }

            // Fill right matrix if present
            if let Some(ref mut right_mat) = right_matrix {
                // One column for each missing data block
                for col in 0..self.data_missing {
                    let base_idx = self.data_missing_index[col as usize] as usize;
                    let base = Galois16::new(self.database[base_idx]);
                    let factor = base.pow(exponent);

                    let matrix_idx = ((row + self.data_missing) * out_count + col) as usize;
                    right_mat[matrix_idx] = factor;
                }
                // One column for each missing recovery block
                for col in 0..self.par_missing {
                    let factor = if row == col {
                        Galois16::new(1)
                    } else {
                        Galois16::new(0)
                    };
                    let matrix_idx =
                        ((row + self.data_missing) * out_count + col + self.data_missing) as usize;
                    right_mat[matrix_idx] = factor;
                }
            }

            output_row_iter += 1;
        }

        Ok(())
    }

    fn gauss_eliminate(
        &mut self,
        rows: u32,
        cols: u32,
        right_matrix: &mut Vec<Galois16>,
    ) -> RsResult<()> {
        // Gaussian elimination following par2cmdline approach
        for row in 0..self.data_missing {
            let pivot_idx = (row * rows + row) as usize;
            if pivot_idx >= right_matrix.len() {
                return Err(RsError::InvalidMatrix);
            }

            let pivot = right_matrix[pivot_idx];
            if pivot.value() == 0 {
                return Err(RsError::ComputationError);
            }

            // Scale row to make pivot = 1
            if pivot.value() != 1 {
                for col in 0..cols {
                    let idx = (row * cols + col) as usize;
                    if idx < self.left_matrix.len() {
                        self.left_matrix[idx] = self.left_matrix[idx] / pivot;
                    }
                }
                right_matrix[pivot_idx] = Galois16::new(1);
                for col in (row + 1)..rows {
                    let idx = (row * rows + col) as usize;
                    if idx < right_matrix.len() {
                        right_matrix[idx] = right_matrix[idx] / pivot;
                    }
                }
            }

            // Eliminate other rows
            for other_row in 0..rows {
                if other_row != row {
                    let factor_idx = (other_row * rows + row) as usize;
                    if factor_idx < right_matrix.len() {
                        let factor = right_matrix[factor_idx];

                        if factor.value() != 0 {
                            for col in 0..cols {
                                let src_idx = (row * cols + col) as usize;
                                let dst_idx = (other_row * cols + col) as usize;

                                if src_idx < self.left_matrix.len()
                                    && dst_idx < self.left_matrix.len()
                                {
                                    let scaled = self.left_matrix[src_idx] * factor;
                                    self.left_matrix[dst_idx] = self.left_matrix[dst_idx] - scaled;
                                }
                            }

                            right_matrix[factor_idx] = Galois16::new(0);
                            for col in (row + 1)..rows {
                                let src_idx = (row * rows + col) as usize;
                                let dst_idx = (other_row * rows + col) as usize;

                                if src_idx < right_matrix.len() && dst_idx < right_matrix.len() {
                                    let scaled = right_matrix[src_idx] * factor;
                                    right_matrix[dst_idx] = right_matrix[dst_idx] - scaled;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
/// Result of Reed-Solomon reconstruction
#[derive(Debug)]
pub struct ReconstructionResult {
    pub success: bool,
    pub reconstructed_slices: HashMap<usize, Vec<u8>>,
    pub error_message: Option<String>,
}

/// Reconstruction engine for PAR2-compatible Reed-Solomon operations
/// This follows the par2cmdline approach more closely
pub struct ReconstructionEngine {
    slice_size: usize,
    total_input_slices: usize,
    recovery_slices: Vec<RecoverySlicePacket>,
}

impl ReconstructionEngine {
    pub fn new(
        slice_size: usize,
        total_input_slices: usize,
        recovery_slices: Vec<RecoverySlicePacket>,
    ) -> Self {
        Self {
            slice_size,
            total_input_slices,
            recovery_slices,
        }
    }

    /// Check if reconstruction is possible with the given number of missing slices
    pub fn can_reconstruct(&self, missing_count: usize) -> bool {
        self.recovery_slices.len() >= missing_count
    }

    /// Reconstruct missing slices using Reed-Solomon error correction
    /// Following par2cmdline approach with proper exponents
    pub fn reconstruct_missing_slices(
        &self,
        existing_slices: &HashMap<usize, Vec<u8>>,
        missing_slices: &[usize],
        _global_slice_map: &HashMap<usize, usize>,
    ) -> ReconstructionResult {
        if missing_slices.is_empty() {
            return ReconstructionResult {
                success: true,
                reconstructed_slices: HashMap::new(),
                error_message: None,
            };
        }

        if !self.can_reconstruct(missing_slices.len()) {
            return ReconstructionResult {
                success: false,
                reconstructed_slices: HashMap::new(),
                error_message: Some("Not enough recovery slices available".to_string()),
            };
        }

        let mut rs = ReedSolomon::new();

        // Set up input blocks (present/missing status)
        let mut present_status = vec![false; self.total_input_slices];
        for &slice_idx in existing_slices.keys() {
            if slice_idx < self.total_input_slices {
                present_status[slice_idx] = true;
            }
        }

        if let Err(e) = rs.set_input(&present_status) {
            return ReconstructionResult {
                success: false,
                reconstructed_slices: HashMap::new(),
                error_message: Some(format!("Failed to set input: {}", e)),
            };
        }

        // Set up recovery blocks using actual exponents from recovery slices
        // This is crucial - par2cmdline uses specific exponents, not sequential indices
        let recovery_count = missing_slices.len().min(self.recovery_slices.len());
        for i in 0..recovery_count {
            // Use the actual exponent from the recovery slice packet
            let exponent = self.recovery_slices[i].exponent as u16;
            if let Err(e) = rs.set_output(true, exponent) {
                return ReconstructionResult {
                    success: false,
                    reconstructed_slices: HashMap::new(),
                    error_message: Some(format!("Failed to set output: {}", e)),
                };
            }
        }

        // Set missing outputs (these don't need real exponents as they're being computed)
        for &slice_idx in missing_slices {
            if let Err(e) = rs.set_output(false, slice_idx as u16) {
                return ReconstructionResult {
                    success: false,
                    reconstructed_slices: HashMap::new(),
                    error_message: Some(format!("Failed to set missing output: {}", e)),
                };
            }
        }

        // Compute the Reed-Solomon matrix
        if let Err(e) = rs.compute() {
            return ReconstructionResult {
                success: false,
                reconstructed_slices: HashMap::new(),
                error_message: Some(format!("Failed to compute matrix: {}", e)),
            };
        }

        // Perform reconstruction
        let mut reconstructed_slices = HashMap::new();

        for &missing_slice_idx in missing_slices {
            let mut reconstructed_data = vec![0u8; self.slice_size];

            // Process each existing slice through the Reed-Solomon matrix
            for (&existing_slice_idx, existing_data) in existing_slices {
                if existing_slice_idx < self.total_input_slices {
                    if let Err(e) = rs.process(
                        existing_slice_idx as u32,
                        existing_data,
                        missing_slice_idx as u32,
                        &mut reconstructed_data,
                    ) {
                        return ReconstructionResult {
                            success: false,
                            reconstructed_slices: HashMap::new(),
                            error_message: Some(format!("Failed to process slice: {}", e)),
                        };
                    }
                }
            }

            // Process recovery slices with their data
            for (recovery_idx, recovery_slice) in
                self.recovery_slices.iter().enumerate().take(recovery_count)
            {
                if let Err(e) = rs.process(
                    (self.total_input_slices + recovery_idx) as u32,
                    &recovery_slice.recovery_data,
                    missing_slice_idx as u32,
                    &mut reconstructed_data,
                ) {
                    return ReconstructionResult {
                        success: false,
                        reconstructed_slices: HashMap::new(),
                        error_message: Some(format!("Failed to process recovery slice: {}", e)),
                    };
                }
            }

            reconstructed_slices.insert(missing_slice_idx, reconstructed_data);
        }

        ReconstructionResult {
            success: true,
            reconstructed_slices,
            error_message: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reed_solomon_basic() {
        let mut rs = ReedSolomon::new();

        // Set up 4 input blocks, all present
        rs.set_input_all_present(4).unwrap();

        // Set up 2 recovery blocks, to be computed
        rs.set_output(false, 0).unwrap();
        rs.set_output(false, 1).unwrap();

        // Compute the matrix
        rs.compute().unwrap();

        // Basic test passed if we get here without panicking
        assert_eq!(rs.data_present, 4);
        assert_eq!(rs.par_missing, 2);
    }

    #[test]
    fn test_gcd_function() {
        use crate::reed_solomon::galois::gcd;
        assert_eq!(gcd(48, 18), 6);
        assert_eq!(gcd(65535, 2), 1);
        assert_eq!(gcd(65535, 3), 3);
    }
}
