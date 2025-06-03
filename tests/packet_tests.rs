use binread::BinReaderExt;
use std::fs::File;
use std::path::Path;
use par2rs::{Par2Header, MainPacket, FileDescriptionPacket, InputFileSliceChecksumPacket, RecoverySlicePacket, CreatorPacket};

#[test]
fn test_par2_header() {
    let file_path = Path::new("test/fixtures/testfile.par2");
    let mut file = File::open(file_path).expect("Failed to open test file");
    let header: Par2Header = file.read_le().expect("Failed to read Par2Header");
    assert_eq!(header.magic, *b"PAR2\0PKT");
    assert!(header.length > 0);
}

#[test]
fn test_main_packet() {
    let file_path = Path::new("test/fixtures/testfile.par2");
    let mut file = File::open(file_path).expect("Failed to open test file");
    file.read_le::<Par2Header>().expect("Failed to read Par2Header");
    let main_packet: MainPacket = file.read_le().expect("Failed to read MainPacket");
    assert!(main_packet.slice_size > 0);
    assert!(main_packet.file_count > 0);
}

#[test]
fn test_file_description_packet() {
    let file_path = Path::new("test/fixtures/testfile.par2");
    let mut file = File::open(file_path).expect("Failed to open test file");
    file.read_le::<Par2Header>().expect("Failed to read Par2Header");
    file.read_le::<MainPacket>().expect("Failed to read MainPacket");
    let file_description: FileDescriptionPacket = file.read_le().expect("Failed to read FileDescriptionPacket");
    assert!(file_description.file_length > 0);
    assert!(!file_description.file_name.is_empty());
}

#[test]
fn test_input_file_slice_checksum_packet() {
    let file_path = Path::new("test/fixtures/testfile.par2");
    let mut file = File::open(file_path).expect("Failed to open test file");
    file.read_le::<Par2Header>().expect("Failed to read Par2Header");
    file.read_le::<MainPacket>().expect("Failed to read MainPacket");
    let input_file_slice_checksum: InputFileSliceChecksumPacket = file.read_le().expect("Failed to read InputFileSliceChecksumPacket");
    assert!(!input_file_slice_checksum.slice_checksums.is_empty());
}

#[test]
fn test_recovery_slice_packet() {
    let file_path = Path::new("test/fixtures/testfile.vol00+01.par2");
    let mut file = File::open(file_path).expect("Failed to open test file");
    file.read_le::<Par2Header>().expect("Failed to read Par2Header");
    let recovery_slice: RecoverySlicePacket = file.read_le().expect("Failed to read RecoverySlicePacket");
    assert!(!recovery_slice.recovery_data.is_empty());
}

#[test]
fn test_creator_packet() {
    let file_path = Path::new("test/fixtures/testfile.par2");
    let mut file = File::open(file_path).expect("Failed to open test file");
    file.read_le::<Par2Header>().expect("Failed to read Par2Header");
    let creator_packet: CreatorPacket = file.read_le().expect("Failed to read CreatorPacket");
    assert!(!creator_packet.creator_info.is_empty());
}
