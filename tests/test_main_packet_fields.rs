use binrw::BinReaderExt;
use par2rs::packets::main_packet::MainPacket;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

#[test]
fn test_main_packet_fields() {
    let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();

    let mut buffer = [0u8; 8];
    file.read_exact(&mut buffer).unwrap();
    assert_eq!(&buffer, b"PAR2\0PKT", "Magic bytes mismatch");

    let mut buffer = [0u8; 8];
    file.read_exact(&mut buffer).unwrap();
    let expected_length = u64::from_le_bytes(buffer);
    assert_eq!(
        expected_length.to_le_bytes(),
        [0x5c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        "Expected length mismatch"
    );

    let mut buffer = [0u8; 16];
    file.read_exact(&mut buffer).unwrap();
    let expected_md5 = buffer;
    assert_eq!(
        expected_md5,
        [
            0xbb, 0xcf, 0x29, 0x18, 0x55, 0x6d, 0x0c, 0xd3, 0xaf, 0xe9, 0x0a, 0xb5, 0x12, 0x3c,
            0x3f, 0xac
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
    assert_eq!(
        buffer,
        [
            0x50, 0x41, 0x52, 0x20, 0x32, 0x2e, 0x30, 0x00, 0x4D, 0x61, 0x69, 0x6e, 0x00, 0x00,
            0x00, 0x00
        ],
        "Type of packet mismatch"
    );

    let mut buffer = [0u8; 8];
    file.read_exact(&mut buffer).unwrap();
    let expected_slice_size = u64::from_le_bytes(buffer);
    assert_eq!(
        expected_slice_size.to_le_bytes(),
        [0x10, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        "Slice size mismatch on reading test file"
    );

    let mut buffer = [0u8; 4];
    file.read_exact(&mut buffer).unwrap();
    let expected_file_count = u32::from_le_bytes(buffer);
    assert_eq!(
        expected_file_count.to_le_bytes(),
        [0x01, 0x00, 0x00, 0x00],
        "File count mismatch"
    );

    let mut file_ids = Vec::new();
    let file_ids_count = (expected_length - 72) / 16;
    for _ in 0..file_ids_count {
        let mut buffer = [0u8; 16];
        file.read_exact(&mut buffer).unwrap();
        file_ids.push(buffer);
    }

    assert_eq!(
        file_ids,
        [[
            0x87, 0x42, 0x70, 0xa6, 0x34, 0xd2, 0x77, 0xf8, 0x8c, 0x0e, 0x0b, 0x25, 0x85, 0x17,
            0xc2, 0x63
        ]],
        "File IDs mismatch"
    );

    let mut non_recovery_file_ids = Vec::new();
    let non_recovery_file_ids_count = (expected_length - 72 - (file_ids_count * 16)) / 16;
    for _ in 0..non_recovery_file_ids_count {
        let mut buffer = [0u8; 16];
        file.read_exact(&mut buffer).unwrap();
        non_recovery_file_ids.push(buffer);
    }

    file.seek(SeekFrom::Start(0)).unwrap(); // Reset file position for BinRead
    let main_packet: MainPacket = file.read_le().unwrap();

    // Assertions
    assert_eq!(main_packet.length, expected_length, "Length mismatch");
    assert_eq!(main_packet.md5, expected_md5, "MD5 mismatch");
    assert_eq!(main_packet.set_id, expected_set_id, "Set ID mismatch");
    assert_eq!(
        main_packet.slice_size, expected_slice_size,
        "Slice size mismatch"
    );
    assert_eq!(
        main_packet.file_count, expected_file_count,
        "File count mismatch"
    );
    assert_eq!(main_packet.file_ids, file_ids, "File IDs mismatch");
    assert_eq!(
        main_packet.non_recovery_file_ids, non_recovery_file_ids,
        "Non-recovery File IDs mismatch"
    );
    assert_eq!(
        expected_length, 92,
        "Parsed length does not match the expected value of 92"
    );
}
