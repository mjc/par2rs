use crate::domain::{FileId, Md5Hash, RecoverySetId};
use binrw::{BinRead, BinWrite};

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0PkdMain\0";

#[derive(Debug)]
pub struct PackedMainPacket {
    pub length: u64,                       // Length of the packet
    pub md5: Md5Hash,                      // MD5 hash of the packet
    pub set_id: RecoverySetId,             // Unique identifier for the PAR2 set
    pub subslice_size: u64, // Subslice size. Must be a multiple of 4 and equally divide the slice size.
    pub slice_size: u64, // Slice size. Must be a multiple of 4 and a multiple of the subslice size.
    pub file_count: u32, // Number of files in the recovery set.
    pub recovery_set_ids: Vec<FileId>, // File IDs of all files in the recovery set.
    pub non_recovery_set_ids: Vec<FileId>, // File IDs of all files in the non-recovery set.
}

impl BinRead for PackedMainPacket {
    type Args<'a> = ();

    fn read_options<R: std::io::Read + std::io::Seek>(
        reader: &mut R,
        _endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        use binrw::BinReaderExt;

        // Read magic
        let magic: [u8; 8] = reader.read_le()?;
        if &magic != b"PAR2\0PKT" {
            return Err(binrw::Error::AssertFail {
                pos: 0,
                message: "Invalid magic".to_string(),
            });
        }

        let length: u64 = reader.read_le()?;
        let md5_bytes: [u8; 16] = reader.read_le()?;
        let set_id_bytes: [u8; 16] = reader.read_le()?;
        let _packet_type: [u8; 16] = reader.read_le()?; // Skip type_of_packet
        let subslice_size: u64 = reader.read_le()?;
        let slice_size: u64 = reader.read_le()?;
        let file_count: u32 = reader.read_le()?;

        let mut recovery_set_ids = Vec::with_capacity(file_count as usize);
        for _ in 0..file_count {
            let id: [u8; 16] = reader.read_le()?;
            recovery_set_ids.push(FileId::new(id));
        }

        let non_recovery_count =
            (length as usize - 64 - 8 - 8 - 4 - (file_count as usize * 16)) / 16;
        let mut non_recovery_set_ids = Vec::with_capacity(non_recovery_count);
        for _ in 0..non_recovery_count {
            let id: [u8; 16] = reader.read_le()?;
            non_recovery_set_ids.push(FileId::new(id));
        }

        Ok(PackedMainPacket {
            length,
            md5: Md5Hash::new(md5_bytes),
            set_id: RecoverySetId::new(set_id_bytes),
            subslice_size,
            slice_size,
            file_count,
            recovery_set_ids,
            non_recovery_set_ids,
        })
    }
}

impl BinWrite for PackedMainPacket {
    type Args<'a> = ();

    fn write_options<W: std::io::Write + std::io::Seek>(
        &self,
        writer: &mut W,
        _endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        writer.write_all(b"PAR2\0PKT")?;
        writer.write_all(&self.length.to_le_bytes())?;
        writer.write_all(self.md5.as_bytes())?;
        writer.write_all(self.set_id.as_bytes())?;
        writer.write_all(TYPE_OF_PACKET)?;
        writer.write_all(&self.subslice_size.to_le_bytes())?;
        writer.write_all(&self.slice_size.to_le_bytes())?;
        writer.write_all(&self.file_count.to_le_bytes())?;
        for id in &self.recovery_set_ids {
            writer.write_all(id.as_bytes())?;
        }
        for id in &self.non_recovery_set_ids {
            writer.write_all(id.as_bytes())?;
        }
        Ok(())
    }
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
        data.extend_from_slice(self.set_id.as_bytes());
        data.extend_from_slice(TYPE_OF_PACKET);
        data.extend_from_slice(&self.subslice_size.to_le_bytes());
        data.extend_from_slice(&self.slice_size.to_le_bytes());
        data.extend_from_slice(&self.file_count.to_le_bytes());
        for id in &self.recovery_set_ids {
            data.extend_from_slice(id.as_bytes());
        }
        for id in &self.non_recovery_set_ids {
            data.extend_from_slice(id.as_bytes());
        }
        use md5::Digest;
        let computed_md5: [u8; 16] = md5::Md5::digest(&data).into();
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
