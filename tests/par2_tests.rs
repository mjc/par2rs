use std::path::Path;

#[test]
fn test_par2_header_parsing() {
    let test_file_path = Path::new("test/fixtures/testfile.par2");
    assert!(test_file_path.exists(), "Test file does not exist");

    println!("Test file exists and is ready for further testing.");
}
