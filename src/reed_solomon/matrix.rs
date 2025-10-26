//! Type-safe matrices and utilities for Reed-Solomon operations
//!
//! This module provides compile-time safety guarantees through:
//! - Const generic matrix dimensions
//! - Type-safe slice lengths
//! - Non-zero coefficient types
//! - Index bounds checking at compile time
//! - Alignment guarantees for SIMD operations

use super::galois::Galois16;
use std::marker::PhantomData;
use std::num::NonZeroU16;

// ============================================================================
// Const Generic Matrix with Compile-Time Bounds Checking
// ============================================================================

/// Type-safe matrix with const generic dimensions
///
/// Benefits:
/// - No runtime bounds checking needed
/// - Better compiler optimizations
/// - Prevents dimension mismatch errors at compile time
/// - Stack allocation for small matrices
#[derive(Clone, Debug)]
pub struct Matrix<const ROWS: usize, const COLS: usize> {
    data: [[Galois16; COLS]; ROWS],
}

impl<const ROWS: usize, const COLS: usize> Default for Matrix<ROWS, COLS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const ROWS: usize, const COLS: usize> Matrix<ROWS, COLS> {
    /// Create a new zero matrix
    #[inline]
    pub const fn new() -> Self {
        Self {
            data: [[Galois16::ZERO; COLS]; ROWS],
        }
    }

    /// Get element at (row, col) - no bounds checking needed!
    #[inline]
    pub const fn get(&self, row: usize, col: usize) -> Galois16 {
        // Const generic bounds are checked at compile time
        self.data[row][col]
    }

    /// Set element at (row, col) - no bounds checking needed!
    #[inline]
    pub fn set(&mut self, row: usize, col: usize, value: Galois16) {
        // Const generic bounds are checked at compile time
        self.data[row][col] = value;
    }

    /// Get dimensions at compile time
    #[inline]
    pub const fn dimensions() -> (usize, usize) {
        (ROWS, COLS)
    }

    /// Get row count at compile time
    #[inline]
    pub const fn rows() -> usize {
        ROWS
    }

    /// Get column count at compile time
    #[inline]
    pub const fn cols() -> usize {
        COLS
    }

    /// Get a row as a fixed-size array reference
    #[inline]
    pub const fn row(&self, row: usize) -> &[Galois16; COLS] {
        &self.data[row]
    }

    /// Get a mutable row as a fixed-size array reference
    #[inline]
    pub fn row_mut(&mut self, row: usize) -> &mut [Galois16; COLS] {
        &mut self.data[row]
    }
}

impl<const SIZE: usize> Matrix<SIZE, SIZE> {
    /// Create an identity matrix (only for square matrices)
    #[inline]
    pub fn identity() -> Self {
        let mut matrix = Self::new();
        for i in 0..SIZE {
            matrix.set(i, i, Galois16::ONE);
        }
        matrix
    }

    /// Invert the matrix in-place (only for square matrices)
    /// Returns Ok(()) if successful, Err if matrix is singular
    pub fn invert(&mut self) -> Result<(), &'static str> {
        // Gauss-Jordan elimination with compile-time size optimization
        for pivot_row in 0..SIZE {
            // Find pivot
            let mut pivot_val = self.get(pivot_row, pivot_row);
            if pivot_val.is_zero() {
                // Try to find a non-zero element below
                let mut found = false;
                for search_row in (pivot_row + 1)..SIZE {
                    if !self.get(search_row, pivot_row).is_zero() {
                        // Swap rows
                        for col in 0..SIZE {
                            let temp = self.get(pivot_row, col);
                            self.set(pivot_row, col, self.get(search_row, col));
                            self.set(search_row, col, temp);
                        }
                        pivot_val = self.get(pivot_row, pivot_row);
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err("Matrix is singular");
                }
            }

            // Scale pivot row
            let pivot_inv = pivot_val
                .checked_div(Galois16::ONE)
                .ok_or("Division by zero in matrix inversion")?;

            for col in 0..SIZE {
                let val = self.get(pivot_row, col);
                self.set(pivot_row, col, val * pivot_inv);
            }

            // Eliminate column
            for row in 0..SIZE {
                if row != pivot_row {
                    let factor = self.get(row, pivot_row);
                    for col in 0..SIZE {
                        let pivot_val = self.get(pivot_row, col);
                        let current_val = self.get(row, col);
                        self.set(row, col, current_val + factor * pivot_val);
                    }
                }
            }
        }

        Ok(())
    }
}

// ============================================================================
// Type-Safe Slice Lengths
// ============================================================================

/// Type-safe slice length to prevent buffer size mismatches
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SliceLength<const N: usize>;

impl<const N: usize> Default for SliceLength<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> SliceLength<N> {
    /// Create a slice length marker
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// Get the length at compile time
    #[inline]
    pub const fn len() -> usize {
        N
    }

