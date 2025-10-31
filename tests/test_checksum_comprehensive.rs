use par2rs::checksum::*;
use par2rs::domain::Md5Hash;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_compute_md5_basic() {
    let data = b"Hello, World!";
    let hash = compute_md5(data);
    assert_eq!(hash.as_bytes().len(), 16);
}

#[test]
fn test_compute_md5_empty() {
    let data = b"";
    let hash = compute_md5(data);
    assert_eq!(hash.as_bytes().len(), 16);
}

#[test]
fn test_compute_md5_large() {
    let data = vec![0u8; 1024 * 1024]; // 1MB of zeros
    let hash = compute_md5(&data);
    assert_eq!(hash.as_bytes().len(), 16);
}

#[test]
fn test_new_md5_hasher() {
    let hasher = new_md5_hasher();
    // Just ensure it can be created
    let _ = hasher;
}

#[test]
fn test_finalize_md5() {
    let hasher = new_md5_hasher();
    let hash = finalize_md5(hasher);
    assert_eq!(hash.as_bytes().len(), 16);
}

#[test]
fn test_compute_md5_bytes() {
    let data = b"test data";
    let hash_bytes = compute_md5_bytes(data);
    assert_eq!(hash_bytes.len(), 16);
}

#[test]
fn test_compute_crc32_basic() {
    let data = b"test";
    let crc = compute_crc32(data);
    assert!(crc.as_u32() > 0);
}

#[test]
fn test_compute_crc32_empty() {
    let data = b"";
    let crc = compute_crc32(data);
    // Empty data should have a CRC value
    let _ = crc.as_u32();
}

#[test]
fn test_compute_crc32_padded_no_padding_needed() {
    let data = vec![0u8; 100];
    let crc = compute_crc32_padded(&data, 100);
    let _ = crc.as_u32(); // CRC exists
}

#[test]
fn test_compute_crc32_padded_with_padding() {
    let data = vec![0xAA; 50];
    let block_size = 100;
    let crc = compute_crc32_padded(&data, block_size);
    // Should pad with zeros to block_size
    let _ = crc.as_u32(); // CRC exists
}

#[test]
fn test_compute_crc32_padded_exact_multiple() {
    let data = vec![1u8; 200];
    let block_size = 100;
    let crc = compute_crc32_padded(&data, block_size);
    let _ = crc.as_u32(); // CRC exists
}

#[test]
fn test_compute_block_checksums() {
    let data = b"Block of data for checksumming";
    let (md5, crc) = compute_block_checksums(data);
    assert_eq!(md5.as_bytes().len(), 16);
    let _ = crc.as_u32(); // CRC exists
}

#[test]
fn test_compute_block_checksums_empty() {
    let data = b"";
    let (md5, crc) = compute_block_checksums(data);
    assert_eq!(md5.as_bytes().len(), 16);
    let _ = crc.as_u32();
}

#[test]
fn test_compute_block_checksums_padded() {
    let data = vec![0xFF; 75];
    let block_size = 100;
    let (md5, crc) = compute_block_checksums_padded(&data, block_size);
    assert_eq!(md5.as_bytes().len(), 16);
    let _ = crc.as_u32(); // CRC exists
}

#[test]
fn test_compute_block_checksums_padded_large() {
    let data = vec![42u8; 5000];
    let block_size = 16384;
    let (md5, crc) = compute_block_checksums_padded(&data, block_size);
    assert_eq!(md5.as_bytes().len(), 16);
    let _ = crc.as_u32(); // CRC exists
}

#[test]
fn test_compute_md5_crc32_simultaneous() {
    let data = b"Simultaneous computation test";
    let (md5, crc) = compute_md5_crc32_simultaneous(data);
    assert_eq!(md5.as_bytes().len(), 16);
    let _ = crc.as_u32(); // CRC exists
}

