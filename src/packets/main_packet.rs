use binrw::BinRead;

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0Main\0\0\0\0";

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
/// A doctest for testing the `MainPacket` structure with `binread`.
///
/// ```rust
/// use std::fs::File;
/// use binrw::BinReaderExt;
/// use par2rs::packets::main_packet::MainPacket;
///
/// let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
/// let main_packet: MainPacket = file.read_le().unwrap();
///
/// assert_eq!(main_packet.length, 92); // Updated assertion
/// assert_eq!(main_packet.file_ids.len(), 1); // Updated assertion
/// ```
pub struct MainPacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
    pub slice_size: u64, // Size of each slice
    #[br(count = (length - 72) / 16)]
    pub file_ids: Vec<[u8; 16]>, // File IDs of all files in the recovery set
    #[br(count = (length - 72 - (file_ids.len() as u64 * 16)) / 16)]
    pub non_recovery_file_ids: Vec<[u8; 16]>, // File IDs of all files in the non-recovery set
}
