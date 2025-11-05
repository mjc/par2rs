use par2rs::domain::FileId;
use par2rs::verify::FileScanMetadata;

#[test]
fn test_empty_file_is_perfect_match() {
    let mut metadata = FileScanMetadata::new();
    let file_id = FileId::new([1; 16]);

    metadata.analyze_block_positions(file_id);

    assert!(metadata.is_perfect_match());
    assert!(metadata.first_block_at_offset_zero);
    assert!(metadata.blocks_in_sequence);
}

#[test]
fn test_single_block_at_offset_zero() {
    let mut metadata = FileScanMetadata::new();
    let file_id = FileId::new([1; 16]);

    metadata.record_block_found(0, file_id, 0);
    metadata.analyze_block_positions(file_id);

    assert!(metadata.is_perfect_match());
    assert!(metadata.first_block_at_offset_zero);
    assert!(metadata.blocks_in_sequence);
}

#[test]
fn test_single_block_not_at_offset_zero() {
    let mut metadata = FileScanMetadata::new();
    let file_id = FileId::new([1; 16]);

    metadata.record_block_found(1024, file_id, 0);
    metadata.analyze_block_positions(file_id);

    assert!(!metadata.is_perfect_match());
    assert!(!metadata.first_block_at_offset_zero);
}

#[test]
fn test_multiple_blocks_in_perfect_sequence() {
    let mut metadata = FileScanMetadata::new();
    let file_id = FileId::new([1; 16]);

    metadata.record_block_found(0, file_id, 0);
    metadata.record_block_found(1024, file_id, 1);
    metadata.record_block_found(2048, file_id, 2);
    metadata.analyze_block_positions(file_id);

    assert!(metadata.is_perfect_match());
    assert!(metadata.first_block_at_offset_zero);
    assert!(metadata.blocks_in_sequence);
}

#[test]
fn test_blocks_out_of_order_still_detected_as_sequence() {
    // Blocks recorded out of order but at correct offsets
    let mut metadata = FileScanMetadata::new();
    let file_id = FileId::new([1; 16]);

    metadata.record_block_found(2048, file_id, 2);
    metadata.record_block_found(0, file_id, 0);
    metadata.record_block_found(1024, file_id, 1);
    metadata.analyze_block_positions(file_id);

    assert!(metadata.is_perfect_match());
    assert!(metadata.first_block_at_offset_zero);
    assert!(metadata.blocks_in_sequence);
}

#[test]
fn test_missing_block_in_sequence() {
    let mut metadata = FileScanMetadata::new();
    let file_id = FileId::new([1; 16]);

    metadata.record_block_found(0, file_id, 0);
    metadata.record_block_found(2048, file_id, 2); // Missing block 1
    metadata.analyze_block_positions(file_id);

    assert!(!metadata.is_perfect_match());
    assert!(metadata.first_block_at_offset_zero);
    assert!(!metadata.blocks_in_sequence); // Sequence is broken
}

#[test]
fn test_first_block_not_block_zero() {
    let mut metadata = FileScanMetadata::new();
    let file_id = FileId::new([1; 16]);

    // Block 1 at offset 0 (not block 0)
    metadata.record_block_found(0, file_id, 1);
    metadata.record_block_found(1024, file_id, 2);
    metadata.analyze_block_positions(file_id);

    assert!(!metadata.is_perfect_match());
    assert!(!metadata.first_block_at_offset_zero); // Block 0 not at offset 0
    assert!(!metadata.blocks_in_sequence); // First block must be block 0
}

#[test]
fn test_block_zero_not_at_offset_zero() {
    let mut metadata = FileScanMetadata::new();
    let file_id = FileId::new([1; 16]);

    // Block 0 at offset 1024 (displaced)
    metadata.record_block_found(1024, file_id, 0);
    metadata.record_block_found(2048, file_id, 1);
    metadata.analyze_block_positions(file_id);

    assert!(!metadata.is_perfect_match());
    assert!(!metadata.first_block_at_offset_zero);
}

#[test]
fn test_duplicate_blocks_break_sequence() {
    let mut metadata = FileScanMetadata::new();
    let file_id = FileId::new([1; 16]);

    // This simulates the bug we fixed - same block detected twice
    metadata.record_block_found(0, file_id, 0);
    metadata.record_block_found(0, file_id, 0); // Duplicate
    metadata.analyze_block_positions(file_id);

    assert!(metadata.first_block_at_offset_zero);
    assert!(!metadata.blocks_in_sequence); // w[1].1 == w[0].1 + 1 fails when both are 0
}

#[test]
fn test_blocks_from_different_files_ignored() {
    let mut metadata = FileScanMetadata::new();
    let file_id_1 = FileId::new([1; 16]);
    let file_id_2 = FileId::new([2; 16]);

    metadata.record_block_found(0, file_id_1, 0);
    metadata.record_block_found(1024, file_id_1, 1);
    metadata.record_block_found(0, file_id_2, 0); // Different file
    metadata.analyze_block_positions(file_id_1);

    assert!(metadata.is_perfect_match());
    assert_eq!(metadata.found_blocks.len(), 3); // All recorded
                                                // But only file_id_1 blocks analyzed
}

#[test]
fn test_blocks_with_gaps_in_offsets_but_sequential_numbers() {
    let mut metadata = FileScanMetadata::new();
    let file_id = FileId::new([1; 16]);

    // Blocks at non-standard offsets but sequential numbers
    metadata.record_block_found(0, file_id, 0);
    metadata.record_block_found(1000, file_id, 1); // Not aligned
    metadata.record_block_found(2000, file_id, 2);
    metadata.analyze_block_positions(file_id);

    // Still perfect - we only check block numbers, not offset alignment
    assert!(metadata.is_perfect_match());
    assert!(metadata.first_block_at_offset_zero);
    assert!(metadata.blocks_in_sequence);
}

#[test]
fn test_large_block_numbers() {
    let mut metadata = FileScanMetadata::new();
    let file_id = FileId::new([1; 16]);

    // Large file with many blocks
    for i in 0..1000u32 {
        metadata.record_block_found((i as usize) * 1024, file_id, i);
    }
    metadata.analyze_block_positions(file_id);

    assert!(metadata.is_perfect_match());
    assert!(metadata.first_block_at_offset_zero);
    assert!(metadata.blocks_in_sequence);
}

#[test]
fn test_blocks_in_reverse_offset_order() {
    let mut metadata = FileScanMetadata::new();
    let file_id = FileId::new([1; 16]);

    // Blocks at offsets that don't match their block numbers
    // Block 2 at offset 0, block 0 at offset 2048
    metadata.record_block_found(0, file_id, 2);
    metadata.record_block_found(1024, file_id, 1);
    metadata.record_block_found(2048, file_id, 0);
    metadata.analyze_block_positions(file_id);

    assert!(!metadata.is_perfect_match());
    // After sorting by offset: (0, 2), (1024, 1), (2048, 0)
    // First block at offset 0 is block 2, not block 0
    assert!(!metadata.first_block_at_offset_zero);
}
