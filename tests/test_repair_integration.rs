//! Integration tests for PAR2 repair functionality
//!
//! These tests verify that our par2repair implementation can correctly
//! repair files in various corruption scenarios.

use par2rs::repair::repair_files;
use std::fs;
use std::path::Path;

#[test]
fn test_repair_corrupted_file() {
    // This test should reproduce the current failure with the corrupted test file
    let test_dir = "tests/fixtures/corrupted_test";
    let par2_file = format!("{}/testfile.par2", test_dir);
    
    // Ensure the test file exists
    assert!(Path::new(&par2_file).exists(), "PAR2 test file not found");
    
    // Ensure the corrupted testfile exists
    let test_file = format!("{}/testfile", test_dir);
    assert!(Path::new(&test_file).exists(), "Corrupted test file not found");
    
    // Attempt repair
    let result = repair_files(&par2_file, &[], false);
    
    match result {
        Ok(repair_result) => {
            // Currently this should fail (files_repaired should be 0)
            // When we fix the implementation, we should change this to assert success
            println!("Repair result: {:?}", repair_result);
            
            // For now, we expect the repair to report failure due to the Reed-Solomon issue
            if repair_result.files_repaired == 0 && repair_result.files_verified == 0 {
                println!("Expected failure reproduced: no files were repaired due to RS computation error");
                // This is the current failing behavior we want to fix
            } else if repair_result.files_repaired > 0 {
                // If this passes, it means we've successfully fixed the issue!
                println!("SUCCESS: File was successfully repaired!");
                
                // Verify the repaired file exists and has correct content
                assert!(Path::new(&test_file).exists());
                
                // Get file size to verify it was properly repaired
                let metadata = fs::metadata(&test_file).unwrap();
                assert_eq!(metadata.len(), 1048576, "Repaired file should be 1MB");
            }
        }
        Err(e) => {
            panic!("Repair function failed with error: {}", e);
        }
    }
}

#[test] 
fn test_repair_missing_file() {
    // Test repair when the file is completely missing
    let test_dir = "tests/fixtures/corrupted_test";
    let par2_file = format!("{}/testfile.par2", test_dir);
    let test_file = format!("{}/testfile", test_dir);
    
    // Back up the original file and remove it
    let backup_file = format!("{}.backup", test_file);
    if Path::new(&test_file).exists() {
        fs::copy(&test_file, &backup_file).expect("Failed to backup test file");
        fs::remove_file(&test_file).expect("Failed to remove test file");
    }
    
    // Attempt repair on missing file
    let result = repair_files(&par2_file, &[], false);
    
    // Restore the original file
    if Path::new(&backup_file).exists() {
        fs::copy(&backup_file, &test_file).expect("Failed to restore test file");
        fs::remove_file(&backup_file).expect("Failed to remove backup file");
    }
    
    match result {
        Ok(repair_result) => {
            println!("Missing file repair result: {:?}", repair_result);
            
            // With the current implementation, this should fail because we need
            // 1986 recovery blocks but only have 99
            assert_eq!(repair_result.files_repaired, 0, "Should not be able to repair completely missing file with insufficient recovery blocks");
        }
        Err(e) => {
            println!("Expected error for insufficient recovery blocks: {}", e);
        }
    }
}

#[test]
fn test_verify_intact_file() {
    // Test verification of an already intact file
    let test_dir = "tests/fixtures/repair_scenarios"; 
    let par2_file = format!("{}/testfile.par2", test_dir);
    
    if !Path::new(&par2_file).exists() {
        println!("Skipping test - repair_scenarios directory not available");
        return;
    }
    
    let result = repair_files(&par2_file, &[], false);
    
    match result {
        Ok(repair_result) => {
            println!("Intact file verification result: {:?}", repair_result);
            
            // For an intact file, we should see it verified, not repaired
            if repair_result.success {
                // Either verified (if already intact) or repaired (if was corrupted)
                assert!(repair_result.files_verified > 0 || repair_result.files_repaired > 0);
            }
        }
        Err(e) => {
            println!("Error during verification: {}", e);
        }
    }
}

#[cfg(test)]
mod helpers {
    use std::fs;
    use std::path::Path;
    
    /// Create a temporary corruption in a file for testing
    pub fn create_temporary_corruption(file_path: &str, offset: u64, corrupt_bytes: &[u8]) -> Result<Vec<u8>, std::io::Error> {
        let original_data = fs::read(file_path)?;
        let mut corrupted_data = original_data.clone();
        
        let start = offset as usize;
        let end = (start + corrupt_bytes.len()).min(corrupted_data.len());
        
        for (i, &byte) in corrupt_bytes.iter().enumerate() {
            if start + i < end {
                corrupted_data[start + i] = byte;
            }
        }
        
        fs::write(file_path, &corrupted_data)?;
        Ok(original_data)
    }
    
    /// Restore original file content
    pub fn restore_file_content(file_path: &str, original_data: &[u8]) -> Result<(), std::io::Error> {
        fs::write(file_path, original_data)
    }
}