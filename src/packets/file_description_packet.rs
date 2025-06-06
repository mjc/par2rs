use binrw::{BinRead, BinWrite};

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0FileDesc";

#[derive(Debug, BinRead, BinWrite)]
#[br(magic = b"PAR2\0PKT")]
pub struct FileDescriptionPacket {
    pub length: u64,   // Length of the packet
    pub md5: [u8; 16], // MD5 hash of the packet type and body
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(assert(packet_type == TYPE_OF_PACKET, "Packet type mismatch for FileDescriptionPacket. Expected {:?}, got {:?}", TYPE_OF_PACKET, packet_type))]
    pub packet_type: [u8; 16], // Type of the packet
    pub file_id: [u8; 16], // Unique identifier for the file
    pub md5_hash: [u8; 16], // MD5 hash of the entire file
    pub md5_16k: [u8; 16], // MD5 hash of the first 16kB of the file
    pub file_length: u64, // Length of the file
    #[br(count = length.saturating_sub(120), map = |v: Vec<u8>| {
        v.into_iter().take_while(|&b| b != 0).collect::<Vec<u8>>()
    })]
    pub file_name: Vec<u8>, // Name of the file (null bytes trimmed during parsing)
}

impl FileDescriptionPacket {
    /// Verifies the MD5 hash of the packet and other invariants.
    /// Computes the MD5 hash of the serialized fields (type + body) and compares it to the stored MD5 value.
    /// Also checks file ID and serialized length consistency.
    ///
    /// A doctest for testing the `verify` method of `FileDescriptionPacket`.
    ///
    /// ```rust
    /// use std::fs::File;
    /// use std::path::Path;
    /// use binrw::BinReaderExt;
    /// use par2rs::packets::file_description_packet::{FileDescriptionPacket, TYPE_OF_PACKET};
    ///
    /// let file_path = Path::new("tests/fixtures/packets/FileDescriptionPacket.par2");
    /// assert!(file_path.exists(), "Test file does not exist: {:?}", file_path);
    ///
    /// let mut file = File::open(file_path).expect("Failed to open test file");
    /// let packet: FileDescriptionPacket = file.read_le().expect("Failed to read FileDescriptionPacket");
    ///
    /// // Ensure the packet_type field is correctly read (it's asserted by BinRead)
    /// assert_eq!(packet.packet_type, TYPE_OF_PACKET, "Packet type field mismatch");
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
    /// id_data.extend_from_slice(file_name.as_bytes()); // Use trimmed file name for File ID
    /// let expected_file_id = md5::compute(&id_data);
    /// assert_eq!(packet.file_id, expected_file_id.as_ref(), "File ID mismatch");
    ///
    /// // Full verification using the method
    /// assert!(packet.verify(), "Packet verification failed");
    /// ```
    pub fn verify(&self) -> bool {
        if self.length < 120 { // Minimum size for a FileDescriptionPacket (including all headers and fixed fields)
            println!("Invalid packet length: {}", self.length);
            return false;
        }

        // Verify Packet Type (already asserted by BinRead, but good for standalone verify)
        if self.packet_type != TYPE_OF_PACKET {
            println!("Packet type field mismatch in verify: expected {:?}, got {:?}", TYPE_OF_PACKET, self.packet_type);
            return false;
        }

        // Compute the File ID based on the specification
        let mut id_data = Vec::new();
        id_data.extend_from_slice(&self.md5_16k);
        id_data.extend_from_slice(&self.file_length.to_le_bytes());
        // Per PAR2 spec, File ID uses the filename as stored (which might include padding in some contexts,
        // but here self.file_name is trimmed. The doctest implies trimmed name is used for ID calc).
        // For consistency with doctest and common practice:
        let trimmed_file_name_for_id = self.file_name.iter().take_while(|&&b| b != 0).cloned().collect::<Vec<u8>>();
        id_data.extend_from_slice(&trimmed_file_name_for_id);
        let computed_file_id = md5::compute(&id_data);

        // Debug: Log computed File ID
        // println!("Computed File ID: {:?}", computed_file_id);

        // Verify the File ID
        if computed_file_id.as_ref() != self.file_id {
            println!(
                "File ID mismatch: expected {:?}, got {:?}",
                self.file_id, computed_file_id
            );
            return false;
        }

        // Compute the MD5 hash of the (packet type + packet body)
        let mut packet_data_for_md5 = Vec::new();
        packet_data_for_md5.extend_from_slice(&self.packet_type); // Packet Type field first
        packet_data_for_md5.extend_from_slice(&self.set_id);
        packet_data_for_md5.extend_from_slice(&self.file_id);
        packet_data_for_md5.extend_from_slice(&self.md5_hash);
        packet_data_for_md5.extend_from_slice(&self.md5_16k);
        packet_data_for_md5.extend_from_slice(&self.file_length.to_le_bytes());
        packet_data_for_md5.extend_from_slice(&self.file_name); // The actual file name data (trimmed by BinRead map)

        // Ensure the file name is correctly padded for MD5 calculation to match on-disk representation
        // The on-disk file name field has length `self.length - 120`.
        let expected_on_disk_file_name_len = self.length.saturating_sub(120) as usize;
        if self.file_name.len() < expected_on_disk_file_name_len {
            let padding_len = expected_on_disk_file_name_len - self.file_name.len();
            let padding = vec![0; padding_len];
            packet_data_for_md5.extend_from_slice(&padding);
        }
        // If self.file_name.len() > expected_on_disk_file_name_len, it's an inconsistency,
        // as BinRead's `count` should have limited it. Assuming count is correct.

        // Debug: Log serialized packet data for MD5
        // println!("Packet data for MD5 (len {}): {:?}", packet_data_for_md5.len(), packet_data_for_md5);

        let computed_md5 = md5::compute(&packet_data_for_md5);

        // Debug: Log computed MD5 hash
        // println!("Computed MD5 hash: {:?}", computed_md5);
        // Verify the MD5 hash
        if computed_md5.as_ref() != self.md5 {
            println!(
                "MD5 hash mismatch: expected {:?}, got {:?}",
                self.md5, computed_md5
            );
            return false;
        }

        // Check that BinWrite output (struct fields) length is consistent with self.length
        // self.length is total packet size (magic + fields).
        // Serialized struct fields exclude magic (8 bytes).
        let mut buffer = std::io::Cursor::new(Vec::new());
        if self.write_le(&mut buffer).is_err() {
            println!("Failed to serialize packet for length check");
            return false;
        }

        let serialized_struct_fields_length = buffer.get_ref().len() as u64;
        if serialized_struct_fields_length + 8 != self.length { // Add 8 for magic
            println!(
                "Serialized length consistency check failed: struct fields length {} + magic 8 != total packet length {}. Expected struct fields length: {}",
                serialized_struct_fields_length, self.length, self.length.saturating_sub(8)
            );
            return false;
        }

        true
    }
}
