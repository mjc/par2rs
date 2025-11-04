//! Tests for global verification scanning bugs
//!
//! Each test demonstrates a specific bug in the scan_file_in_chunks implementation.
//! These tests should FAIL with the current implementation and PASS when bugs are fixed.

use par2rs::domain::{Crc32Value, FileId, Md5Hash, RecoverySetId};
use par2rs::packets::{FileDescriptionPacket, InputFileSliceChecksumPacket, MainPacket};
use par2rs::reporters::ConsoleVerificationReporter;
use par2rs::verify::GlobalVerificationEngine;
use par2rs::Packet;
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;

/// Helper to create a test file with known blocks at specific positions
fn create_test_file_with_blocks(path: &std::path::Path, blocks: &[(usize, &[u8])]) {
    let mut file = File::create(path).unwrap();

    // Find the maximum position needed
    let max_end = blocks
        .iter()
        .map(|(pos, data)| pos + data.len())
        .max()
        .unwrap_or(0);

    // Create file filled with zeros
    let mut file_data = vec![0u8; max_end];

    // Insert blocks at specified positions
    for (pos, data) in blocks {
        file_data[*pos..*pos + data.len()].copy_from_slice(data);
    }

    file.write_all(&file_data).unwrap();
}

/// Helper to create packet set with known checksums
fn create_packet_set(
    block_size: u64,
    file_id: FileId,
    file_length: u64,
    checksums: Vec<(Md5Hash, Crc32Value)>,
) -> Vec<Packet> {
    let set_id = RecoverySetId::new([1; 16]);

    let main_packet = MainPacket {
        length: 92,
        md5: Md5Hash::new([0; 16]),
        set_id,
        slice_size: block_size,
        file_count: 1,
        file_ids: vec![file_id],
        non_recovery_file_ids: vec![],
    };

    let file_desc = FileDescriptionPacket {
        length: 120 + 8,
        md5: Md5Hash::new([0; 16]),
        set_id,
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id,
        md5_hash: Md5Hash::new([0; 16]),
        md5_16k: Md5Hash::new([0; 16]),
        file_length,
        file_name: b"test.bin".to_vec(),
    };

    let checksum_packet = InputFileSliceChecksumPacket {
        length: 64 + 16 + (checksums.len() * 20) as u64,
        md5: Md5Hash::new([0; 16]),
        set_id,
        file_id,
        slice_checksums: checksums,
    };

    vec![
        Packet::Main(main_packet),
        Packet::FileDescription(file_desc),
        Packet::InputFileSliceChecksum(checksum_packet),
    ]
}

#[test]

fn test_bug_1_file_position_desync_after_finding_block() {
    // Bug: After finding a block at offset within buffer, file.read() is called
    // without seeking, causing file position and current_offset to desync.
    //
    // This test creates a file with two blocks:
    // - Block 1 at byte 100 (misaligned within first buffer read)
    // - Block 2 at byte 100 + 1024 = 1124 (immediately after block 1)
    //
    // Expected: Both blocks should be found
    // Actual (buggy): Only first block is found because after finding it,
    //                 the code doesn't seek properly before next read

    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.bin");
    let block_size = 1024;

    // Create two blocks with known content
    let block1 = vec![0x42u8; block_size];
    let block2 = vec![0x43u8; block_size];

    let (md5_1, crc_1) = par2rs::checksum::compute_block_checksums(&block1);
    let (md5_2, crc_2) = par2rs::checksum::compute_block_checksums(&block2);

    // Create file with block1 at offset 100, block2 immediately after at 1124
    create_test_file_with_blocks(&file_path, &[(100, &block1), (100 + block_size, &block2)]);

    let file_id = FileId::new([2; 16]);
    let checksums = vec![(md5_1, crc_1), (md5_2, crc_2)];
    let packets = create_packet_set(block_size as u64, file_id, 4096, checksums);

    let engine = GlobalVerificationEngine::from_packets(&packets, temp_dir.path()).unwrap();
    let reporter = ConsoleVerificationReporter::new();
    let results = engine.verify_recovery_set(&reporter, true);

    // Both blocks should be found
    assert_eq!(
        results.available_block_count, 2,
        "Expected both blocks to be found, but only {} were found. \
         This indicates file position desync after finding first block.",
        results.available_block_count
    );
}

#[test]

