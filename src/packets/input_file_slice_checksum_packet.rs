
use binrw::{BinRead, BinWrite};

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0IFSC\0\0\0\0";

#[derive(Debug)]
pub struct InputFileSliceChecksumPacket {
    pub length: u64,                           // Length of the packet
    pub md5: [u8; 16],                         // MD5 hash of the packet
    pub set_id: [u8; 16],                      // Unique identifier for the PAR2 set
    pub file_id: [u8; 16],                     // File ID of the file
    pub slice_checksums: Vec<([u8; 16], u32)>, // MD5 and CRC32 pairs for slices
}

impl BinRead for InputFileSliceChecksumPacket {
    type Args<'a> = ();

    fn read_options<R: std::io::Read + std::io::Seek>(
        reader: &mut R,
        _endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        // OPTIMIZED: Read header in one bulk operation
        let mut header = [0u8; 64];
        reader
            .read_exact(&mut header)
            .map_err(binrw::Error::Io)?;

        // Verify magic
        if &header[0..8] != b"PAR2\0PKT" {
            return Err(binrw::Error::AssertFail {
                pos: 0,
                message: "Invalid magic".to_string(),
            });
        }

        let length = u64::from_le_bytes(header[8..16].try_into().unwrap());
        let mut md5 = [0u8; 16];
        md5.copy_from_slice(&header[16..32]);
        let mut set_id = [0u8; 16];
        set_id.copy_from_slice(&header[32..48]);
        // Skip type_of_packet at 48..64

        // Read file_id
        let mut file_id = [0u8; 16];
        reader
            .read_exact(&mut file_id)
            .map_err(binrw::Error::Io)?;

        // Calculate number of checksums and read them in bulk
        let num_checksums = ((length - 64 - 16) / 20) as usize;
        let checksum_bytes = num_checksums * 20;
        let mut buffer = vec![0u8; checksum_bytes];
        reader
            .read_exact(&mut buffer)
            .map_err(binrw::Error::Io)?;

        // Parse checksums from buffer using unsafe for speed
        let mut slice_checksums = Vec::with_capacity(num_checksums);
        unsafe {
            let ptr = buffer.as_ptr();
            for i in 0..num_checksums {
                let offset = i * 20;
                let mut md5 = [0u8; 16];
                std::ptr::copy_nonoverlapping(ptr.add(offset), md5.as_mut_ptr(), 16);
                let crc32 = u32::from_le_bytes([
                    *ptr.add(offset + 16),
                    *ptr.add(offset + 17),
                    *ptr.add(offset + 18),
                    *ptr.add(offset + 19),
                ]);
                slice_checksums.push((md5, crc32));
            }
        }

        Ok(InputFileSliceChecksumPacket {
            length,
            md5,
            set_id,
            file_id,
            slice_checksums,
        })
    }
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
        use md5::Digest;
        let computed_md5: [u8; 16] = md5::Md5::digest(&data).into();
        if computed_md5 != self.md5 {
            println!(
                "MD5 mismatch: computed {:?}, expected {:?}",
                computed_md5, self.md5
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
