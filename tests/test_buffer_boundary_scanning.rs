//! Tests for buffer boundary block detection
//!
//! These tests verify that blocks are correctly found even when they span
//! buffer boundaries during byte-by-byte scanning.

use par2rs::checksum::compute_block_checksums_padded;
use par2rs::domain::{Crc32Value, FileId, Md5Hash, RecoverySetId};
use par2rs::packets::{FileDescriptionPacket, InputFileSliceChecksumPacket, MainPacket};
use par2rs::reporters::SilentVerificationReporter;
use par2rs::verify::GlobalVerificationEngine;
use par2rs::Packet;
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;

fn create_packet_set(
    block_size: u64,
    file_id: FileId,
    file_name: &str,
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
        length: 120 + file_name.len() as u64,
        md5: Md5Hash::new([0; 16]),
        set_id,
        packet_type: *b"PAR 2.0\0FileDesc",
        file_id,
        md5_hash: Md5Hash::new([0; 16]),
        md5_16k: Md5Hash::new([0; 16]),
        file_length,
        file_name: file_name.as_bytes().to_vec(),
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
fn test_block_at_buffer_boundary() {
    let temp_dir = TempDir::new().unwrap();
    let base_dir = temp_dir.path();
    let block_size = 1024usize;

    // The buffer size is 2 * block_size = 2048 bytes
    // To trigger the bug, we need a block that STARTS at position 2048 or later
    // This ensures the block will only be found after sliding the buffer forward

    let test_file_path = base_dir.join("test.bin");
    let mut test_file = File::create(&test_file_path).unwrap();

    // Block 0: position 0-1023 (in first buffer read)
    let block0_data = vec![0u8; block_size];
    test_file.write_all(&block0_data).unwrap();

    // Block 1: position 1024-2047 (in first buffer read)
    let block1_data = vec![0x11u8; block_size];
    test_file.write_all(&block1_data).unwrap();

    // Block 2: position 2048-3071 (THIS will be missed with buggy code!)
    // After scanning the first 2048 bytes, buggy code seeks to position 2048
    // and reads a fresh buffer, missing any block that starts at exactly 2048
    let block2_data = vec![0xAAu8; block_size];
    test_file.write_all(&block2_data).unwrap();

    // Block 3: position 3072-4095 (padding)
    let block3_data = vec![0xFFu8; block_size];
    test_file.write_all(&block3_data).unwrap();

    test_file.flush().unwrap();
    drop(test_file);

    // Only block 2 has the correct checksum
    let (expected_md5, expected_crc32) = compute_block_checksums_padded(&block2_data, block_size);

    let file_id = FileId::new([1; 16]);
    let checksums = vec![
        (Md5Hash::new([0; 16]), Crc32Value::new(0)), // Block 0: wrong checksum
        (Md5Hash::new([0; 16]), Crc32Value::new(0)), // Block 1: wrong checksum
        (expected_md5, expected_crc32),              // Block 2: CORRECT checksum
        (Md5Hash::new([0; 16]), Crc32Value::new(0)), // Block 3: wrong checksum
    ];

    let packets = create_packet_set(
        block_size as u64,
        file_id,
        "test.bin",
        (block_size * 4) as u64,
        checksums,
    );

    let engine = GlobalVerificationEngine::from_packets(&packets, base_dir).unwrap();
    let reporter = SilentVerificationReporter::new();
    let results = engine.verify_recovery_set(&reporter);

    assert_eq!(results.files.len(), 1);
    let file_result = &results.files[0];

    // With the fix, block 2 should be found (not in damaged_blocks list)
    // With the bug, block 2 is missed (will be in damaged_blocks list)
    assert!(
        !file_result.damaged_blocks.contains(&2),
        "Block 2 at buffer boundary (position 2048) should be found! \
         This test demonstrates the buffer sliding bug. \
         Damaged blocks: {:?}",
        file_result.damaged_blocks
    );

    // Blocks 0, 1, and 3 should be damaged (wrong checksums)
    assert_eq!(file_result.damaged_blocks.len(), 3);
    assert!(file_result.damaged_blocks.contains(&0));
    assert!(file_result.damaged_blocks.contains(&1));
    assert!(file_result.damaged_blocks.contains(&3));
}
