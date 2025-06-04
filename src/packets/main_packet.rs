use binrw::{BinRead, BinWrite};

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0Main\0\0\0\0";

#[derive(Debug, BinRead)]
#[br(magic = b"PAR2\0PKT")]
/// A doctest for testing the `MainPacket` structure with `binread`.
///
/// ```rust
/// use std::fs::File;
/// use binrw::BinReaderExt;
/// use par2rs::packets::main_packet::MainPacket;
///
/// let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
/// let main_packet: MainPacket = file.read_le().unwrap();
///
/// assert_eq!(main_packet.length, 92); // Updated assertion
/// assert_eq!(main_packet.file_ids.len(), 1); // Updated assertion
/// ```
pub struct MainPacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet
    #[br(pad_after = 16)] // Ensure proper alignment for the `slice_size` field
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    pub slice_size: u64, // Size of each slice
    pub file_count: u32, // Number of files in the recovery set
    #[br(count = (length - 72) / 16)]
    pub file_ids: Vec<[u8; 16]>, // File IDs of all files in the recovery set
    #[br(count = (length - 72 - (file_ids.len() as u64 * 16)) / 16)]
    pub non_recovery_file_ids: Vec<[u8; 16]>, // File IDs of all files in the non-recovery set
}

/// A doctest for testing the `BinWrite` implementation of `MainPacket`.
///
/// ```rust
/// use std::fs::File;
/// use std::io::Cursor;
/// use binrw::{BinWriterExt, BinWrite};
/// use par2rs::packets::main_packet::MainPacket;
///
/// let main_packet = MainPacket {
///     length: 92,
///     md5: [0; 16],
///     set_id: [0; 16],
///     slice_size: 1024,
///     file_count: 1,
///     file_ids: vec![[0; 16]],
///     non_recovery_file_ids: vec![],
/// };
///
/// let mut buffer = Cursor::new(Vec::new());
/// main_packet.write_le(&mut buffer).unwrap();
///
/// let expected = std::fs::read("tests/fixtures/packets/MainPacket.par2").unwrap();
/// let actual = buffer.into_inner();
///
/// for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
///     if a != e {
///         println!("Byte mismatch at position {}: actual = {}, expected = {}", i, a, e);
///     }
/// }
/// ```
impl BinWrite for MainPacket {
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
        writer.write_all(&self.slice_size.to_le_bytes())?;
        for file_id in &self.file_ids {
            writer.write_all(file_id)?;
        }
        for non_recovery_file_id in &self.non_recovery_file_ids {
            writer.write_all(non_recovery_file_id)?;
        }
        Ok(())
    }
}

impl MainPacket {
    /// Verifies the MD5 hash of the packet.
    /// Computes the MD5 hash of the serialized fields and compares it to the stored MD5 value.
    ///
    /// A doctest for testing the `verify` method of `MainPacket`.
    ///
    /// ```rust
    /// use std::fs::File;
    /// use binrw::BinReaderExt;
    /// use par2rs::packets::main_packet::MainPacket;
    ///
    /// let mut file = File::open("tests/fixtures/packets/MainPacket.par2").unwrap();
    /// let main_packet: MainPacket = file.read_le().unwrap();
    ///
    /// assert!(main_packet.verify(), "MD5 verification failed");
    /// ```
    pub fn verify(&self) -> bool {
        // Serialize fields to compute the hash
        let mut data = Vec::new();

        // Exclude header fields (MAGIC_BYTES, length, md5)
        data.extend_from_slice(&self.set_id);
        data.extend_from_slice(TYPE_OF_PACKET);
        data.extend_from_slice(&self.slice_size.to_le_bytes());
        data.extend_from_slice(&self.file_count.to_le_bytes());
        for file_id in &self.file_ids {
            data.extend_from_slice(file_id);
        }
        for non_recovery_file_id in &self.non_recovery_file_ids {
            data.extend_from_slice(non_recovery_file_id);
        }

        // Compute MD5 hash and compare with stored MD5
        let computed_md5 = md5::compute(&data);
        computed_md5.as_ref() == self.md5
    }
}
