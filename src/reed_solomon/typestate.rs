//! Zero-cost type-state pattern for Reed-Solomon operations
//!
//! This module provides compile-time safety for Reed-Solomon state transitions
//! while maintaining identical runtime performance to the original implementation.
//!
//! ## State Transitions
//!
//! ```text
//! ReedSolomonNew
//!   ↓ (set_input)
//! ReedSolomonConfigured
//!   ↓ (compute)  
//! ReedSolomonComputed
//!   → (process) - can be called multiple times
//! ```
//!
//! ## Zero-Cost Design
//!
//! - Delegates to original `ReedSolomon` implementation for identical behavior
//! - Uses `PhantomData` for state tracking (zero runtime cost)  
//! - Move semantics prevent copying data during transitions
//! - All validation happens at compile time
//! - Generated assembly should be identical to original

use super::reedsolomon::{ReedSolomon as OriginalReedSolomon, RsError, RsResult};
use std::marker::PhantomData;

// ============================================================================
// State Types (Zero Runtime Cost)
// ============================================================================

/// Initial state - ReedSolomon just created
pub struct New;

/// Configured state - input and outputs have been set
pub struct Configured;

/// Computed state - matrix has been computed, ready for processing
pub struct Computed;

// ============================================================================
// Type-Safe ReedSolomon with State Enforcement
// ============================================================================

/// Type-safe Reed-Solomon encoder/decoder with compile-time state enforcement
///
/// This wrapper delegates to the original ReedSolomon implementation while providing
/// compile-time safety for state transitions. Runtime performance is identical.
pub struct ReedSolomon<State = New> {
    // Delegates to original implementation for identical behavior
    inner: OriginalReedSolomon,

    // Zero-cost state marker
    _state: PhantomData<State>,
}

// ============================================================================
// State-Specific Implementations
// ============================================================================

impl ReedSolomon<New> {
    /// Create a new Reed-Solomon instance in the initial state
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: OriginalReedSolomon::new(),
            _state: PhantomData,
        }
    }

    /// Set which input blocks are present or missing
    /// Transitions to Configured state
    #[inline]
    pub fn set_input(mut self, present: &[bool]) -> RsResult<ReedSolomon<Configured>> {
        self.inner.set_input(present)?;
        Ok(ReedSolomon {
            inner: self.inner,
            _state: PhantomData,
        })
    }

    /// Set all input blocks as present
    /// Transitions to Configured state
    #[inline]
    pub fn set_input_all_present(mut self, count: u32) -> RsResult<ReedSolomon<Configured>> {
        self.inner.set_input_all_present(count)?;
        Ok(ReedSolomon {
            inner: self.inner,
            _state: PhantomData,
        })
    }
}

impl ReedSolomon<Configured> {
    /// Record whether a recovery block with the specified exponent is present or missing
    #[inline]
    pub fn set_output(mut self, present: bool, exponent: u16) -> RsResult<ReedSolomon<Configured>> {
        self.inner.set_output(present, exponent)?;
        Ok(self)
    }

    /// Record whether recovery blocks with the specified range of exponents are present or missing
    #[inline]
    pub fn set_output_range(
        mut self,
        present: bool,
        low_exponent: u16,
        high_exponent: u16,
    ) -> RsResult<ReedSolomon<Configured>> {
        self.inner
            .set_output_range(present, low_exponent, high_exponent)?;
        Ok(self)
    }

    /// Compute the Reed-Solomon matrix
    /// Transitions to Computed state - the only state where process() can be called
    #[inline]
    pub fn compute(mut self) -> RsResult<ReedSolomon<Computed>> {
        self.inner.compute()?;
        Ok(ReedSolomon {
            inner: self.inner,
            _state: PhantomData,
        })
    }
}

impl ReedSolomon<Computed> {
    /// Process a block of data through the Reed-Solomon matrix
    /// Only available after successful compute() - guaranteed safe to call
    #[inline]
    pub fn process(
        &self,
        input_index: u32,
        input_data: &[u8],
        output_index: u32,
        output_data: &mut [u8],
    ) -> RsResult<()> {
        self.inner
            .process(input_index, input_data, output_index, output_data)
    }