fn test_bug_2_missing_blocks_in_second_buffer() {
    // Bug: When scanning completes on first buffer and no block is found,
    // the buffer shift logic doesn't properly seek the file before reading.
    //
    // This test creates a file with a block positioned such that it spans
    // the boundary between what should be the first and second buffer reads.
    //
    // Expected: Block should be found during second buffer scan
    // Actual (buggy): Block is missed because file position is wrong

    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.bin");
    let block_size = 1024;

    let block = vec![0x55u8; block_size];
    let (md5, crc) = par2rs::checksum::compute_block_checksums(&block);

    // Place block at position that requires second buffer:
    // First buffer reads 0-2048, block starts at 2000
    // So block spans from 2000-3024, partially in first buffer
    create_test_file_with_blocks(&file_path, &[(2000, &block)]);

    let file_id = FileId::new([2; 16]);
    let checksums = vec![(md5, crc)];
    let packets = create_packet_set(block_size as u64, file_id, 4096, checksums);

    let engine = GlobalVerificationEngine::from_packets(&packets, temp_dir.path()).unwrap();
    let reporter = ConsoleVerificationReporter::new();
    let results = engine.verify_recovery_set(&reporter, true);

    assert_eq!(
        results.available_block_count, 1,
        "Expected block to be found in second buffer, but it was missed. \
         This indicates incorrect buffer shift/seek logic."
    );
}

#[test]

fn test_bug_3_skipped_data_between_buffers() {
    // Bug: The buffer management logic doesn't properly handle the transition
    // between buffers, potentially skipping bytes.
    //
    // This test creates multiple blocks at carefully chosen positions to
    // verify that ALL data is scanned byte-by-byte without gaps.
    //
    // Expected: All 4 blocks should be found
    // Actual (buggy): Some blocks are skipped due to incorrect buffer management

    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.bin");
    let block_size = 1024;

    // Create 4 different blocks
    let block1 = vec![0x11u8; block_size];
    let block2 = vec![0x22u8; block_size];
    let block3 = vec![0x33u8; block_size];
    let block4 = vec![0x44u8; block_size];

    let (md5_1, crc_1) = par2rs::checksum::compute_block_checksums(&block1);
    let (md5_2, crc_2) = par2rs::checksum::compute_block_checksums(&block2);
    let (md5_3, crc_3) = par2rs::checksum::compute_block_checksums(&block3);
    let (md5_4, crc_4) = par2rs::checksum::compute_block_checksums(&block4);

    // Position blocks at tricky offsets that test buffer boundaries:
    // - Block 1 at 50 (early in first buffer 0-2048)
    // - Block 2 at 1100 (still in first buffer, after jump point at 1024)
    // - Block 3 at 2600 (in second buffer 1024-3072 after first jump)
    // - Block 4 at 3900 (in third buffer 2048-4096 or later)
    create_test_file_with_blocks(
        &file_path,
        &[
            (50, &block1),
            (1100, &block2),
            (2600, &block3),
            (3900, &block4),
        ],
    );

    let file_id = FileId::new([2; 16]);
    let checksums = vec![
        (md5_1, crc_1),
        (md5_2, crc_2),
        (md5_3, crc_3),
        (md5_4, crc_4),
    ];
    let packets = create_packet_set(block_size as u64, file_id, 8192, checksums);

    let engine = GlobalVerificationEngine::from_packets(&packets, temp_dir.path()).unwrap();
    let reporter = ConsoleVerificationReporter::new();
    let results = engine.verify_recovery_set(&reporter, true);

    assert_eq!(
        results.available_block_count, 4,
        "Expected all 4 blocks to be found, but only {} were found. \
         This indicates data is being skipped during buffer transitions.",
        results.available_block_count
    );
}

#[test]

fn test_bug_4_current_offset_tracking_incorrect() {
    // Bug: current_offset is incremented but not used for seeking,
    // making it meaningless and causing position tracking to be wrong.
    //
    // This test verifies that the scanning logic correctly tracks position
    // by finding blocks at precise offsets.
    //
    // Expected: Block should be found at exact position
    // Actual (buggy): Block might be missed or position tracking is off

    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.bin");
    let block_size = 1024;

    let block = vec![0x99u8; block_size];
    let (md5, crc) = par2rs::checksum::compute_block_checksums(&block);

    // Place block at an offset that would expose position tracking issues
    // Offset 1000 is less than block_size, so it should be in first buffer scan
    let offset = 1000;
    create_test_file_with_blocks(&file_path, &[(offset, &block)]);

    let file_id = FileId::new([2; 16]);
    let checksums = vec![(md5, crc)];
    let packets = create_packet_set(block_size as u64, file_id, 4096, checksums);

    let engine = GlobalVerificationEngine::from_packets(&packets, temp_dir.path()).unwrap();
    let reporter = ConsoleVerificationReporter::new();
    let results = engine.verify_recovery_set(&reporter, true);

    assert_eq!(
        results.available_block_count, 1,
        "Expected block at offset {} to be found, but it was missed. \
         This indicates current_offset tracking is not being used correctly.",
        offset
    );
}

#[test]

