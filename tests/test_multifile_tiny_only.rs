/// Minimal test to debug tiny_file.bin reconstruction (1 slice only)
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
fn test_tiny_file_reconstruction() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    // Setup: Copy fixture to temp directory
    let fixture_dir = PathBuf::from("tests/fixtures/multifile_test");
    let temp_dir = std::env::temp_dir().join(format!("multifile_tiny_{}", std::process::id()));
    
    // Clean up any previous run
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    
    // Copy all files
    for entry in fs::read_dir(&fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let dest = temp_dir.join(entry.file_name());
        fs::copy(entry.path(), &dest).unwrap();
    }
    
    // Delete ONLY tiny_file.bin (leave large_file.bin intact)
    let tiny_file = temp_dir.join("tiny_file.bin");
    
    // Load PAR2 to get the EXPECTED MD5 (not the damaged file's MD5!)
    let par2_file = temp_dir.join("multifile.par2");
    let par2_files = file_ops::collect_par2_files(&par2_file);
    let metadata = file_ops::parse_recovery_slice_metadata(&par2_files, false);
    let packets = file_ops::load_par2_packets(&par2_files, false);
    
    // Get expected MD5s from PAR2 packets
    println!("\nFile MD5s from PAR2 FileDescription packets:");
    for p in &packets {
        if let par2rs::packets::Packet::FileDescription(fd) = p {
            let name = fd.file_name.iter().take_while(|&&b| b != 0).copied().collect::<Vec<_>>();
            let name_str = String::from_utf8_lossy(&name);
            let md5 = hex::encode(fd.md5.as_bytes());
            println!("  {} -> {}", name_str, md5);
        }
    }
    
    // tiny_file.bin doesn't exist in fixture - that's intentional!
    // The repair should create it from scratch.
    if tiny_file.exists() {
        fs::remove_file(&tiny_file).unwrap();
    }
    
    let context = RepairContext::new_with_metadata(packets, metadata, temp_dir.clone()).unwrap();
    
    // Get the expected MD5 from the recovery_set (this is what repair uses)
    let tiny_expected_md5 = context.recovery_set.files.iter()
        .find(|f| f.file_name == "tiny_file.bin")
        .map(|f| hex::encode(f.md5_hash.as_bytes()))
        .expect("tiny_file.bin not found in recovery set");
    
    println!("\nExpected tiny MD5 from recovery_set: {}", tiny_expected_md5);
    
    // Print file order
    println!("\nFile order from PAR2:");
    for (idx, file) in context.recovery_set.files.iter().enumerate() {
        println!("  [{}] {} - global offset: {}, slices: {}, md5: {}",
            idx,
            file.file_name,
            file.global_slice_offset.as_usize(),
            file.slice_count,
            hex::encode(file.md5_hash.as_bytes())
        );
    }
    
    let result = context.repair().unwrap();
    println!("\nRepair result: {:?}", result);
    
    // Verify tiny_file.bin was reconstructed correctly
    let tiny_repaired = fs::read(&tiny_file).unwrap();
    let tiny_repaired_md5 = compute_md5(&tiny_repaired);
    
    println!("\nRepaired tiny MD5: {}", tiny_repaired_md5);
    println!("Repaired tiny size: {} bytes", tiny_repaired.len());
    
    // Show first 64 bytes of repaired
    println!("Repaired data (first 64 bytes): {:02x?}", &tiny_repaired[..64.min(tiny_repaired.len())]);
    
    assert_eq!(
        tiny_expected_md5, tiny_repaired_md5,
        "tiny_file.bin MD5 mismatch: expected {}, got {}",
        tiny_expected_md5, tiny_repaired_md5
    );
    
    // Cleanup
    fs::remove_dir_all(&temp_dir).unwrap();
}
