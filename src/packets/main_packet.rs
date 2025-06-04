use binrw::BinRead;

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0Main\0\0\0\0";

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
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