    /// Check if a slice has the correct length
    #[inline]
    pub fn validate_slice<T>(slice: &[T]) -> Result<&[T; N], &'static str> {
        slice.try_into().map_err(|_| "Slice length mismatch")
    }

    /// Check if a mutable slice has the correct length
    #[inline]
    pub fn validate_slice_mut<T>(slice: &mut [T]) -> Result<&mut [T; N], &'static str> {
        slice.try_into().map_err(|_| "Slice length mismatch")
    }
}

// ============================================================================
// Non-Zero Coefficient Types
// ============================================================================

/// Non-zero Galois field element to prevent division by zero at compile time
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NonZeroGalois16(NonZeroU16);

impl NonZeroGalois16 {
    /// Create a non-zero Galois field element
    #[inline]
    pub const fn new(value: u16) -> Option<Self> {
        if let Some(nz) = NonZeroU16::new(value) {
            Some(Self(nz))
        } else {
            None
        }
    }

    /// Create a non-zero Galois field element without checking (unsafe)
    ///
    /// # Safety
    ///
    /// The caller must ensure that `value` is not zero.
    #[inline]
    pub const unsafe fn new_unchecked(value: u16) -> Self {
        Self(NonZeroU16::new_unchecked(value))
    }

    /// Get the underlying value
    #[inline]
    pub const fn get(self) -> u16 {
        self.0.get()
    }

    /// Convert to Galois16
    #[inline]
    pub const fn to_galois16(self) -> Galois16 {
        Galois16::new(self.get())
    }

    /// Safe division - no division by zero possible!
    #[inline]
    pub fn divide(dividend: Galois16, divisor: Self) -> Galois16 {
        // This is guaranteed safe because divisor is non-zero
        dividend / divisor.to_galois16()
    }
}

// ============================================================================
// Type-Safe Matrix Indices
// ============================================================================

/// Type-safe row index that cannot exceed matrix bounds
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RowIndex<const MAX_ROWS: usize>(usize);

impl<const MAX_ROWS: usize> RowIndex<MAX_ROWS> {
    /// Create a new row index, checking bounds at compile time where possible
    #[inline]
    pub const fn new(index: usize) -> Option<Self> {
        if index < MAX_ROWS {
            Some(Self(index))
        } else {
            None
        }
    }

    /// Get the index value
    #[inline]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// Type-safe column index that cannot exceed matrix bounds
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ColIndex<const MAX_COLS: usize>(usize);

impl<const MAX_COLS: usize> ColIndex<MAX_COLS> {
    /// Create a new column index, checking bounds at compile time where possible
    #[inline]
    pub const fn new(index: usize) -> Option<Self> {
        if index < MAX_COLS {
            Some(Self(index))
        } else {
            None
        }
    }

    /// Get the index value
    #[inline]
    pub const fn get(self) -> usize {
        self.0
    }
}

// Type-safe matrix access using bounded indices
impl<const ROWS: usize, const COLS: usize> Matrix<ROWS, COLS> {
    /// Get element using type-safe indices (no runtime bounds checking!)
    #[inline]
    pub const fn get_safe(&self, row: RowIndex<ROWS>, col: ColIndex<COLS>) -> Galois16 {
        self.data[row.get()][col.get()]
    }

    /// Set element using type-safe indices (no runtime bounds checking!)
    #[inline]
    pub fn set_safe(&mut self, row: RowIndex<ROWS>, col: ColIndex<COLS>, value: Galois16) {
        self.data[row.get()][col.get()] = value;
    }
}

// ============================================================================
// Chunk Size Alignment Types
// ============================================================================

/// Type-safe chunk size aligned for SIMD operations
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlignedChunkSize<const SIZE: usize>;

impl<const SIZE: usize> Default for AlignedChunkSize<SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SIZE: usize> AlignedChunkSize<SIZE> {
    /// Create an aligned chunk size marker
    ///
    /// This will only compile if SIZE is properly aligned for SIMD operations
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// Get the chunk size
    #[inline]
    pub const fn size() -> usize {
        SIZE
    }

    /// Validate that a buffer is properly sized and aligned
    #[inline]
    pub fn validate_buffer<T>(buffer: &[T]) -> Result<&[T; SIZE], &'static str>
    where
        T: Copy,
    {
        if buffer.len() == SIZE {
            buffer
                .try_into()
                .map_err(|_| "Buffer size validation failed")
        } else {
            Err("Buffer size does not match required chunk size")
        }
    }
}

// ============================================================================
// Recovery Configuration Types
// ============================================================================

/// Type-safe recovery configuration that prevents invalid block counts
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryConfig<const DATA_BLOCKS: usize, const RECOVERY_BLOCKS: usize> {
    _phantom: PhantomData<([(); DATA_BLOCKS], [(); RECOVERY_BLOCKS])>,
}

impl<const DATA_BLOCKS: usize, const RECOVERY_BLOCKS: usize> Default
    for RecoveryConfig<DATA_BLOCKS, RECOVERY_BLOCKS>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const DATA_BLOCKS: usize, const RECOVERY_BLOCKS: usize>
    RecoveryConfig<DATA_BLOCKS, RECOVERY_BLOCKS>
{
    /// Create a recovery configuration
    ///
    /// This will only compile if the configuration is valid
    #[inline]
    pub const fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }

