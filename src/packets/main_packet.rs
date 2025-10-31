use std::fmt::{Debug, Display};

use crate::domain::{FileId, Md5Hash, RecoverySetId};
use binrw::{BinRead, BinWrite};

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0Main\0\0\0\0";

#[derive(Clone, BinRead)]
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
    pub length: u64, // Length of the packet
    #[br(map = |x: [u8; 16]| Md5Hash::new(x))]
    pub md5: Md5Hash, // MD5 hash of the packet
    #[br(pad_after = 16)] // Ensure proper alignment for the `slice_size` field
    #[br(map = |x: [u8; 16]| RecoverySetId::new(x))]
    pub set_id: RecoverySetId, // Unique identifier for the PAR2 set
    pub slice_size: u64, // Size of each slice
    pub file_count: u32, // Number of files in the recovery set
    #[br(count = (length - 72) / 16)]
    #[br(map = |v: Vec<[u8; 16]>| v.into_iter().map(FileId::new).collect())]
    pub file_ids: Vec<FileId>, // File IDs of all files in the recovery set
    #[br(count = (length - 72 - (file_ids.len() as u64 * 16)) / 16)]
    #[br(map = |v: Vec<[u8; 16]>| v.into_iter().map(FileId::new).collect())]
    pub non_recovery_file_ids: Vec<FileId>, // File IDs of all files in the non-recovery set
}

/// A doctest for testing the `BinWrite` implementation of `MainPacket`.
///
/// ```rust
/// use std::fs::File;
/// use std::io::Cursor;
/// use binrw::{BinWriterExt, BinWrite};
/// use par2rs::packets::main_packet::MainPacket;
/// use par2rs::domain::{Md5Hash, RecoverySetId, FileId};
///
/// let main_packet = MainPacket {
///     length: 92,
///     md5: Md5Hash::new([0; 16]),
///     set_id: RecoverySetId::new([0; 16]),
///     slice_size: 1024,
///     file_count: 1,
///     file_ids: vec![FileId::new([0; 16])],
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
        writer.write_all(self.md5.as_bytes())?;
        writer.write_all(self.set_id.as_bytes())?;
        writer.write_all(TYPE_OF_PACKET)?;

        writer.write_all(&self.slice_size.to_le_bytes())?;
        writer.write_all(&self.file_count.to_le_bytes())?;
        for file_id in &self.file_ids {
            writer.write_all(file_id.as_bytes())?;
        }
        for non_recovery_file_id in &self.non_recovery_file_ids {
            writer.write_all(non_recovery_file_id.as_bytes())?;
        }
        Ok(())
    }
}

impl Display for MainPacket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn fmt_md5(md5: &[u8; 16]) -> String {
            format!("{:x?}", u128::from_le_bytes(*md5))
        }

        write!(
            f,
            "MainPacket {{ length: {}, md5: {}, set_id: {}, slice_size: {}, file_count: {}, file_ids: {:?}, non_recovery_file_ids: {:?} }}",
            self.length, fmt_md5(self.md5.as_bytes()), fmt_md5(self.set_id.as_bytes()), self.slice_size, self.file_count, self.file_ids.iter().map(|f| fmt_md5(f.as_bytes())).collect::<Vec<_>>(), self.non_recovery_file_ids.iter().map(|f| fmt_md5(f.as_bytes())).collect::<Vec<_>>()
        )
    }
}

impl Debug for MainPacket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, f)
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
        if self.length < 72 {
            println!("Invalid packet length: {}", self.length);
            return false;
        }

        // Serialize fields to compute the hash
        let mut data = Vec::new();

        // Exclude header fields (MAGIC_BYTES, length, md5)
        data.extend_from_slice(self.set_id.as_bytes());
        data.extend_from_slice(TYPE_OF_PACKET);
        data.extend_from_slice(&self.slice_size.to_le_bytes());
        data.extend_from_slice(&self.file_count.to_le_bytes());
        for file_id in &self.file_ids {
            data.extend_from_slice(file_id.as_bytes());
        }
        for non_recovery_file_id in &self.non_recovery_file_ids {
            data.extend_from_slice(non_recovery_file_id.as_bytes());
        }

        // Compute MD5 hash and compare with stored MD5
        let computed_md5 = crate::checksum::compute_md5_bytes(&data);
        if computed_md5 != *self.md5.as_bytes() {
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
