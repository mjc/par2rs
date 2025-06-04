use binrw::BinReaderExt;
use par2rs::packets::recovery_slice_packet::RecoverySlicePacket;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

#[test]
fn test_recovery_slice_packet_fields() {
    let mut file = File::open("tests/fixtures/packets/RecoverySlicePacket.par2").unwrap();

    let mut buffer = [0u8; 8];
    file.read_exact(&mut buffer).unwrap();
    assert_eq!(&buffer, b"PAR2\0PKT", "Magic bytes mismatch");

    let mut buffer = [0u8; 8];
    file.read_exact(&mut buffer).unwrap();
    let expected_length = u64::from_le_bytes(buffer);
    assert_eq!(expected_length, 596, "Expected length mismatch");

    let mut buffer = [0u8; 16];
    file.read_exact(&mut buffer).unwrap();
    let expected_md5 = buffer;
    assert_eq!(expected_md5, [
        0x91, 0x89, 0xce, 0xb8, 0x19, 0xf0, 0xd5, 0x51,
        0xa9, 0x8a, 0xc7, 0xe6, 0x6c, 0xb9, 0xe6, 0x47
    ], "MD5 mismatch");

    let mut buffer = [0u8; 16];
    file.read_exact(&mut buffer).unwrap();
    let expected_set_id = buffer;
    assert_eq!(expected_set_id, [
        0x64, 0x32, 0x80, 0xa0, 0x12, 0xea, 0xe7, 0xfe,
        0xfb, 0xa0, 0x54, 0x72, 0x61, 0xdf, 0xcd, 0xf3
    ], "Set ID mismatch");

    let mut buffer = [0u8; 16];
    file.read_exact(&mut buffer).unwrap();
    assert_eq!(&buffer, b"PAR 2.0\0RecvSlic", "Type of packet mismatch");

    let mut buffer = [0u8; 4];
    file.read_exact(&mut buffer).unwrap();
    let expected_exponent = u32::from_le_bytes(buffer);
    assert_eq!(expected_exponent, 15, "Exponent mismatch");

    let mut recovery_data = Vec::new();
    file.read_to_end(&mut recovery_data).unwrap();
    assert_eq!(recovery_data,
        [38, 105, 147, 93, 250, 13, 39, 212, 246, 253, 163, 45, 253, 116, 72, 92, 98, 178, 178, 49, 140, 23, 18, 207, 202, 117, 168, 228, 224, 94, 154, 3, 149, 216, 159, 0, 212, 129, 95, 216, 171, 5, 134, 215, 47, 217, 82, 142, 202, 174, 12, 170, 68, 229, 180, 93, 98, 72, 201, 176, 205, 96, 56, 45, 87, 213, 93, 75, 156, 178, 177, 45, 225, 73, 203, 168, 94, 144, 104, 147, 139, 14, 125, 165, 83, 210, 141, 24, 199, 31, 57, 21, 172, 100, 119, 50, 250, 230, 51, 228, 26, 130, 226, 173, 156, 22, 19, 12, 14, 74, 137, 228, 91, 212, 53, 104, 15, 134, 166, 244, 248, 55, 212, 150, 119, 208, 173, 149, 103, 49, 70, 226, 177, 107, 118, 236, 63, 166, 164, 154, 102, 176, 105, 71, 252, 163, 44, 214, 82, 143, 57, 173, 210, 146, 170, 86, 198, 207, 92, 13, 225, 153, 16, 123, 147, 56, 248, 68, 1, 235, 214, 31, 239, 211, 76, 205, 28, 100, 180, 177, 150, 225, 144, 194, 10, 238, 24, 64, 99, 1, 174, 49, 221, 82, 231, 218, 90, 154, 174, 48, 237, 205, 153, 61, 96, 97, 110, 5, 183, 30, 89, 113, 179, 158, 229, 17, 184, 126, 79, 70, 92, 193, 196, 212, 45, 128, 59, 59, 191, 238, 113, 156, 207, 91, 139, 27, 3, 24, 229, 44, 158, 29, 103, 181, 235, 51, 224, 63, 247, 44, 188, 221, 217, 14, 23, 10, 6, 28, 77, 74, 46, 84, 226, 100, 68, 51, 135, 12, 253, 83, 206, 246, 151, 131, 140, 97, 91, 242, 221, 165, 196, 120, 53, 31, 194, 3, 123, 159, 53, 72, 150, 80, 187, 248, 140, 206, 19, 117, 39, 217, 177, 91, 195, 92, 192, 206, 192, 242, 8, 102, 60, 126, 126, 73, 245, 118, 26, 178, 148, 50, 56, 206, 15, 201, 31, 174, 226, 145, 30, 191, 255, 37, 195, 166, 132, 17, 142, 207, 238, 222, 87, 150, 229, 122, 254, 78, 128, 142, 87, 247, 221, 21, 136, 121, 93, 169, 161, 34, 16, 32, 240, 239, 91, 123, 118, 218, 70, 240, 223, 133, 110, 197, 140, 74, 191, 31, 34, 192, 149, 225, 208, 16, 110, 33, 96, 40, 43, 195, 217, 50, 201, 16, 225, 17, 154, 42, 164, 4, 56, 185, 109, 39, 68, 153, 191, 197, 126, 220, 182, 54, 173, 138, 183, 194, 149, 94, 121, 87, 167, 120, 181, 184, 167, 217, 97, 121, 74, 47, 204, 2, 201, 85, 47, 69, 129, 94, 227, 77, 227, 55, 220, 38, 221, 97, 55, 124, 10, 112, 25, 196, 188, 190, 70, 200, 153, 53, 159, 250, 12, 243, 251, 118, 219, 75, 235, 169, 146, 118, 93, 106, 60, 70, 12, 151, 68, 158, 103, 52, 21, 87, 17, 205, 61, 44, 16, 132, 205, 90, 193, 94, 150, 75, 134, 244, 61, 196, 193, 62, 15, 75, 100, 143, 229, 213, 47, 28, 195, 169, 251, 252, 82, 163, 115, 115, 19, 91, 228, 86, 21, 161, 97, 138, 202, 153, 60, 132, 170, 167],
         "Recovery data should not be empty");

    file.seek(SeekFrom::Start(0)).unwrap(); // Reset file position for BinRead
    let recovery_slice_packet: RecoverySlicePacket = file.read_le().unwrap();

    // Assertions
    assert_eq!(recovery_slice_packet.length, expected_length, "Length mismatch");
    assert_eq!(recovery_slice_packet.md5, expected_md5, "MD5 mismatch");
    assert_eq!(recovery_slice_packet.set_id, expected_set_id, "Set ID mismatch");
    assert_eq!(recovery_slice_packet.exponent, expected_exponent, "Exponent mismatch");
    assert_eq!(recovery_slice_packet.recovery_data, recovery_data, "Recovery data mismatch");
}
