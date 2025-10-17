//! Tests to ensure repair operations don't accidentally overwrite PAR2 files
//! 
//! This test suite investigates the bug where PAR2 files disappeared during
//! 25GB benchmark repairs between iteration 1 and 2.

use par2rs::file_ops;
use par2rs::repair::RepairContext;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

/// Test environment with PAR2 files
struct TestEnv {
    #[allow(dead_code)]
    temp_dir: TempDir,
    test_file: PathBuf,
    par2_file: PathBuf,
    par2_vol_files: Vec<PathBuf>,
}

impl TestEnv {
    fn new() -> Self {
        let temp_dir = TempDir::new().unwrap();
        let fixtures = PathBuf::from("tests/fixtures");

        // Copy test files
        let test_file = temp_dir.path().join("testfile");
        fs::copy(fixtures.join("testfile"), &test_file).unwrap();
        
        let par2_file = temp_dir.path().join("testfile.par2");
        fs::copy(fixtures.join("testfile.par2"), &par2_file).unwrap();

        // Copy all volume files
        let mut par2_vol_files = Vec::new();
        for entry in fs::read_dir(&fixtures).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("par2") 
                && path != fixtures.join("testfile.par2") {
                let filename = path.file_name().unwrap();
                let dest = temp_dir.path().join(filename);
                fs::copy(&path, &dest).unwrap();
                par2_vol_files.push(dest);
            }
        }

        TestEnv {
            temp_dir,
            test_file,
            par2_file,
            par2_vol_files,
        }
    }

    fn corrupt_at(&self, offset: u64, data: &[u8]) {
        let mut file = File::options().write(true).open(&self.test_file).unwrap();
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(offset)).unwrap();
        file.write_all(data).unwrap();
    }

    fn par2_files_exist(&self) -> bool {
        self.par2_file.exists() && self.par2_vol_files.iter().all(|f| f.exists())
    }

    fn count_par2_files(&self) -> usize {
        let mut count = 0;
        if self.par2_file.exists() {
            count += 1;
        }
        count += self.par2_vol_files.iter().filter(|f| f.exists()).count();
        count
    }

    fn list_all_files(&self) -> Vec<String> {
        let mut files = Vec::new();
        for entry in fs::read_dir(self.temp_dir.path()).unwrap() {
            let entry = entry.unwrap();
            files.push(entry.file_name().to_string_lossy().to_string());
        }
        files.sort();
        files
    }

    fn load_context(&self) -> RepairContext {
        let par2_files = file_ops::collect_par2_files(&self.par2_file);
        let metadata = file_ops::parse_recovery_slice_metadata(&par2_files, false);
        let packets = file_ops::load_par2_packets(&par2_files, false);
        RepairContext::new_with_metadata(packets, metadata, self.temp_dir.path().to_path_buf()).unwrap()
    }

    fn get_file_names_from_par2(&self) -> Vec<String> {
        let context = self.load_context();
        context.recovery_set.files.iter()
            .map(|f| f.file_name.clone())
            .collect()
    }
}

#[test]
fn test_par2_files_not_deleted_after_repair() {
    // Primary test: Ensure PAR2 files are never deleted during repair
    let env = TestEnv::new();
    
    let par2_count_before = env.count_par2_files();
    println!("PAR2 files before repair: {}", par2_count_before);
    println!("Files: {:?}", env.list_all_files());
    
    // Corrupt the test file
    env.corrupt_at(5000, &vec![0xFFu8; 1000]);
    
    // Perform repair
    let context = env.load_context();
    let result = context.repair().unwrap();
    
    assert!(result.is_success(), "Repair should succeed");
    
    let par2_count_after = env.count_par2_files();
    println!("PAR2 files after repair: {}", par2_count_after);
    println!("Files: {:?}", env.list_all_files());
    
    assert_eq!(
        par2_count_before, par2_count_after,
        "PAR2 file count should not change during repair"
    );
    
    assert!(
        env.par2_files_exist(),
        "All PAR2 files should still exist after repair"
    );
}

#[test]
fn test_multiple_repairs_dont_delete_par2_files() {
    // Test multiple repair cycles (simulating benchmark iterations)
    let env = TestEnv::new();
    
    let par2_count_initial = env.count_par2_files();
    
    for iteration in 1..=5 {
        println!("\n=== Iteration {} ===", iteration);
        
        // Corrupt at different location each time
        let offset = (iteration * 1000) as u64;
        env.corrupt_at(offset, &vec![0xAAu8; 500]);
        
        // Repair
        let context = env.load_context();
        let result = context.repair().unwrap();
        assert!(result.is_success(), "Repair should succeed in iteration {}", iteration);
        
        // Verify PAR2 files still exist
        let par2_count = env.count_par2_files();
        assert_eq!(
            par2_count_initial, par2_count,
            "PAR2 files should not disappear after iteration {}. Files present: {:?}",
            iteration, env.list_all_files()
        );
    }
}

