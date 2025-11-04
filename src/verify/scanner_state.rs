//! Scanner state management for file block scanning
//!
//! This module encapsulates the complex state needed for scanning files
//! with a sliding window buffer.

use crate::domain::Crc32Value;
use crate::verify::types::{BlockSize, BufferPosition, BufferSize, BytesProcessed, ScanPhase};

/// State for scanning a file with a sliding window buffer
#[derive(Debug)]
pub struct ScannerState {
    /// Current position within the buffer being scanned
    pub scan_pos: BufferPosition,
    /// Rolling CRC32 value (if available) for efficient scanning
    pub rolling_crc: Option<Crc32Value>,
    /// Number of valid bytes currently in the buffer
    pub bytes_in_buffer: BufferSize,
    /// Current scanning phase (first buffer vs subsequent buffers)
    pub scan_phase: ScanPhase,
    /// Total bytes processed through the file (for progress tracking)
    pub bytes_processed: BytesProcessed,
}

impl ScannerState {
    /// Create a new scanner state with initial buffer
    pub fn new(initial_bytes: usize) -> Self {
        Self {
            scan_pos: BufferPosition::zero(),
            rolling_crc: None,
            bytes_in_buffer: BufferSize::new(initial_bytes),
            scan_phase: ScanPhase::FirstBuffer,
            bytes_processed: BytesProcessed::zero(),
        }
    }

    /// Check if we're at end of file (no more data in buffer)
    pub fn at_eof(&self) -> bool {
        self.bytes_in_buffer.is_empty()
    }

    /// Check if we can fit a full block starting at current scan position
    pub fn can_fit_block(&self, block_size: BlockSize) -> bool {
        self.scan_pos
            .can_fit_block(self.bytes_in_buffer, block_size)
    }

    /// Check if we should try aligned block optimization
    pub fn should_try_aligned_blocks(&self, block_size: BlockSize) -> bool {
        self.scan_phase.is_first_buffer()
            && self.bytes_in_buffer.has_at_least_n_blocks(2, block_size)
    }

    /// Get the remainder size (bytes after scan_pos that don't fit a full block)
    pub fn remainder_size(&self, block_size: BlockSize) -> usize {
        let remainder = self.bytes_in_buffer.remainder_from(self.scan_pos);
        if remainder < block_size.as_usize() {
            remainder
        } else {
            0
        }
    }

    /// Check if we're scanning a remainder at the start of the buffer
    /// (This is used to avoid infinite loops when handling partial blocks)
    pub fn is_remainder_at_start(&self) -> bool {
        self.scan_pos == BufferPosition::zero()
    }

    /// Advance scan position by one byte (for rolling window)
    pub fn advance_one_byte(&mut self) {
        self.scan_pos.advance_by(1);
    }

    /// Skip ahead by a full block (after finding a match)
    pub fn skip_block(&mut self, block_size: BlockSize) {
        self.scan_pos.advance_by(block_size.as_usize());
    }

    /// Clear rolling CRC (when we can't maintain it)
    pub fn clear_rolling_crc(&mut self) {
        self.rolling_crc = None;
    }

    /// Set rolling CRC to specific value
    pub fn set_rolling_crc(&mut self, crc: Option<Crc32Value>) {
        self.rolling_crc = crc;
    }

    /// Reset scan position to start of buffer
    pub fn reset_scan_pos(&mut self) {
        self.scan_pos = BufferPosition::zero();
    }

    /// Update state after sliding the buffer window
    pub fn slide_window(&mut self, block_size: BlockSize, new_bytes_in_buffer: BufferSize) {
        self.scan_phase.mark_advanced();
        self.bytes_in_buffer = new_bytes_in_buffer;
        self.bytes_processed.advance_by(block_size);
        self.reset_scan_pos();
        self.clear_rolling_crc();
    }

    /// Check if we have enough data to slide the window
    pub fn can_slide_window(&self, block_size: BlockSize) -> bool {
        self.bytes_in_buffer.has_at_least(block_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scanner_state_creation() {
        let state = ScannerState::new(2048);
        assert_eq!(state.bytes_in_buffer.as_usize(), 2048);
        assert!(!state.at_eof());
        assert_eq!(state.scan_pos, BufferPosition::zero());
        assert!(state.scan_phase.is_first_buffer());
    }

    #[test]
    fn test_eof_detection() {
        let mut state = ScannerState::new(0);
        assert!(state.at_eof());

        state.bytes_in_buffer = BufferSize::new(100);
        assert!(!state.at_eof());
    }

    #[test]
    fn test_aligned_block_optimization_check() {
        let block_size = BlockSize::new(1024);

        // First buffer with enough data
        let mut state = ScannerState::new(2048);
        assert!(state.should_try_aligned_blocks(block_size));

        // Not first buffer
        state.scan_phase.mark_advanced();
        assert!(!state.should_try_aligned_blocks(block_size));

        // First buffer but not enough data
        let state2 = ScannerState::new(1500);
        assert!(!state2.should_try_aligned_blocks(block_size));
    }

    #[test]
    fn test_remainder_calculation() {
        let block_size = BlockSize::new(1024);
        let mut state = ScannerState::new(1500);

        // At start: 1500 bytes, can fit one block with 476 byte remainder
        assert_eq!(state.remainder_size(block_size), 0); // No remainder at scan_pos=0

        // Advance past one block
        state.scan_pos.advance_by(1024);
        assert_eq!(state.remainder_size(block_size), 476);
    }

    #[test]
    fn test_window_sliding() {
        let block_size = BlockSize::new(1024);
        let mut state = ScannerState::new(2048);

        assert!(state.can_slide_window(block_size));
        assert!(state.scan_phase.is_first_buffer());

        state.slide_window(block_size, BufferSize::new(1500));

        assert!(!state.scan_phase.is_first_buffer());
        assert_eq!(state.bytes_in_buffer.as_usize(), 1500);
        assert_eq!(state.scan_pos, BufferPosition::zero());
        assert!(state.rolling_crc.is_none());
    }

    #[test]
    fn test_scan_position_advancement() {
        let mut state = ScannerState::new(2048);
        let block_size = BlockSize::new(1024);

        assert_eq!(state.scan_pos.as_usize(), 0);

        state.advance_one_byte();
        assert_eq!(state.scan_pos.as_usize(), 1);

        state.skip_block(block_size);
        assert_eq!(state.scan_pos.as_usize(), 1025);
    }

    #[test]
    fn test_remainder_at_start_detection() {
        let state = ScannerState::new(500);
        assert!(state.is_remainder_at_start());

        let mut state2 = ScannerState::new(500);
        state2.scan_pos.advance_by(100);
        assert!(!state2.is_remainder_at_start());
    }
}
