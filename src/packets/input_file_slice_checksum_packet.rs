use binrw::{BinRead, BinWrite};

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0IFSC\0\0\0\0";

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct InputFileSliceChecksumPacket {
    pub length: u64,   // Length of the packet
    pub md5: [u8; 16], // MD5 hash of the packet
    #[br(pad_after = 16)] // Skip the `type_of_packet` field
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    pub file_id: [u8; 16], // File ID of the file
    #[br(count = (length - 64 - 16) / 20)]
    pub slice_checksums: Vec<([u8; 16], u32)>, // MD5 and CRC32 pairs for slices
}

impl InputFileSliceChecksumPacket {
    /// Verifies the MD5 hash of the packet.
    /// Computes the MD5 hash of the serialized fields and compares it to the stored MD5 value.
    ///
    /// A doctest for testing the `verify` method of `InputFileSliceChecksumPacket`.
    ///
    /// ```rust
    /// use std::fs::File;
    /// use binrw::BinReaderExt;
    /// use par2rs::packets::input_file_slice_checksum_packet::InputFileSliceChecksumPacket;
    ///
    /// let mut file = File::open("tests/fixtures/packets/InputFileSliceChecksumPacket.par2").unwrap();
    /// let packet: InputFileSliceChecksumPacket = file.read_le().unwrap();
    ///
    /// assert!(packet.verify(), "MD5 verification failed for InputFileSliceChecksumPacket");
    /// ```
    pub fn verify(&self) -> bool {
        if self.length < 64 {
            println!("Invalid packet length: {}", self.length);
            return false;
        }
        let mut data = Vec::new();
        data.extend_from_slice(&self.set_id);
        data.extend_from_slice(TYPE_OF_PACKET);
        data.extend_from_slice(&self.file_id);
        for (md5, crc32) in &self.slice_checksums {
            data.extend_from_slice(md5);
            data.extend_from_slice(&crc32.to_le_bytes());
        }
        let computed_md5 = md5::compute(&data);
        if computed_md5.as_ref() != self.md5 {
            println!("MD5 mismatch: computed {:?}, expected {:?}", computed_md5, self.md5);
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

impl BinWrite for InputFileSliceChecksumPacket {
    type Args<'a> = ();

    fn write_options<W: std::io::Write + std::io::Seek>(
        &self,
        writer: &mut W,
        _endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        writer.write_all(super::MAGIC_BYTES)?;
        writer.write_all(&self.length.to_le_bytes())?;
        writer.write_all(&self.md5)?;
        writer.write_all(&self.set_id)?;
        writer.write_all(TYPE_OF_PACKET)?;
        writer.write_all(&self.file_id)?;
        for (md5, crc32) in &self.slice_checksums {
            writer.write_all(md5)?;
            writer.write_all(&crc32.to_le_bytes())?;
        }
        Ok(())
    }
}