#[test]
fn test_temp_file_cleanup() {
    // Ensure .par2_tmp files are cleaned up correctly
    let env = TestEnv::new();
    
    env.corrupt_at(1000, &vec![0xBBu8; 200]);
    
    let context = env.load_context();
    let _result = context.repair().unwrap();
    
    // Check for any leftover temp files
    let all_files = env.list_all_files();
    let temp_files: Vec<_> = all_files.iter()
        .filter(|f| f.contains("par2_tmp"))
        .collect();
    
    assert!(
        temp_files.is_empty(),
        "No .par2_tmp files should remain after repair. Found: {:?}",
        temp_files
    );
}

#[test]
fn test_file_names_from_par2_packets() {
    // Verify what filenames are stored in PAR2 packets
    let env = TestEnv::new();
    
    let file_names = env.get_file_names_from_par2();
    
    println!("Filenames from PAR2 packets: {:?}", file_names);
    
    // Ensure no PAR2 files are listed as data files
    for name in &file_names {
        assert!(
            !name.ends_with(".par2"),
            "PAR2 packet should not list '{}' as a data file",
            name
        );
    }
    
    // Should only be the data file
    assert_eq!(file_names.len(), 1, "Should have exactly 1 data file");
    assert_eq!(file_names[0], "testfile", "Data file should be 'testfile'");
}

#[test]
fn test_with_extension_behavior() {
    // Test that with_extension doesn't accidentally create PAR2 filenames
    use std::path::Path;
    
    let test_cases = vec![
        ("testfile", "testfile.par2_tmp"),
        ("testfile.txt", "testfile.par2_tmp"),
        ("testfile.par2", "testfile.par2_tmp"), // This would be bad if testfile.par2 is a data file!
        ("testfile.vol00+01.par2", "testfile.vol00+01.par2_tmp"),
    ];
    
    for (input, expected) in test_cases {
        let path = Path::new(input);
        let result = path.with_extension("par2_tmp");
        println!("{} -> {}", input, result.display());
        assert_eq!(
            result.to_string_lossy(),
            expected,
            "with_extension behavior for '{}'",
            input
        );
    }
}

#[test]
fn test_repair_doesnt_write_to_par2_directory() {
    // Ensure repair writes to data file location, not PAR2 file location
    let env = TestEnv::new();
    
    env.corrupt_at(2000, &vec![0xCCu8; 300]);
    
    let context = env.load_context();
    let base_path = context.base_path.clone();
    
    // Get the data file name from the recovery set
    let data_file_name = &context.recovery_set.files[0].file_name;
    let expected_repair_path = base_path.join(data_file_name);
    
    println!("Expected repair path: {:?}", expected_repair_path);
    println!("PAR2 file path: {:?}", env.par2_file);
    
    // These should be different paths (unless data file IS named .par2, which would be weird)
    if data_file_name.ends_with(".par2") {
        panic!("Data file should not have .par2 extension: {}", data_file_name);
    }
    
    let _result = context.repair().unwrap();
    
    // Verify the repair wrote to the correct file
    assert!(
        expected_repair_path.exists(),
        "Repaired data file should exist at {:?}",
        expected_repair_path
    );
}

#[test]
fn test_large_file_simulation() {
    // Simulate the 25GB case with a smaller file but same pattern
    // This tests if there's any size-related behavior that could cause issues
    let env = TestEnv::new();
    
    let initial_files = env.list_all_files();
    let initial_par2_count = env.count_par2_files();
    
    println!("Initial files: {:?}", initial_files);
    println!("Initial PAR2 count: {}", initial_par2_count);
    
    // Simulate multiple repairs like the benchmark script does
    for i in 1..=3 {
        println!("\n=== Simulated benchmark iteration {} ===", i);
        
        // Corrupt
        env.corrupt_at(10000, &vec![0xDDu8; 512]);
        
        // Repair
        let context = env.load_context();
        let result = context.repair();
        
        match result {
            Ok(r) => {
                println!("Iteration {}: Repair succeeded", i);
                assert!(r.is_success());
            }
            Err(e) => {
                println!("Iteration {}: Repair failed: {}", i, e);
                println!("Files after failure: {:?}", env.list_all_files());
                println!("PAR2 files exist: {}", env.par2_files_exist());
                println!("PAR2 count: {}", env.count_par2_files());
                panic!("Repair failed in iteration {}: {}", i, e);
            }
        }
        
        // Check files still exist
        let current_par2_count = env.count_par2_files();
        let current_files = env.list_all_files();
        
        println!("After iteration {}: {} PAR2 files", i, current_par2_count);
        println!("Files: {:?}", current_files);
        
        if current_par2_count != initial_par2_count {
            panic!(
                "PAR2 files disappeared after iteration {}! Before: {}, After: {}. Files: {:?}",
                i, initial_par2_count, current_par2_count, current_files
            );
        }
    }
}
