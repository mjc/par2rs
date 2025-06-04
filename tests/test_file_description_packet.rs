use binrw::BinReaderExt;
use par2rs::packets::file_description_packet::FileDescriptionPacket;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

#[test]
fn test_file_description_packet_fields() {
    let mut file = File::open("tests/fixtures/packets/FileDescriptionPacket.par2").unwrap();

    let mut buffer = [0u8; 8];
    file.read_exact(&mut buffer).unwrap();
    assert_eq!(&buffer, b"PAR2\0PKT", "Magic bytes mismatch");

    let mut buffer = [0u8; 8];
    file.read_exact(&mut buffer).unwrap();
    let expected_length = u64::from_le_bytes(buffer);
    assert_eq!(
        expected_length.to_le_bytes(),
        [0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        "Expected length mismatch"
    );

    let mut buffer = [0u8; 16];
    file.read_exact(&mut buffer).unwrap();
    let expected_md5 = buffer;
    assert_eq!(
        expected_md5,
        [
            0x09, 0xa5, 0xe9, 0xd0, 0x88, 0xff, 0xb9, 0xa0, 0xf7, 0x24, 0xc5, 0x31, 0xde, 0xd7,
            0x4f, 0x52
        ],
        "MD5 mismatch"
    );

    let mut buffer = [0u8; 16];
    file.read_exact(&mut buffer).unwrap();
    let expected_set_id = buffer;
    assert_eq!(
        buffer,
        [
            0x64, 0x32, 0x80, 0xa0, 0x12, 0xea, 0xe7, 0xfe, 0xfb, 0xa0, 0x54, 0x72, 0x61, 0xdf,
            0xcd, 0xf3
        ],
        "Set ID mismatch"
    );

    let mut buffer = [0u8; 16];
    file.read_exact(&mut buffer).unwrap();

    println!("Type of packet: {:?}", String::from_utf8_lossy(&buffer));
    assert_eq!(&buffer, b"PAR 2.0\0FileDesc", "Type of packet mismatch");

    let mut buffer = [0u8; 16];
    file.read_exact(&mut buffer).unwrap();
    let expected_file_id = buffer;
    assert_eq!(
        expected_file_id,
        [
            0x87, 0x42, 0x70, 0xa6, 0x34, 0xd2, 0x77, 0xf8, 0x8c, 0x0e, 0x0b, 0x25, 0x85, 0x17,
            0xc2, 0x63
        ],
        "File ID mismatch"
    );

    let mut buffer = [0u8; 16];
    file.read_exact(&mut buffer).unwrap();
    let expected_md5_hash = buffer;
    assert_eq!(
        expected_md5_hash,
        [
            0x25, 0x62, 0xcb, 0xea, 0x92, 0x74, 0x14, 0xec, 0xba, 0xce, 0x1d, 0x91, 0x3b, 0x18,
            0xde, 0xcf
        ],
        "full MD5 hash mismatch"
    );

    let mut buffer = [0u8; 16];
    file.read_exact(&mut buffer).unwrap();
    let expected_md5_16k = buffer;
    assert_eq!(
        expected_md5_16k,
        [
            0x56, 0x2F, 0xAF, 0x48, 0x65, 0xD2, 0x28, 0x5D, 0x2F, 0xA8, 0x81, 0x34, 0xED, 0x5B,
            0xDF, 0x3D
        ],
        "MD5 of first 16kB mismatch"
    );

    let mut buffer = [0u8; 8];
    file.read_exact(&mut buffer).unwrap();
    let expected_file_length = u64::from_le_bytes(buffer);
    assert_eq!(
        expected_file_length.to_le_bytes(),
        [0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00],
        "File length mismatch"
    );

    let mut file_name_buffer = Vec::new();
    file.read_to_end(&mut file_name_buffer).unwrap();
    let expected_file_name = file_name_buffer;
    assert_eq!(expected_file_name, b"testfile", "File name mismatch");

    file.seek(SeekFrom::Start(0)).unwrap(); // Reset file position for BinRead
    let file_description_packet: FileDescriptionPacket = file.read_le().unwrap();

    // Assertions
    assert_eq!(
        file_description_packet.length, expected_length,
        "Length mismatch"
    );
    assert_eq!(file_description_packet.md5, expected_md5, "MD5 mismatch");
    assert_eq!(
        file_description_packet.set_id, expected_set_id,
        "Set ID mismatch"
    );
    assert_eq!(
        file_description_packet.file_length, expected_file_length,
        "File length mismatch"
    );
    assert_eq!(
        file_description_packet.file_name, expected_file_name,
        "File name mismatch"
    );
}
