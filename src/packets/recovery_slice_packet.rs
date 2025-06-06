use binrw::{BinRead, BinWrite};

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0RecvSlic";

#[derive(Debug, BinRead, BinWrite)]
#[br(magic = b"PAR2\0PKT")]
pub struct RecoverySlicePacket {
    pub length: u64,   // Length of the packet
    pub md5: [u8; 16], // MD5 hash of the packet
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    pub exponent: u32, // Exponent used to generate recovery data
    #[br(count = length as usize - (8 + 8 + 16 + 16 + 16 + 4))]
    // Include the type_of_packet field (16 bytes) in the calculation
    pub recovery_data: Vec<u8>, // Recovery data
}

impl RecoverySlicePacket {
    /// Verifies the MD5 hash of the packet.
    /// Computes the MD5 hash of the serialized fields and compares it to the stored MD5 value.
    ///
    /// A doctest for testing the `verify` method of `RecoverySlicePacket`.
    ///
    /// ```rust
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
        if self.length < 64 {
            println!("Invalid packet length: {}", self.length);
            return false;
        }
        let mut data = Vec::new();
        data.extend_from_slice(&self.set_id);
        data.extend_from_slice(TYPE_OF_PACKET);
        data.extend_from_slice(&self.exponent.to_le_bytes());
        data.extend_from_slice(&self.recovery_data);
        let computed_md5 = md5::compute(&data);
        if computed_md5.as_ref() != self.md5 {
            println!("MD5 verification failed");
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
