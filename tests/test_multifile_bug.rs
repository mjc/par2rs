/// Test to reproduce the multifile MD5 mismatch bug
use std::fs;
use std::path::PathBuf;
use par2rs::repair::RepairContext;
use par2rs::file_ops;

fn compute_md5(data: &[u8]) -> String {
    use md5::{Md5, Digest};
    let mut hasher = Md5::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

#[test]
fn test_multifile_repair_md5_mismatch() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    // Setup: Copy fixture to temp directory
    let fixture_dir = PathBuf::from("tests/fixtures/multifile_test");
    let temp_dir = std::env::temp_dir().join(format!("multifile_test_{}", std::process::id()));
    
    // Clean up any previous run
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    
    // Copy all files
    for entry in fs::read_dir(&fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let dest = temp_dir.join(entry.file_name());
        fs::copy(entry.path(), &dest).unwrap();
    }
    
    // Get original MD5s before corruption
    let large_original = temp_dir.join("large_file.bin");
    let tiny_original = temp_dir.join("tiny_file.bin");
    let large_md5 = compute_md5(&fs::read(&large_original).unwrap());
    let tiny_md5 = compute_md5(&fs::read(&tiny_original).unwrap());
    
    println!("Original large MD5: {}", large_md5);
    println!("Original tiny MD5: {}", tiny_md5);
    
    // The fixture has large_file.bin.1 as the corrupted version
    // Replace large_file.bin with the corrupted one
    fs::remove_file(&large_original).unwrap();
    fs::copy(temp_dir.join("large_file.bin.1"), &large_original).unwrap();
    
    // Delete tiny_file.bin
    fs::remove_file(&tiny_original).unwrap();
    
    // Run repair
    let par2_file = temp_dir.join("multifile.par2");
    let par2_files = file_ops::collect_par2_files(&par2_file);
    let metadata = file_ops::parse_recovery_slice_metadata(&par2_files, false);
    let packets = file_ops::load_par2_packets(&par2_files, false);
    
    let mut context = RepairContext::new_with_metadata(packets, metadata, temp_dir.clone()).unwrap();
    let result = context.repair().unwrap();
    
    println!("Repair result: {:?}", result);
    
    // Verify repaired files
    let large_repaired = fs::read(&large_original).unwrap();
    let tiny_repaired = fs::read(&tiny_original).unwrap();
    
    let large_repaired_md5 = compute_md5(&large_repaired);
    let tiny_repaired_md5 = compute_md5(&tiny_repaired);
    
    println!("Repaired large MD5: {}", large_repaired_md5);
    println!("Repaired tiny MD5: {}", tiny_repaired_md5);
    
    // Assert MD5s match
    assert_eq!(
        large_md5, large_repaired_md5,
        "large_file.bin MD5 mismatch after repair"
    );
    assert_eq!(
        tiny_md5, tiny_repaired_md5,
        "tiny_file.bin MD5 mismatch after repair"
    );
    
    // Cleanup
    fs::remove_dir_all(&temp_dir).unwrap();
}
