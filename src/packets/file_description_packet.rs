use binrw::BinRead;

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0FileDesc\0";

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct FileDescriptionPacket {
    pub length: u64,   // Length of the packet
    pub md5: [u8; 16], // MD5 hash of the packet
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    pub file_id: [u8; 16], // Unique identifier for the file
    pub md5_hash: [u8; 16], // MD5 hash of the entire file
    pub md5_16k: [u8; 16], // MD5 hash of the first 16kB of the file
    pub file_length: u64, // Length of the file
    #[br(count = length - 120, map = |v: Vec<u8>| {
        v.into_iter().take_while(|&b| b != 0).collect::<Vec<u8>>()
    })]
    pub file_name: Vec<u8>, // Name of the file (null bytes trimmed during parsing)
}

impl FileDescriptionPacket {
    /// Verifies the MD5 hash of the packet.
    /// Computes the MD5 hash of the serialized fields and compares it to the stored MD5 value.
    ///
    /// A doctest for testing the `verify` method of `FileDescriptionPacket`.
    ///
    /// ```rust
    /// use std::fs::File;
    /// use std::path::Path;
    /// use binrw::BinReaderExt;
    /// use par2rs::packets::file_description_packet::FileDescriptionPacket;
    ///
    /// let file_path = Path::new("tests/fixtures/packets/FileDescriptionPacket.par2");
    /// assert!(file_path.exists(), "Test file does not exist: {:?}", file_path);
    ///
    /// let mut file = File::open(file_path).expect("Failed to open test file");
    /// let packet: FileDescriptionPacket = file.read_le().expect("Failed to read FileDescriptionPacket");
    ///
    /// // Ensure the file name is trimmed correctly
    /// let file_name = String::from_utf8_lossy(&packet.file_name).trim_end_matches('\0').to_string();
    /// assert_eq!(file_name, "testfile", "File name mismatch");
    ///
    /// // Validate the file length
    /// assert_eq!(packet.file_length, 1048576, "File length mismatch");
    ///
    /// // Verify the File ID
    /// let mut id_data = Vec::new();
    /// id_data.extend_from_slice(&packet.md5_16k);
    /// id_data.extend_from_slice(&packet.file_length.to_le_bytes());
    /// id_data.extend_from_slice(file_name.as_bytes());
    /// let expected_file_id = md5::compute(&id_data);
    /// assert_eq!(packet.file_id, expected_file_id.as_ref(), "File ID mismatch");
    /// ```
    pub fn verify(&self) -> bool {
        // Compute the File ID based on the specification
        let mut id_data = Vec::new();
        id_data.extend_from_slice(&self.md5_16k);
        id_data.extend_from_slice(&self.file_length.to_le_bytes());
        id_data.extend_from_slice(&self.file_name);
        let computed_file_id = md5::compute(&id_data);

        // Debug: Log computed File ID
        println!("Computed File ID: {:?}", computed_file_id);

        // Verify the File ID
        if computed_file_id.as_ref() != self.file_id {
            println!(
                "File ID mismatch: expected {:?}, got {:?}",
                self.file_id, computed_file_id
            );
            return false;
        }

        // Compute the MD5 hash of the packet
        let mut packet_data = Vec::new();
        packet_data.extend_from_slice(&self.set_id);
        packet_data.extend_from_slice(TYPE_OF_PACKET);
        packet_data.extend_from_slice(&self.file_id);
        packet_data.extend_from_slice(&self.md5_hash);
        packet_data.extend_from_slice(&self.md5_16k);
        packet_data.extend_from_slice(&self.file_length.to_le_bytes());
        packet_data.extend_from_slice(&self.file_name);

        // Ensure the file name is correctly padded
        if self.file_name.len() < (self.length - 120) as usize {
            let padding = vec![0; (self.length - 120) as usize - self.file_name.len()];
            packet_data.extend_from_slice(&padding);
        }

        // Debug: Log serialized packet data
        println!("Serialized packet data: {:?}", packet_data);

        let computed_md5 = md5::compute(&packet_data);

        // Debug: Log computed MD5 hash
        println!("Computed MD5 hash: {:?}", computed_md5);
        // Verify the MD5 hash
        if computed_md5.as_ref() != self.md5 {
            println!(
                "MD5 hash mismatch: expected {:?}, got {:?}",
                self.md5, computed_md5
            );
            return false;
        }

        true
    }
}
