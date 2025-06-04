use binrw::BinRead;

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0RecvSlic";

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct RecoverySlicePacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    pub exponent: u32, // Exponent used to generate recovery data
    #[br(count = length as usize - (8 + 8 + 16 + 16 + 4))] // Subtract sizes of all other fields
    pub recovery_data: Vec<u8>, // Recovery data
}
