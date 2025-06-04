use binrw::BinRead;

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0FileDesc\0";

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct FileDescriptionPacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    pub file_id: [u8; 16], // Unique identifier for the file
    pub md5_hash: [u8; 16], // MD5 hash of the entire file
    pub md5_16k: [u8; 16], // MD5 hash of the first 16kB of the file
    pub file_length: u64, // Length of the file
    #[br(count = length - 120)] // Adjusted count to account for removed magic field
    pub file_name: Vec<u8>, // Name of the file (not null-terminated)
}

impl FileDescriptionPacket {
    /// Verifies the MD5 hash of the packet.
    /// Computes the MD5 hash of the serialized fields and compares it to the stored MD5 value.
    ///
    /// A doctest for testing the `verify` method of `FileDescriptionPacket`.
    ///
    /// ```rust
    /// use std::fs::File;
    /// use binrw::BinReaderExt;
    /// use par2rs::packets::file_description_packet::FileDescriptionPacket;
    ///
    /// let mut file = File::open("tests/fixtures/packets/FileDescriptionPacket.par2").unwrap();
    /// let packet: FileDescriptionPacket = file.read_le().unwrap();
    ///
    /// assert!(packet.verify(), "MD5 verification failed for FileDescriptionPacket");
    /// ```
    pub fn verify(&self) -> bool {
        let mut data = Vec::new();
        data.extend_from_slice(&self.set_id);
        data.extend_from_slice(TYPE_OF_PACKET);
        data.extend_from_slice(&self.file_id);
        data.extend_from_slice(&self.md5_hash);
        data.extend_from_slice(&self.md5_16k);
        data.extend_from_slice(&self.file_length.to_le_bytes());
        data.extend_from_slice(&self.file_name);
        let computed_md5 = md5::compute(&data);
        computed_md5.as_ref() == self.md5
    }
}
