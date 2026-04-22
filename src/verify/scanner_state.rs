//! Scanner state management for file block scanning
//!
//! This module encapsulates the complex state needed for scanning files
//! with a sliding window buffer.

use crate::domain::{Crc32Value, FileId};
use crate::verify::types::{
    BlockSize, BufferPosition, BufferSize, BytesProcessed, FileScanMetadata, ScanPhase,
};

/// State for scanning a file with a sliding window buffer
#[derive(Debug)]
pub struct ScannerState {
    /// Current position within the buffer being scanned
    pub buffer_position: BufferPosition,
    /// Rolling CRC32 value (if available) for efficient scanning
    pub rolling_crc: Option<Crc32Value>,
    /// Number of valid bytes currently in the buffer
    pub bytes_in_buffer: BufferSize,
    /// Current scanning phase (first buffer vs subsequent buffers)
    pub scan_phase: ScanPhase,
    /// Total bytes processed through the file (for progress tracking)
    pub bytes_processed: BytesProcessed,
    /// Metadata about blocks found during scanning (for validation)
    pub scan_metadata: FileScanMetadata,
}

impl ScannerState {
    /// Create a new scanner state with initial buffer
    pub fn new(initial_bytes: usize) -> Self {
        Self {
            buffer_position: BufferPosition::zero(),
            rolling_crc: None,
            bytes_in_buffer: BufferSize::new(initial_bytes),
            scan_phase: ScanPhase::FirstBuffer,
            bytes_processed: BytesProcessed::zero(),
            scan_metadata: FileScanMetadata::new(),
        }
    }

    /// Get current file offset (bytes processed + current buffer position)
    pub fn current_file_offset(&self) -> usize {
        self.bytes_processed.as_usize() + self.buffer_position.as_usize()
    }

    /// Record that a block was found at the current position
    pub fn record_block_found(&mut self, file_id: FileId, block_number: u32) {
        self.scan_metadata
            .record_block_found(self.current_file_offset(), file_id, block_number);
    }

    /// Check if we can fit a full block starting at current scan position
    pub fn can_fit_block(&self, block_size: BlockSize) -> bool {
        self.buffer_position
            .can_fit_block(self.bytes_in_buffer, block_size)
    }

    /// Check if we should try aligned block optimization
    /// Get the remainder size (bytes after scan_pos that don't fit a full block)
    pub fn remainder_size(&self, block_size: BlockSize) -> usize {
        let remainder = self.bytes_in_buffer.remainder_from(self.buffer_position);
        if remainder < block_size.as_usize() {
            remainder
        } else {
            0
        }
    }

    /// Check if we're scanning a remainder at the start of the buffer
    /// (This is used to avoid infinite loops when handling partial blocks)
    pub fn is_remainder_at_start(&self) -> bool {
        self.buffer_position == BufferPosition::zero()
    }

    /// Advance scan position by one byte (for rolling window)
    pub fn advance_one_byte(&mut self) {
        self.buffer_position.advance_by(1);
    }

    /// Advance scan position by a caller-specified byte count.
    pub fn advance_by(&mut self, bytes: usize) {
        self.buffer_position.advance_by(bytes);
    }

    /// Skip ahead by a full block (after finding a match)
    pub fn skip_block(&mut self, block_size: BlockSize) {
        self.buffer_position.advance_by(block_size.as_usize());
    }

    /// Clear rolling CRC (when we can't maintain it)
    pub fn clear_rolling_crc(&mut self) {
        self.rolling_crc = None;
    }

    /// Set rolling CRC to specific value
    pub fn set_rolling_crc(&mut self, crc: Option<Crc32Value>) {
        self.rolling_crc = crc;
    }

    /// Update state after sliding the buffer window
    /// Reference: par2cmdline-turbo/src/filechecksummer.cpp:110-163 (Jump function)
    /// When we slide, we keep blocksize bytes at the start of the buffer and discard earlier data
    /// The buffer_position stays relative to the new buffer start
    pub fn slide_window(&mut self, block_size: BlockSize, new_bytes_in_buffer: BufferSize) {
        self.scan_phase.mark_advanced();
        self.bytes_in_buffer = new_bytes_in_buffer;
        self.bytes_processed.advance_by(block_size);
        // Buffer position adjusts by -blocksize (we kept blocksize bytes, discarded earlier data)
        // So if we were at position 1500 and blocksize is 1024, we're now at position 476
        self.buffer_position = BufferPosition::new(
            self.buffer_position
                .as_usize()
                .saturating_sub(block_size.as_usize()),
        );
        self.clear_rolling_crc();
    }

