use binrw::{BinRead, BinWrite};
use crate::repair::{Md5Hash, RecoverySetId};

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0Creator\0";

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]                             // Reverted to using the literal value
pub struct CreatorPacket {
    pub length: u64,                                    // Length of the packet
    #[br(map = |x: [u8; 16]| Md5Hash::new(x))]
    pub md5: Md5Hash,                                   // MD5 hash of the packet
    #[br(pad_after = 16)]                               // Skip the `type_of_packet` field
    #[br(map = |x: [u8; 16]| RecoverySetId::new(x))]
    pub set_id: RecoverySetId,                          // Unique identifier for the PAR2 set
    #[br(count = length as usize - (8 + 8 + 16 + 16 + 16))]
    pub creator_info: Vec<u8>,                          // ASCII text identifying the client
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
        if self.length < 64 {
            println!("Invalid packet length: {}", self.length);
            return false;
        }
        let mut data = Vec::new();
        data.extend_from_slice(self.set_id.as_bytes());
        data.extend_from_slice(TYPE_OF_PACKET);
        data.extend_from_slice(&self.creator_info);
        use md5::Digest;
        let computed_md5: [u8; 16] = md5::Md5::digest(&data).into();
        if computed_md5 != *self.md5.as_bytes() {
            println!(
                "MD5 mismatch: expected {:?}, computed {:?}",
                self.md5.as_bytes(), computed_md5
            );
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
            println!("Serialized data: {:?}", buffer.get_ref()); // Debugging serialized data
            return false;
        }

        true
    }
}

impl BinWrite for CreatorPacket {
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
        writer.write_all(self.md5.as_bytes())?;

        // Write the set_id field
        writer.write_all(self.set_id.as_bytes())?;

        // Write the type of packet (TYPE_OF_PACKET)
        writer.write_all(TYPE_OF_PACKET)?;

        // Write the creator_info field
        writer.write_all(&self.creator_info)?;

        Ok(())
    }
}
