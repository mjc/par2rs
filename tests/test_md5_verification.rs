use binrw::BinReaderExt;
use par2rs::packets::main_packet::MainPacket;
use std::fs::File;

#[test]
fn test_md5_verification() {
    let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
    let main_packet: MainPacket = file.read_le().unwrap();

    let expected_md5 = [
        0xbb, 0xcf, 0x29, 0x18, 0x55, 0x6d, 0x0c, 0xd3, 0xaf, 0xe9, 0x0a, 0xb5, 0x12, 0x3c, 0x3f,
        0xac,
    ];

    assert_eq!(main_packet.md5, expected_md5, "MD5 mismatch");
}