    /// Check if we have enough data to slide the window
    pub fn can_slide_window(&self, block_size: BlockSize) -> bool {
        self.bytes_in_buffer.has_at_least(block_size)
    }

    /// Slide rolling CRC forward by one byte using rolling window algorithm
    pub fn slide_crc_one_byte(
        &mut self,
        rolling_table: &crate::checksum::rolling_crc::RollingCrcTable,
        buffer: &crate::verify::types::ScanBuffer,
        block_size: BlockSize,
    ) {
        use crate::domain::Crc32Value;

        if let Some(crc) = self.rolling_crc {
            let new_crc = rolling_table
                .slide_crc_forward(
                    crc.as_u32(),
                    buffer.as_slice(),
                    self.buffer_position.as_usize(),
                    block_size.as_usize(),
                    self.bytes_in_buffer.as_usize(),
                )
                .map(Crc32Value::new);
            self.set_rolling_crc(new_crc);
        }
    }

    /// Update CRC after skipping forward (recomputes from scratch at new position)
    /// Used after finding a block match - we skip ahead and need to recompute CRC
    /// Reference: par2cmdline-turbo/src/crc.cpp:119-122
    #[cfg(test)]
    pub fn update_crc_after_skip(
        &mut self,
        _rolling_table: &crate::checksum::rolling_crc::RollingCrcTable,
        buffer: &crate::verify::types::ScanBuffer,
        block_size: BlockSize,
    ) {
        use crate::checksum::compute_crc32;

        // Check if we can fit another block at current position
        if !self.can_fit_block(block_size) {
            self.clear_rolling_crc();
            return;
        }

        // Recompute CRC for the block at current position
        let start = self.buffer_position.as_usize();
        let end = start + block_size.as_usize();
        let crc = compute_crc32(buffer.slice(start..end));
        self.set_rolling_crc(Some(crc));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scanner_state_creation() {
        let state = ScannerState::new(2048);
        assert_eq!(state.bytes_in_buffer.as_usize(), 2048);
        assert_eq!(state.buffer_position, BufferPosition::zero());
        assert!(matches!(state.scan_phase, ScanPhase::FirstBuffer));
    }

    #[test]
    fn test_aligned_block_optimization_check() {
        let block_size = BlockSize::new(1024);

        // First buffer with enough data
        let mut state = ScannerState::new(2048);
        assert!(matches!(state.scan_phase, ScanPhase::FirstBuffer));
        assert!(state.bytes_in_buffer.has_at_least_n_blocks(2, block_size));

        // Not first buffer
        state.scan_phase.mark_advanced();
        assert!(matches!(state.scan_phase, ScanPhase::SubsequentBuffer));

        // First buffer but not enough data
        let state2 = ScannerState::new(1500);
        assert!(matches!(state2.scan_phase, ScanPhase::FirstBuffer));
        assert!(!state2.bytes_in_buffer.has_at_least_n_blocks(2, block_size));
    }

    #[test]
    fn test_remainder_calculation() {
        let block_size = BlockSize::new(1024);
        let mut state = ScannerState::new(1500);

        // At start: 1500 bytes, can fit one block with 476 byte remainder
        assert_eq!(state.remainder_size(block_size), 0); // No remainder at scan_pos=0

        // Advance past one block
        state.buffer_position.advance_by(1024);
        assert_eq!(state.remainder_size(block_size), 476);
    }

    #[test]
    fn test_window_sliding() {
        let block_size = BlockSize::new(1024);
        let mut state = ScannerState::new(2048);

        assert!(state.can_slide_window(block_size));
        assert!(matches!(state.scan_phase, ScanPhase::FirstBuffer));

        state.slide_window(block_size, BufferSize::new(1500));

        assert!(!matches!(state.scan_phase, ScanPhase::FirstBuffer));
        assert_eq!(state.bytes_in_buffer.as_usize(), 1500);
        assert_eq!(state.buffer_position, BufferPosition::zero());
        assert!(state.rolling_crc.is_none());
    }

    #[test]
    fn test_scan_position_advancement() {
        let mut state = ScannerState::new(2048);
        let block_size = BlockSize::new(1024);

        assert_eq!(state.buffer_position.as_usize(), 0);

        state.advance_one_byte();
        assert_eq!(state.buffer_position.as_usize(), 1);

        state.skip_block(block_size);
        assert_eq!(state.buffer_position.as_usize(), 1025);
    }

    #[test]
    fn test_remainder_at_start_detection() {
        let state = ScannerState::new(500);
        assert!(state.is_remainder_at_start());

        let mut state2 = ScannerState::new(500);
        state2.buffer_position.advance_by(100);
        assert!(!state2.is_remainder_at_start());
    }
}
