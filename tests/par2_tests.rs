use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::fs::File;
use std::path::Path;
use binread::BinReaderExt;
use par2rs::Par2Header;

#[test]
fn test_par2_header_parsing() {
    let test_file_path = Path::new("test/fixtures/testfile.par2");
    assert!(test_file_path.exists(), "Test file does not exist");

    let mut file = File::open(test_file_path).expect("Failed to open test file");
    let header: Par2Header = file.read_le().expect("Failed to read Par2Header");

    // Validate the parsed header fields
    assert_eq!(&header.magic, b"PAR2\0PKT", "Magic field does not match");
    assert!(header.length > 0, "Length field should be greater than 0");
    // Add more assertions as needed to validate other fields
}

#[test]
fn test_repair_par2_file() {
    let test_file_path = "test/fixtures/testfile.par2";

    // Ensure the test file exists
    assert!(std::path::Path::new(test_file_path).exists(), "Test file does not exist");

    // Call the repair function
    par2rs::repair::repair_par2_file(test_file_path);

    // Add assertions to verify the repair process
    // For example, check if the repaired file exists or matches expected output
    println!("Repair test completed for: {}", test_file_path);
}

#[test]
fn test_repair_with_damaged_file() {
    let test_file_path = "test/fixtures/testfile.par2";
    let damaged_file_path = "test/fixtures/testfile_damaged.par2";

    // Ensure the test file exists
    assert!(std::path::Path::new(test_file_path).exists(), "Test file does not exist");

    // Create a damaged copy of the test file
    fs::copy(test_file_path, damaged_file_path).expect("Failed to copy test file");

    // Damage 1% of the file
    let mut file = OpenOptions::new().read(true).write(true).open(damaged_file_path).expect("Failed to open damaged file");
    let mut content = Vec::new();
    file.read_to_end(&mut content).expect("Failed to read damaged file");

    let damage_size = content.len() / 100; // 1% of the file size
    for i in 0..damage_size {
        content[i] = 0xFF; // Corrupt the data
    }

    file.set_len(0).expect("Failed to truncate damaged file");
    file.write_all(&content).expect("Failed to write damaged file");

    // Run the repair function
    par2rs::repair::repair_par2_file(damaged_file_path);

    // Add assertions to verify the repair process
    // For example, check if the repaired file matches the original test file
    println!("Repair test completed for damaged file: {}", damaged_file_path);

    // Clean up the damaged file
    fs::remove_file(damaged_file_path).expect("Failed to remove damaged file");
}
