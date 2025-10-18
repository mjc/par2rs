use crate::domain::{Md5Hash, RecoverySetId};
use crate::recovery_loader::{FileSystemLoader, RecoveryDataLoader};
use binrw::{BinRead, BinWrite};
use std::path::PathBuf;
use std::sync::Arc;

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0RecvSlic";

/// Lightweight metadata for a recovery slice - does NOT load data into memory
/// This will eventually replace RecoverySlicePacket to minimize memory usage
#[derive(Clone)]
pub struct RecoverySliceMetadata {
    pub exponent: u32,
    pub set_id: RecoverySetId,
    /// Pluggable loader - can be filesystem, mmap, or custom implementation
    loader: Arc<dyn RecoveryDataLoader>,
}

impl std::fmt::Debug for RecoverySliceMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoverySliceMetadata")
            .field("exponent", &self.exponent)
            .field("set_id", &self.set_id)
            .field("data_size", &self.data_size())
            .finish()
    }
}

impl RecoverySliceMetadata {
    /// Create metadata with a custom loader
    pub fn new(exponent: u32, set_id: RecoverySetId, loader: Arc<dyn RecoveryDataLoader>) -> Self {
        Self {
            exponent,
            set_id,
            loader,
        }
    }

    /// Create metadata with filesystem-based loading
    pub fn from_file(
        exponent: u32,
        set_id: RecoverySetId,
        file_path: PathBuf,
        data_offset: u64,
        data_size: usize,
    ) -> Self {
        let loader = Arc::new(FileSystemLoader {
            file_path,
            data_offset,
            data_size,
        });
        Self::new(exponent, set_id, loader)
    }

    /// Read the actual recovery data from the loader when needed
    pub fn load_data(&self) -> std::io::Result<Vec<u8>> {
        self.loader.load_data()
    }

    /// Read a chunk of recovery data (memory-efficient)
    ///
    /// # Arguments
    /// * `chunk_offset` - Byte offset within the recovery data (not file offset)
    /// * `chunk_size` - Number of bytes to read
    ///
    /// # Returns
    /// Vector containing the requested chunk (may be smaller if at end of data)
    pub fn load_chunk(&self, chunk_offset: usize, chunk_size: usize) -> std::io::Result<Vec<u8>> {
        self.loader.load_chunk(chunk_offset, chunk_size)
    }

    /// Get the size of the recovery data
    pub fn data_size(&self) -> usize {
        self.loader.data_size()
    }

    /// Parse recovery slice metadata from a reader without loading the data
    /// This is the memory-efficient alternative to parsing RecoverySlicePacket
    pub fn parse_from_reader<R: std::io::Read + std::io::Seek>(
        reader: &mut R,
        file_path: PathBuf,
    ) -> std::io::Result<Self> {
        use std::io::SeekFrom;

        // Read packet header (64 bytes)
        let mut header = [0u8; 64];
        reader.read_exact(&mut header)?;

        // Check magic
        if &header[0..8] != b"PAR2\0PKT" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid PAR2 packet magic",
            ));
        }

        // Parse fields from header
        let length = u64::from_le_bytes(header[8..16].try_into().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid length field")
        })?);
        let set_id_bytes: [u8; 16] = header[32..48].try_into().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid set_id field")
        })?;
        let type_bytes: [u8; 16] = header[48..64].try_into().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid type field")
        })?;

        // Check type
        if type_bytes != *TYPE_OF_PACKET {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Not a recovery slice packet",
            ));
        }

        // Read exponent (4 bytes after the header)
        let mut exponent_bytes = [0u8; 4];
        reader.read_exact(&mut exponent_bytes)?;
        let exponent = u32::from_le_bytes(exponent_bytes);

        // Calculate data offset and size
        // Header size is 64 (fixed header) + 4 (exponent) = 68 bytes
        let header_size = 68u64;
        let data_size = length.checked_sub(header_size).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid packet length")
        })? as usize;

        // Get current absolute position in file (this is where recovery_data starts)
        let data_offset = reader.stream_position()?;

        // Skip past the recovery data without reading it into memory
        reader.seek(SeekFrom::Current(data_size as i64))?;

        // Create metadata with filesystem loader
        Ok(Self::from_file(
            exponent,
            RecoverySetId::new(set_id_bytes),
            file_path,
            data_offset,
            data_size,
        ))
    }
}

/// Full recovery slice packet - currently loads ALL data into memory
/// WARNING: This uses ~1.9GB of RAM for large PAR2 sets!
/// Transitioning to use RecoverySliceMetadata instead
#[derive(Debug, Clone, BinRead)]
#[br(magic = b"PAR2\0PKT")]
pub struct RecoverySlicePacket {
    pub length: u64, // Length of the packet
    #[br(map = |x: [u8; 16]| Md5Hash::new(x))]
    pub md5: Md5Hash, // MD5 hash of the packet
    #[br(map = |x: [u8; 16]| RecoverySetId::new(x))]
    pub set_id: RecoverySetId, // Unique identifier for the PAR2 set
    pub type_of_packet: [u8; 16], // Type of packet - should be "PAR 2.0\0RecvSlic"
    pub exponent: u32, // Exponent used to generate recovery data
    #[br(count = length as usize - (8 + 8 + 16 + 16 + 16 + 4))]
    // Calculate recovery data size: total length - (magic + length + md5 + set_id + type + exponent)
    pub recovery_data: Vec<u8>, // Recovery data - THIS IS THE MEMORY HOG!
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
        data.extend_from_slice(self.set_id.as_bytes());
        data.extend_from_slice(TYPE_OF_PACKET);
        data.extend_from_slice(&self.exponent.to_le_bytes());
        data.extend_from_slice(&self.recovery_data);
        use md5::Digest;
        let computed_md5: [u8; 16] = md5::Md5::digest(&data).into();
        if computed_md5 != *self.md5.as_bytes() {
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
        writer.write_all(self.md5.as_bytes())?;

        // Write the set_id field
        writer.write_all(self.set_id.as_bytes())?;

        // Write the type of packet
        writer.write_all(&self.type_of_packet)?;

        // Write the exponent
        writer.write_all(&self.exponent.to_le_bytes())?;

        // Write the recovery data
        writer.write_all(&self.recovery_data)?;

        Ok(())
    }
}
