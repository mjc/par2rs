use binrw::BinRead;

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0RecvSlic";

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct RecoverySlicePacket {
    pub length: u64,   // Length of the packet
    pub md5: [u8; 16], // MD5 hash of the packet
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    pub exponent: u32, // Exponent used to generate recovery data
    #[br(count = length as usize - (8 + 8 + 16 + 16 + 4))] // Subtract sizes of all other fields
    pub recovery_data: Vec<u8>, // Recovery data
}

impl RecoverySlicePacket {
    /// Verifies the MD5 hash of the packet.
    /// Computes the MD5 hash of the serialized fields and compares it to the stored MD5 value.
    ///
    /// A doctest for testing the `verify` method of `RecoverySlicePacket`.
    ///
    /// ```rust,ignore
    /// use std::fs::File;
    /// use binrw::BinReaderExt;
    /// use par2rs::packets::recovery_slice_packet::RecoverySlicePacket;
    ///
    /// // let mut file = File::open("tests/fixtures/packets/RecoverySlicePacket.par2").unwrap();
    /// // let packet: RecoverySlicePacket = file.read_le().unwrap();
    ///
    /// // assert!(packet.verify(), "MD5 verification failed for RecoverySlicePacket");
    /// ```
    pub fn verify(&self) -> bool {
        let mut data = Vec::new();
        data.extend_from_slice(&self.set_id);
        data.extend_from_slice(TYPE_OF_PACKET);
        data.extend_from_slice(&self.exponent.to_le_bytes());
        data.extend_from_slice(&self.recovery_data);
        let computed_md5 = md5::compute(&data);
        computed_md5.as_ref() == self.md5
    }
}
