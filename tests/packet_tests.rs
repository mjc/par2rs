use binread::BinReaderExt;
use par2rs::{
    CreatorPacket, FileDescriptionPacket, InputFileSliceChecksumPacket, MainPacket,
    RecoverySlicePacket,
};
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[test]
fn test_main_packet_start() {
    let file_path = Path::new("test/fixtures/testfile.par2");
    let mut file = File::open(file_path).expect("Failed to open test file");
    let main_packet: MainPacket = file.read_le().expect("Failed to read MainPacket");
    assert_eq!(
        &main_packet.magic, b"PAR2\0PKT",
        "Magic field does not match"
    );
    assert!(
        main_packet.length > 0,
        "Length field should be greater than 0"
    );
}

#[test]
fn test_file_description_packet_start() {
    let file_path = Path::new("test/fixtures/testfile.par2");
    let mut file = File::open(file_path).expect("Failed to open test file");
    file.read_le::<MainPacket>()
        .expect("Failed to read MainPacket");
    let file_description: FileDescriptionPacket = file
        .read_le()
        .expect("Failed to read FileDescriptionPacket");
    assert_eq!(
        &file_description.magic, b"PAR2\0PKT",
        "Magic field does not match"
    );
    assert!(
        file_description.length > 0,
        "Length field should be greater than 0"
    );
}

#[test]
fn test_input_file_slice_checksum_packet_start() {
    let file_path = Path::new("test/fixtures/testfile.par2");
    let mut file = File::open(file_path).expect("Failed to open test file");
    file.read_le::<MainPacket>()
        .expect("Failed to read MainPacket");
    let input_file_slice_checksum: InputFileSliceChecksumPacket = file
        .read_le()
        .expect("Failed to read InputFileSliceChecksumPacket");
    assert_eq!(
        &input_file_slice_checksum.magic, b"PAR2\0PKT",
        "Magic field does not match"
    );
    assert!(
        input_file_slice_checksum.length > 0,
        "Length field should be greater than 0"
    );
}

#[test]
fn test_recovery_slice_packet_start() {
    let file_path = Path::new("test/fixtures/testfile.vol00+01.par2");
    let mut file = File::open(file_path).expect("Failed to open test file");
    let recovery_slice: RecoverySlicePacket =
        file.read_le().expect("Failed to read RecoverySlicePacket");
    let mut raw_data = vec![0u8; recovery_slice.length as usize];
    file.read_exact(&mut raw_data)
        .expect("Failed to read raw data");
    println!("Raw data: {:?}", raw_data);
    println!("Parsed RecoverySlicePacket: {:?}", recovery_slice);
    assert_eq!(
        &recovery_slice.magic, b"PAR2\0PKT",
        "Magic field does not match"
    );
    assert!(
        recovery_slice.length > 0,
        "Length field should be greater than 0"
    );
}

#[test]
fn test_creator_packet_start() {
    let file_path = Path::new("test/fixtures/testfile.par2");
    let mut file = File::open(file_path).expect("Failed to open test file");
    file.read_le::<MainPacket>()
        .expect("Failed to read MainPacket");
    let creator_packet: CreatorPacket = file.read_le().expect("Failed to read CreatorPacket");
    assert_eq!(
        &creator_packet.magic, b"PAR2\0PKT",
        "Magic field does not match"
    );
    assert!(
        creator_packet.length > 0,
        "Length field should be greater than 0"
    );
}
