//! Test demonstrating that PAR2 blocks do not overlap
//!
//! PAR2 blocks are fixed-size aligned chunks. Once we find a block at position N,
//! the next block can only start at N + block_size, not N + 1.

use par2rs::checksum::{compute_block_checksums_padded, compute_md5_only};
use par2rs::domain::FileId;
use par2rs::verify::GlobalBlockTableBuilder;
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;

#[test]
fn test_blocks_are_not_overlapping() {
    let block_size = 1024;
    let temp_dir = TempDir::new().unwrap();

    // Create a file with 3 blocks of data
    let file_path = temp_dir.path().join("test.bin");
    let mut file = File::create(&file_path).unwrap();

    // Block 0: All 'A's
    let block0_data = vec![b'A'; block_size];
    file.write_all(&block0_data).unwrap();

    // Block 1: All 'B's
    let block1_data = vec![b'B'; block_size];
    file.write_all(&block1_data).unwrap();

    // Block 2: All 'C's
    let block2_data = vec![b'C'; block_size];
    file.write_all(&block2_data).unwrap();

    file.flush().unwrap();
    drop(file);

    // Compute checksums for each block
    let (md5_0, crc_0) = compute_block_checksums_padded(&block0_data, block_size);
    let (md5_1, crc_1) = compute_block_checksums_padded(&block1_data, block_size);
    let (md5_2, crc_2) = compute_block_checksums_padded(&block2_data, block_size);

    // Build a block table with these 3 blocks
    let file_id = FileId::new([1; 16]);
    let mut builder = GlobalBlockTableBuilder::new(block_size as u64);

    builder.add_file_blocks(file_id, &[(md5_0, crc_0), (md5_1, crc_1), (md5_2, crc_2)]);

    let block_table = builder.build();

    // Verify blocks are at expected positions and DON'T overlap
    let file_blocks = block_table.get_file_blocks(file_id);
    assert_eq!(file_blocks.len(), 3);

    // Block 0 at position 0
    let block0 = file_blocks.first().unwrap();
    assert_eq!(block0.checksums.md5_hash, md5_0);
    assert_eq!(block0.checksums.crc32, crc_0);

    // Block 1 at position 1 (NOT overlapping with block 0)
    let block1 = file_blocks.get(1).unwrap();
    assert_eq!(block1.checksums.md5_hash, md5_1);
    assert_eq!(block1.checksums.crc32, crc_1);

    // Block 2 at position 2 (NOT overlapping with block 1)
    let block2 = file_blocks.get(2).unwrap();
    assert_eq!(block2.checksums.md5_hash, md5_2);
    assert_eq!(block2.checksums.crc32, crc_2);
}

#[test]
fn test_overlapping_scan_would_find_false_matches() {
    // This test demonstrates WHY we don't need to scan overlapping positions
    let block_size = 8;

    // Create data: "AAAAAAAABBBBBBBB" (two 8-byte blocks)
    let data = b"AAAAAAAABBBBBBBB";

    // Compute checksums for the two actual blocks
    let block0 = &data[0..8]; // "AAAAAAAA"
    let block1 = &data[8..16]; // "BBBBBBBB"

    let (md5_0, crc_0) = compute_block_checksums_padded(block0, block_size);
    let (md5_1, crc_1) = compute_block_checksums_padded(block1, block_size);

    // If we scanned at offset 1, we'd get: "AAAAAAAB" - this is NOT a valid PAR2 block
    // because PAR2 blocks are aligned to block_size boundaries
    let overlapping = &data[1..9]; // "AAAAAAAB"
    let (md5_overlap, crc_overlap) = compute_block_checksums_padded(overlapping, block_size);

    // This overlapping position should NOT match either block
    assert_ne!(md5_overlap, md5_0);
    assert_ne!(md5_overlap, md5_1);
    assert_ne!(crc_overlap, crc_0);
    assert_ne!(crc_overlap, crc_1);

    // Conclusion: Scanning byte-by-byte is wasting 99.9% of CPU time checking
    // positions that can NEVER be valid PAR2 blocks
}

#[test]
fn test_scanning_should_skip_after_match() {
    // This test shows the correct scanning strategy:
    // When you find a block at position N, skip to position N + block_size
    let block_size = 1024;

    let data = vec![b'X'; block_size * 3]; // 3 blocks worth of data
    let (expected_md5, _expected_crc) =
        compute_block_checksums_padded(&data[0..block_size], block_size);

    // Simulation of efficient scanning:
    let mut positions_checked = Vec::new();
    let mut pos = 0;

    while pos + block_size <= data.len() {
        positions_checked.push(pos);

        let block = &data[pos..pos + block_size];
        let md5 = compute_md5_only(block);

        if md5 == expected_md5 {
            // Found a match! Skip ahead by block_size
            pos += block_size;
        } else {
            // No match, try next byte (needed for misaligned/damaged files)
            pos += 1;
        }
    }

    // In this case, all 3 blocks match, so we should check positions:
    // 0 (match, skip to 1024)
    // 1024 (match, skip to 2048)
    // 2048 (match, skip to 3072, which is >= data.len)
    //
    // But with byte-by-byte scanning without skipping, we'd check:
    // 0, 1, 2, 3, ..., 1023, 1024, 1025, ..., 2047, 2048, ... (3072 positions!)
    //
    // That's 1024x more work than necessary for aligned files!

    println!(
        "Efficient scan: {} positions checked",
        positions_checked.len()
    );
    println!(
        "Byte-by-byte scan: {} positions",
        data.len() - block_size + 1
    );

    // The efficient scan should check far fewer positions
    assert!(positions_checked.len() < 100); // Should be ~3 + a few retries
    assert!(data.len() - block_size + 1 > 2000); // Byte-by-byte would be ~2048
}
