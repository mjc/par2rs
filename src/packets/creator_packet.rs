use binrw::BinRead;

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct CreatorPacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
    #[br(count = length as usize - (8 + 8 + 16 + 16 + 16))]
    pub creator_info: Vec<u8>, // ASCII text identifying the client
}
