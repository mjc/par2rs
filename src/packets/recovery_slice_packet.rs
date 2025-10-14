
use binrw::{BinRead, BinWrite};

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0RecvSlic";

#[derive(Debug, Clone, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct RecoverySlicePacket {
    pub length: u64,              // Length of the packet
    pub md5: [u8; 16],            // MD5 hash of the packet
    pub set_id: [u8; 16],         // Unique identifier for the PAR2 set
    pub type_of_packet: [u8; 16], // Type of packet - should be "PAR 2.0\0RecvSlic"
    pub exponent: u32,            // Exponent used to generate recovery data
    #[br(count = length as usize - (8 + 8 + 16 + 16 + 16 + 4))]
    // Calculate recovery data size: total length - (magic + length + md5 + set_id + type + exponent)
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
        use md5::Digest;
        let computed_md5: [u8; 16] = md5::Md5::digest(&data).into();
        if computed_md5 != self.md5 {
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

impl BinWrite for RecoverySlicePacket {
    type Args<'a> = ();

    fn write_options<W: std::io::Write + std::io::Seek>(
        &self,
        writer: &mut W,
        _endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        // Write the magic bytes
        writer.write_all(b"PAR2\0PKT")?;

        // Write the length field
        writer.write_all(&self.length.to_le_bytes())?;

        // Write the MD5 hash
        writer.write_all(&self.md5)?;

        // Write the set_id field
        writer.write_all(&self.set_id)?;

        // Write the type of packet
        writer.write_all(&self.type_of_packet)?;

        // Write the exponent
        writer.write_all(&self.exponent.to_le_bytes())?;

        // Write the recovery data
        writer.write_all(&self.recovery_data)?;

        Ok(())
    }
}