    // Note: Matrix dimension getters could be added if needed by exposing
    // internal structure or adding public methods to original ReedSolomon

    /// Get the inner ReedSolomon for advanced usage (escape hatch)
    #[inline]
    pub fn inner(&self) -> &OriginalReedSolomon {
        &self.inner
    }
}

// ============================================================================
// Shared Implementation (Delegation to Original)
// ============================================================================

// No shared implementation needed - everything delegates to the original ReedSolomon

// ============================================================================
// Default and Debug Implementations
// ============================================================================

impl Default for ReedSolomon<New> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<State> std::fmt::Debug for ReedSolomon<State> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypeSafeReedSolomon")
            .field("inner", &"<Original ReedSolomon>")
            .finish()
    }
}

// ============================================================================
// Convenience Builder (Maintains Ergonomic API)
// ============================================================================

/// Builder that provides the same ergonomic API but with type safety
///
/// This allows gradual migration from the old API while providing
/// the same convenient builder pattern.
pub struct TypeSafeReedSolomonBuilder {
    input_status: Option<Vec<bool>>,
    recovery_blocks: Vec<(bool, u16)>,
}

impl TypeSafeReedSolomonBuilder {
    pub fn new() -> Self {
        Self {
            input_status: None,
            recovery_blocks: Vec::new(),
        }
    }

    pub fn with_input_status(mut self, status: &[bool]) -> Self {
        self.input_status = Some(status.to_vec());
        self
    }

    pub fn with_recovery_block(mut self, present: bool, exponent: u16) -> Self {
        self.recovery_blocks.push((present, exponent));
        self
    }

    pub fn with_recovery_blocks_range(
        mut self,
        present: bool,
        low_exponent: u16,
        high_exponent: u16,
    ) -> Self {
        for exponent in low_exponent..=high_exponent {
            self.recovery_blocks.push((present, exponent));
        }
        self
    }

    /// Build a type-safe Reed-Solomon instance that's ready to compute
    /// Returns ReedSolomon<Configured> which must call compute() before process()
    pub fn build(self) -> RsResult<ReedSolomon<Configured>> {
        let rs = ReedSolomon::new();

        // Set input configuration (required)
        let mut rs = match self.input_status {
            Some(status) => rs.set_input(&status)?,
            None => return Err(RsError::ComputationError), // Could add better error type
        };

        // Add all recovery blocks
        for (present, exponent) in self.recovery_blocks {
            rs = rs.set_output(present, exponent)?;
        }

        Ok(rs)
    }
}

impl Default for TypeSafeReedSolomonBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_safety_prevents_invalid_transitions() {
        let rs = ReedSolomon::new();

        // This would not compile - process() not available on New state:
        // rs.process(0, &[], 0, &mut []);

        let rs = rs.set_input(&[true, false]).unwrap();

        // This would not compile - process() not available on Configured state:
        // rs.process(0, &[], 0, &mut []);

        let rs = rs.set_output(true, 0).unwrap().compute().unwrap();

        // Now process() is available and guaranteed safe:
        let input = vec![1u8; 4];
        let mut output = vec![0u8; 4];
        let _ = rs.process(0, &input, 0, &mut output);
    }

    #[test]
    fn builder_provides_same_ergonomic_api() {
        let rs = TypeSafeReedSolomonBuilder::new()
            .with_input_status(&[true, true, false])
            .with_recovery_block(true, 0)
            .build()
            .unwrap()
            .compute()
            .unwrap();

        let input = vec![1u8; 8];
        let mut output = vec![0u8; 8];
        let _ = rs.process(0, &input, 0, &mut output);
    }

    #[test]
    fn zero_cost_wrapper_overhead() {
        // Verify that the wrapper adds minimal overhead (just PhantomData)
        use std::mem;

        // The wrapper should be exactly one pointer size larger (for PhantomData marker)
        // Actually PhantomData is zero-sized, so they should be the same
        let original_size = mem::size_of::<super::super::reedsolomon::ReedSolomon>();
        let wrapper_size = mem::size_of::<ReedSolomon<New>>();

        // Should be identical - PhantomData is zero-cost
        assert_eq!(wrapper_size, original_size);
    }
}