fn test_bug_5_buffer_refill_without_seek() {
    // Bug: After finding a block, the code calls file.read() to refill the buffer
    // but doesn't seek to the correct position first.
    //
    // This test specifically checks the scenario where:
    // 1. A block is found partway through the buffer
    // 2. The code should seek past that block
    // 3. Then refill the buffer from the new position
    //
    // Expected: Subsequent blocks should be found
    // Actual (buggy): File position is wrong, subsequent blocks are missed

    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.bin");
    let block_size = 1024;

    // Create 3 consecutive blocks
    let block1 = vec![0xAAu8; block_size];
    let block2 = vec![0xBBu8; block_size];
    let block3 = vec![0xCCu8; block_size];

    let (md5_1, crc_1) = par2rs::checksum::compute_block_checksums(&block1);
    let (md5_2, crc_2) = par2rs::checksum::compute_block_checksums(&block2);
    let (md5_3, crc_3) = par2rs::checksum::compute_block_checksums(&block3);

    // Place first block at offset 500, then consecutive blocks
    create_test_file_with_blocks(
        &file_path,
        &[
            (500, &block1),                  // at 500
            (500 + block_size, &block2),     // at 1524
            (500 + 2 * block_size, &block3), // at 2548
        ],
    );

    let file_id = FileId::new([2; 16]);
    let checksums = vec![(md5_1, crc_1), (md5_2, crc_2), (md5_3, crc_3)];
    let packets = create_packet_set(block_size as u64, file_id, 8192, checksums);

    let engine = GlobalVerificationEngine::from_packets(&packets, temp_dir.path()).unwrap();
    let reporter = ConsoleVerificationReporter::new();
    let results = engine.verify_recovery_set(&reporter, true);

    assert_eq!(
        results.available_block_count, 3,
        "Expected all 3 consecutive blocks to be found, but only {} were found. \
         This indicates buffer refill doesn't seek to correct position after finding a block.",
        results.available_block_count
    );
}

#[test]

fn test_bug_6_no_seek_in_no_match_path() {
    // Bug: When the scan completes without finding anything and needs to advance,
    // it moves current_offset forward but doesn't seek the file.
    //
    // This test creates a scenario where blocks are positioned such that
    // the first buffer contains no matches, forcing the "no match" code path.
    //
    // Expected: Blocks in later positions should still be found
    // Actual (buggy): Blocks are missed because file isn't seeked properly

    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.bin");
    let block_size = 1024;

    let block = vec![0xDDu8; block_size];
    let (md5, crc) = par2rs::checksum::compute_block_checksums(&block);

    // Place block AFTER the first 2-block buffer would be (> 2048)
    // This forces the scanning to go through "no match" path first
    let offset = 3000;
    create_test_file_with_blocks(&file_path, &[(offset, &block)]);

    let file_id = FileId::new([2; 16]);
    let checksums = vec![(md5, crc)];
    let packets = create_packet_set(block_size as u64, file_id, 8192, checksums);

    let engine = GlobalVerificationEngine::from_packets(&packets, temp_dir.path()).unwrap();
    let reporter = ConsoleVerificationReporter::new();
    let results = engine.verify_recovery_set(&reporter, true);

    assert_eq!(
        results.available_block_count, 1,
        "Expected block at offset {} to be found after 'no match' buffer advance, \
         but it was missed. This indicates missing seek in no-match code path.",
        offset
    );
}

#[test]

fn test_bug_7_buffer_copy_within_desync() {
    // Bug: The buffer.copy_within() logic tries to shift buffer data,
    // but the file position is already past that data, causing misalignment.
    //
    // This test checks that buffer shifting works correctly by placing
    // blocks such that they require proper buffer management.
    //
    // Expected: Block overlapping buffer boundaries should be found
    // Actual (buggy): Block is missed due to buffer/file position mismatch

    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.bin");
    let block_size = 1024;

    let block = vec![0xEEu8; block_size];
    let (md5, crc) = par2rs::checksum::compute_block_checksums(&block);

    // Place block exactly at the block_size boundary (1024)
    // First buffer reads 0-2048, scanning advances by block_size to 1024
    // Block at 1024 should be caught by buffer shifting logic
    let offset = block_size;
    create_test_file_with_blocks(&file_path, &[(offset, &block)]);

    let file_id = FileId::new([2; 16]);
    let checksums = vec![(md5, crc)];
    let packets = create_packet_set(block_size as u64, file_id, 4096, checksums);

    let engine = GlobalVerificationEngine::from_packets(&packets, temp_dir.path()).unwrap();
    let reporter = ConsoleVerificationReporter::new();
    let results = engine.verify_recovery_set(&reporter, true);

    assert_eq!(
        results.available_block_count, 1,
        "Expected block at buffer boundary (offset {}) to be found, \
         but it was missed. This indicates buffer copy_within logic is broken.",
        offset
    );
}