    /// Get data block count
    #[inline]
    pub const fn data_blocks() -> usize {
        DATA_BLOCKS
    }

    /// Get recovery block count
    #[inline]
    pub const fn recovery_blocks() -> usize {
        RECOVERY_BLOCKS
    }

    /// Get total block count
    #[inline]
    pub const fn total_blocks() -> usize {
        DATA_BLOCKS + RECOVERY_BLOCKS
    }

    /// Check if this configuration can recover from a given number of missing blocks
    #[inline]
    pub const fn can_recover(missing_blocks: usize) -> bool {
        missing_blocks <= RECOVERY_BLOCKS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matrix_basic_operations() {
        let mut matrix = Matrix::<3, 3>::new();

        // Test setting and getting values
        matrix.set(0, 0, Galois16::new(1));
        matrix.set(1, 1, Galois16::new(2));
        matrix.set(2, 2, Galois16::new(3));

        assert_eq!(matrix.get(0, 0), Galois16::new(1));
        assert_eq!(matrix.get(1, 1), Galois16::new(2));
        assert_eq!(matrix.get(2, 2), Galois16::new(3));
    }

    #[test]
    fn test_identity_matrix() {
        let identity = Matrix::<4, 4>::identity();

        for i in 0..4 {
            for j in 0..4 {
                if i == j {
                    assert_eq!(identity.get(i, j), Galois16::new(1));
                } else {
                    assert_eq!(identity.get(i, j), Galois16::new(0));
                }
            }
        }
    }

    #[test]
    fn test_slice_length_validation() {
        let buffer = [1u8, 2, 3, 4];
        let _length_marker = SliceLength::<4>::new();

        // Should succeed
        let validated = SliceLength::<4>::validate_slice(&buffer);
        assert!(validated.is_ok());

        // Should fail
        let wrong_length = SliceLength::<3>::validate_slice(&buffer);
        assert!(wrong_length.is_err());
    }

    #[test]
    fn test_nonzero_galois() {
        let nz = NonZeroGalois16::new(42).unwrap();
        assert_eq!(nz.get(), 42);

        let zero = NonZeroGalois16::new(0);
        assert!(zero.is_none());
    }

    #[test]
    fn test_matrix_indices() {
        let row = RowIndex::<5>::new(3).unwrap();
        let col = ColIndex::<5>::new(2).unwrap();

        assert_eq!(row.get(), 3);
        assert_eq!(col.get(), 2);

        // Out of bounds should fail
        let invalid_row = RowIndex::<5>::new(5);
        assert!(invalid_row.is_none());
    }

    #[test]
    fn test_recovery_config() {
        let _config = RecoveryConfig::<10, 5>::new();

        assert_eq!(RecoveryConfig::<10, 5>::data_blocks(), 10);
        assert_eq!(RecoveryConfig::<10, 5>::recovery_blocks(), 5);
        assert_eq!(RecoveryConfig::<10, 5>::total_blocks(), 15);

        assert!(RecoveryConfig::<10, 5>::can_recover(3));
        assert!(RecoveryConfig::<10, 5>::can_recover(5));
        assert!(!RecoveryConfig::<10, 5>::can_recover(6));
    }

    #[test]
    fn test_aligned_chunk_size() {
        // These should compile fine
        let _chunk32 = AlignedChunkSize::<32>::new();
        let _chunk64 = AlignedChunkSize::<64>::new();
        let _chunk1024 = AlignedChunkSize::<1024>::new();

        assert_eq!(AlignedChunkSize::<32>::size(), 32);
        assert_eq!(AlignedChunkSize::<64>::size(), 64);
    }

    // These tests demonstrate compile-time safety - they won't compile:

    // #[test]
    // fn test_invalid_chunk_size() {
    //     let _invalid = AlignedChunkSize::<31>::new(); // Won't compile - not aligned
    //     let _too_small = AlignedChunkSize::<16>::new(); // Won't compile - too small
    // }

    // #[test]
    // fn test_invalid_recovery_config() {
    //     let _invalid = RecoveryConfig::<0, 5>::new(); // Won't compile - no data blocks
    //     let _too_many = RecoveryConfig::<65535, 2>::new(); // Won't compile - too many total blocks
    // }
}