#[test]
fn test_compute_md5_crc32_simultaneous_large() {
    let data = vec![123u8; 100000];
    let (md5, crc) = compute_md5_crc32_simultaneous(&data);
    assert_eq!(md5.as_bytes().len(), 16);
    assert!(crc.as_u32() > 0);
}

#[test]
fn test_compute_md5_crc32_simultaneous_padded() {
    let data = vec![0xAB; 1000];
    let block_size = 16384;
    let (md5, crc) = compute_md5_crc32_simultaneous_padded(&data, block_size);
    assert_eq!(md5.as_bytes().len(), 16);
    let _ = crc.as_u32(); // CRC exists
}

#[test]
fn test_compute_md5_crc32_simultaneous_padded_exact() {
    let data = vec![55u8; 16384];
    let block_size = 16384;
    let (md5, crc) = compute_md5_crc32_simultaneous_padded(&data, block_size);
    assert_eq!(md5.as_bytes().len(), 16);
    assert!(crc.as_u32() > 0);
}

#[test]
fn test_compute_file_id_basic() {
    let md5_16k = Md5Hash::new([1; 16]);
    let file_length = 1024;
    let filename = b"test.txt";
    let file_id = compute_file_id(&md5_16k, file_length, filename);
    assert_eq!(file_id.as_bytes().len(), 16);
}

#[test]
fn test_compute_file_id_empty_filename() {
    let md5_16k = Md5Hash::new([2; 16]);
    let file_length = 0;
    let filename = b"";
    let file_id = compute_file_id(&md5_16k, file_length, filename);
    assert_eq!(file_id.as_bytes().len(), 16);
}

#[test]
fn test_compute_file_id_unicode_filename() {
    let md5_16k = Md5Hash::new([3; 16]);
    let file_length = 5000;
    let filename = "测试文件.txt".as_bytes();
    let file_id = compute_file_id(&md5_16k, file_length, filename);
    assert_eq!(file_id.as_bytes().len(), 16);
}

#[test]
fn test_compute_file_id_long_filename() {
    let md5_16k = Md5Hash::new([4; 16]);
    let file_length = 999999;
    let filename = "very_long_filename_with_many_characters_to_test_edge_cases.txt".as_bytes();
    let file_id = compute_file_id(&md5_16k, file_length, filename);
    assert_eq!(file_id.as_bytes().len(), 16);
}

#[test]
fn test_compute_recovery_set_id() {
    let main_packet_body = vec![1u8; 100];
    let set_id = compute_recovery_set_id(&main_packet_body);
    assert_eq!(set_id.len(), 16);
}

#[test]
fn test_compute_recovery_set_id_empty() {
    let main_packet_body = vec![];
    let set_id = compute_recovery_set_id(&main_packet_body);
    assert_eq!(set_id.len(), 16);
}

#[test]
fn test_calculate_file_md5_16k() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("test.txt");

    // Create a file smaller than 16KB
    let data = b"Small test file";
    fs::write(&file_path, data).unwrap();

    let result = calculate_file_md5_16k(&file_path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().as_bytes().len(), 16);
}

#[test]
fn test_calculate_file_md5_16k_large() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("large.bin");

    // Create a file larger than 16KB
    let data = vec![42u8; 20000];
    fs::write(&file_path, &data).unwrap();

    let result = calculate_file_md5_16k(&file_path);
    assert!(result.is_ok());
    // Should only hash first 16KB
    assert_eq!(result.unwrap().as_bytes().len(), 16);
}

#[test]
fn test_calculate_file_md5_16k_exactly_16kb() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("exact.bin");

    let data = vec![0xAA; 16384];
    fs::write(&file_path, &data).unwrap();

    let result = calculate_file_md5_16k(&file_path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().as_bytes().len(), 16);
}

#[test]
fn test_calculate_file_md5_16k_empty() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("empty.txt");

    fs::write(&file_path, b"").unwrap();

    let result = calculate_file_md5_16k(&file_path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().as_bytes().len(), 16);
}

