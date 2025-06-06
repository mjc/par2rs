use binrw::{BinRead, BinWrite};

pub const TYPE_OF_PACKET: &[u8] = b"PAR 2.0\0FileDesc";

#[derive(Debug, BinRead, BinWrite)]
#[br(magic = b"PAR2\0PKT")]
pub struct FileDescriptionPacket {
    pub length: u64,      // Length of the packet
    pub md5: [u8; 16],    // MD5 hash of the packet type and body
    pub set_id: [u8; 16], // Unique identifier for the PAR2 set
    #[br(assert(packet_type == TYPE_OF_PACKET, "Packet type mismatch for FileDescriptionPacket. Expected {:?}, got {:?}", TYPE_OF_PACKET, packet_type))]
    pub packet_type: [u8; 16], // Type of the packet
    pub file_id: [u8; 16], // Unique identifier for the file
    pub md5_hash: [u8; 16], // MD5 hash of the entire file
    pub md5_16k: [u8; 16], // MD5 hash of the first 16kB of the file
    pub file_length: u64, // Length of the file
    #[br(count = length.saturating_sub(120))]
    // Removed the map function to prevent trimming of null bytes
    pub file_name: Vec<u8>, // Name of the file (including padding or null bytes)
}

impl FileDescriptionPacket {
    /// A doctest to compare the verification process against the `testfile.par2` file.
    ///
    /// ```rust
    /// use std::fs::File;
    /// use std::path::Path;
    /// use std::io::{Read,Seek};
    /// use binrw::{BinReaderExt, BinWrite};
    /// use par2rs::packets::file_description_packet::FileDescriptionPacket;
    ///
    /// let file_path = Path::new("tests/fixtures/packets/FileDescriptionPacket.par2");
    /// let mut file = File::open(file_path).expect("Failed to open test file");
    /// let packet: FileDescriptionPacket = file.read_le().expect("Failed to read FileDescriptionPacket");
    ///
    /// // get the md5 from the packet
    /// let md5_from_packet = packet.md5;
    /// // get the md5 from the open file
    /// let mut md5_from_file: [u8; 16] = [0; 16];
    /// file.seek(std::io::SeekFrom::Start(16)).expect("Failed to seek to MD5 in file");
    /// file.read_exact(&mut md5_from_file).expect("Failed to read MD5 from file");
    /// assert_eq!(md5_from_packet, md5_from_file, "MD5 from packet does not match MD5 from file");
    ///
    /// // Verify the packet using the `verify` method
    /// assert!(packet.verify(), "Packet verification failed");
    /// ```
    pub fn verify(&self) -> bool {
        if self.length < 120 {
            println!("Invalid packet length: {}", self.length);
            return false;
        }

        if self.packet_type != TYPE_OF_PACKET {
            println!(
                "Packet type mismatch: expected {:?}, got {:?}",
                TYPE_OF_PACKET, self.packet_type
            );
            return false;
        }

        let mut buffer = std::io::Cursor::new(Vec::new());
        if self.write_le(&mut buffer).is_err() {
            println!("Failed to serialize packet for length check");
            return false;
        }

        if buffer.get_ref().len() as u64 + 8 != self.length {
            println!(
                "Serialized length mismatch: expected {}, got {}",
                self.length,
                buffer.get_ref().len() as u64 + 8
            );
            return false;
        }

        let mut serialized_packet = std::io::Cursor::new(Vec::new());
        if self.write_le(&mut serialized_packet).is_err() {
            println!("Failed to serialize packet for MD5 verification");
            return false;
        }

        let set_id_start = 24; // Magic (8 bytes) + MD5 (16 bytes)
        let packet_data_for_md5 = serialized_packet.get_ref()[set_id_start..].to_vec();
        let computed_md5 = md5::compute(&packet_data_for_md5);
        if computed_md5.as_ref() != self.md5 {
            println!(
                "MD5 mismatch: expected {:?}, got {:?}",
                self.md5, computed_md5
            );
            return false;
        }

        true
    }
}
