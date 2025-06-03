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
