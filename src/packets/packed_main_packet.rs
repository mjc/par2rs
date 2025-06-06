use binrw::{BinRead, BinWrite};

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0PkdMain\0";

#[derive(Debug, BinRead, BinWrite)]
#[br(magic = b"PAR2\0PKT")]
pub struct PackedMainPacket {
    pub length: u64,   // Length of the packet
    pub md5: [u8; 16], // MD5 hash of the packet
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    pub subslice_size: u64, // Subslice size. Must be a multiple of 4 and equally divide the slice size.
    pub slice_size: u64, // Slice size. Must be a multiple of 4 and a multiple of the subslice size.
    pub file_count: u32, // Number of files in the recovery set.
    #[br(count = file_count)]
    pub recovery_set_ids: Vec<[u8; 16]>, // File IDs of all files in the recovery set.
    #[br(count = (length as usize - 64 - 8 - 8 - 4 - (file_count as usize * 16)) / 16)]
    pub non_recovery_set_ids: Vec<[u8; 16]>, // File IDs of all files in the non-recovery set.
}

impl PackedMainPacket {
    /// Verifies the MD5 hash of the packet.
    /// Computes the MD5 hash of the serialized fields and compares it to the stored MD5 value.
    ///
    /// A doctest for testing the `verify` method of `PackedMainPacket`.
    ///
    /// ```rust
    /// use std::fs::File;
    /// use binrw::BinReaderExt;
    /// use par2rs::packets::packed_main_packet::PackedMainPacket;
    ///
    /// // let mut file = File::open("tests/fixtures/packets/PackedMainPacket.par2").unwrap();
    /// // let packet: PackedMainPacket = file.read_le().unwrap();
    ///
    /// // assert!(packet.verify(), "MD5 verification failed for PackedMainPacket");
    /// ```
    pub fn verify(&self) -> bool {
        if self.length < 64 {
            println!("Invalid packet length: {}", self.length);
            return false;
        }
        let mut data = Vec::new();
        data.extend_from_slice(&self.set_id);
        data.extend_from_slice(TYPE_OF_PACKET);
        data.extend_from_slice(&self.subslice_size.to_le_bytes());
        data.extend_from_slice(&self.slice_size.to_le_bytes());
        data.extend_from_slice(&self.file_count.to_le_bytes());
        for id in &self.recovery_set_ids {
            data.extend_from_slice(id);
        }
        for id in &self.non_recovery_set_ids {
            data.extend_from_slice(id);
        }
        let computed_md5 = md5::compute(&data);
        if computed_md5.as_ref() != self.md5 {
            return false;
        }

        // Check that BinWrite output matches the packet length
        let mut buffer = std::io::Cursor::new(Vec::new());
        if self.write_le(&mut buffer).is_err() {
            println!("Failed to serialize packet");
            return false;
        }

        let serialized_length = buffer.get_ref().len() as u64;
        if serialized_length != self.length {
            println!(
                "Serialized length mismatch: expected {}, got {}",
                self.length, serialized_length
            );
            return false;
        }

        true
    }
}