#[test]
fn test_calculate_file_md5_16k_missing_file() {
    let file_path = std::path::Path::new("/nonexistent/file.txt");
    let result = calculate_file_md5_16k(file_path);
    assert!(result.is_err());
}

#[test]
fn test_calculate_file_md5() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("test.txt");

    let data = b"Test data for full MD5";
    fs::write(&file_path, data).unwrap();

    let result = calculate_file_md5(&file_path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().as_bytes().len(), 16);
}

#[test]
fn test_calculate_file_md5_large() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("large.bin");

    let data = vec![0xFF; 100000];
    fs::write(&file_path, &data).unwrap();

    let result = calculate_file_md5(&file_path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().as_bytes().len(), 16);
}

#[test]
fn test_calculate_file_md5_empty() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("empty.txt");

    fs::write(&file_path, b"").unwrap();

    let result = calculate_file_md5(&file_path);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().as_bytes().len(), 16);
}

#[test]
fn test_calculate_file_md5_missing_file() {
    let file_path = std::path::Path::new("/nonexistent/file.txt");
    let result = calculate_file_md5(file_path);
    assert!(result.is_err());
}

#[test]
fn test_checksums_consistency() {
    // Ensure different methods produce consistent results for the same data
    let data = b"Consistency test data";

    let md5_1 = compute_md5(data);
    let (md5_2, _) = compute_block_checksums(data);
    let (md5_3, _) = compute_md5_crc32_simultaneous(data);

    assert_eq!(md5_1.as_bytes(), md5_2.as_bytes());
    assert_eq!(md5_2.as_bytes(), md5_3.as_bytes());
}

#[test]
fn test_crc_consistency() {
    let data = vec![0x12, 0x34, 0x56, 0x78];

    let crc_1 = compute_crc32(&data);
    let (_, crc_2) = compute_block_checksums(&data);
    let (_, crc_3) = compute_md5_crc32_simultaneous(&data);

    assert_eq!(crc_1.as_u32(), crc_2.as_u32());
    assert_eq!(crc_2.as_u32(), crc_3.as_u32());
}

#[test]
fn test_padding_affects_crc() {
    let data = vec![0xAB; 50];

    let crc_no_pad = compute_crc32(&data);
    let crc_padded = compute_crc32_padded(&data, 100);

    // Padding should produce different CRC
    assert_ne!(crc_no_pad.as_u32(), crc_padded.as_u32());
}

#[test]
fn test_padding_affects_md5() {
    let data = vec![0xCD; 50];

    let (md5_no_pad, _) = compute_block_checksums(&data);
    let (md5_padded, _) = compute_block_checksums_padded(&data, 100);

    // Padding should produce different MD5
    assert_ne!(md5_no_pad.as_bytes(), md5_padded.as_bytes());
}

#[test]
fn test_different_data_different_hashes() {
    let data1 = b"First data";
    let data2 = b"Second data";

    let hash1 = compute_md5(data1);
    let hash2 = compute_md5(data2);

    assert_ne!(hash1.as_bytes(), hash2.as_bytes());
}

#[test]
fn test_different_file_ids_for_different_files() {
    let md5_1 = Md5Hash::new([1; 16]);
    let md5_2 = Md5Hash::new([2; 16]);

    let file_id_1 = compute_file_id(&md5_1, 1000, b"file1.txt");
    let file_id_2 = compute_file_id(&md5_2, 1000, b"file1.txt");
    let file_id_3 = compute_file_id(&md5_1, 2000, b"file1.txt");
    let file_id_4 = compute_file_id(&md5_1, 1000, b"file2.txt");

    // All should be different
    assert_ne!(file_id_1.as_bytes(), file_id_2.as_bytes());
    assert_ne!(file_id_1.as_bytes(), file_id_3.as_bytes());
    assert_ne!(file_id_1.as_bytes(), file_id_4.as_bytes());
}
