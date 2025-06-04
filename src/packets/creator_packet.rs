use binrw::BinRead;

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0Creator\0";

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")] // Reverted to using the literal value
pub struct CreatorPacket {
    pub length: u64,   // Length of the packet
    pub md5: [u8; 16], // MD5 hash of the packet
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(count = length as usize - (8 + 8 + 16 + 16 + 16))]
    pub creator_info: Vec<u8>, // ASCII text identifying the client
}

impl CreatorPacket {
    /// Verifies the MD5 hash of the packet.
    /// Computes the MD5 hash of the serialized fields and compares it to the stored MD5 value.
    ///
    /// A doctest for testing the `verify` method of `CreatorPacket`.
    ///
    /// ```rust
    /// use std::fs::File;
    /// use binrw::BinReaderExt;
    /// use par2rs::packets::creator_packet::CreatorPacket;
    ///
    /// let mut file = File::open("tests/fixtures/packets/CreatorPacket.par2").unwrap();
    /// let packet: CreatorPacket = file.read_le().unwrap();
    ///
    /// assert!(packet.verify(), "MD5 verification failed for CreatorPacket");
    /// ```
    pub fn verify(&self) -> bool {
        let mut data = Vec::new();
        data.extend_from_slice(&self.set_id);
        data.extend_from_slice(TYPE_OF_PACKET);
        data.extend_from_slice(&self.creator_info);
        let computed_md5 = md5::compute(&data);
        computed_md5.as_ref() == self.md5
    }
}
