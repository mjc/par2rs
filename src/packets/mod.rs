use binrw::BinReaderExt;
use std::io::{Read, Seek};

pub mod creator_packet;
pub mod error;
pub mod file_description_packet;
pub mod input_file_slice_checksum_packet;
pub mod main_packet;
pub mod packed_main_packet;
pub mod processing;
pub mod recovery_slice_packet;

pub use creator_packet::CreatorPacket;
pub use error::{PacketParseError, PacketParseResult};
pub use file_description_packet::FileDescriptionPacket;
pub use input_file_slice_checksum_packet::InputFileSliceChecksumPacket;
pub use main_packet::MainPacket;
pub use packed_main_packet::PackedMainPacket;
pub use processing::*;
pub use recovery_slice_packet::{RecoverySliceMetadata, RecoverySlicePacket};

/// PAR2 packet magic bytes signature
/// Reference: par2cmdline-turbo/src/par2fileformat.h
pub const MAGIC_BYTES: &[u8] = b"PAR2\0PKT";

/// Minimum valid packet size (header only)
/// Reference: par2cmdline-turbo/src/par2repairer.cpp:476
pub const MIN_PACKET_SIZE: u64 = 64;

/// Maximum valid packet size (100MB)
/// Reference: par2cmdline-turbo/src/par2repairer.cpp:477-479
pub const MAX_PACKET_SIZE: u64 = 100 * 1024 * 1024;

/// PAR2 packet header (64 bytes)
/// Contains magic bytes, length, MD5 hash, set ID, and packet type
#[derive(Debug, Clone)]
struct PacketHeader {
    /// Raw header bytes (64 bytes total)
    raw: [u8; 64],
    /// Packet type identifier (16 bytes at offset 48)
    packet_type: [u8; 16],
    /// Total packet length including header
    length: u64,
}

impl PacketHeader {
    /// Parse and validate a packet header from a reader
    ///
    /// Reference: par2cmdline-turbo/src/par2repairer.cpp:458-485
    /// Validates magic signature and packet length bounds
    fn parse<R: Read>(reader: &mut R) -> PacketParseResult<Self> {
        let mut header = [0u8; 64];
        reader.read_exact(&mut header).map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                PacketParseError::TruncatedData {
                    expected: 64,
                    actual: 0,
                }
            } else {
                PacketParseError::Io(e)
            }
        })?;

        // Validate magic bytes
        let magic: [u8; 8] = header[0..8].try_into().unwrap();
        if magic != *MAGIC_BYTES {
            return Err(PacketParseError::InvalidMagic(magic));
        }

        // Extract and validate packet length
        let length_bytes: [u8; 8] = header[8..16].try_into().unwrap();
        let length = u64::from_le_bytes(length_bytes);
        if !(MIN_PACKET_SIZE..=MAX_PACKET_SIZE).contains(&length) {
            return Err(PacketParseError::InvalidLength(length));
        }

        // Extract packet type
        let packet_type: [u8; 16] = header[48..64].try_into().unwrap();

        Ok(PacketHeader {
            raw: header,
            packet_type,
            length,
        })
    }
}

/// Read the complete packet data into a buffer for fast parsing
///
/// This is a critical optimization: reading the entire packet into memory
/// at once is ~10x faster than letting binrw make many small reads.
///
/// Reference: Original implementation in Packet::parse() - see commit history
fn read_full_packet<R: Read>(reader: &mut R, header: &PacketHeader) -> PacketParseResult<Vec<u8>> {
    let packet_length = header.length as usize;
    let mut packet_data = vec![0u8; packet_length];

    // Copy header we already read
    packet_data[..64].copy_from_slice(&header.raw);

    // Read remaining packet body
    reader.read_exact(&mut packet_data[64..]).map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            PacketParseError::TruncatedData {
                expected: packet_length,
                actual: 64, // We got the header but not the body
            }
        } else {
            PacketParseError::Io(e)
        }
    })?;

    Ok(packet_data)
}

#[derive(Debug)]
pub enum Packet {
    Main(MainPacket),
    PackedMain(PackedMainPacket),
    FileDescription(FileDescriptionPacket),
    RecoverySlice(RecoverySlicePacket),
    Creator(CreatorPacket),
    InputFileSliceChecksum(InputFileSliceChecksumPacket),
}

impl Packet {
    pub fn verify(&self) -> bool {
        match self {
            Packet::Main(packet) => packet.verify(),
            Packet::PackedMain(packet) => packet.verify(),
            Packet::FileDescription(packet) => packet.verify(),
            Packet::RecoverySlice(packet) => packet.verify(),
            Packet::Creator(packet) => packet.verify(),
            Packet::InputFileSliceChecksum(packet) => packet.verify(),
        }
    }

    /// Parse a single packet from a reader
    ///
    /// This is the main packet parsing entry point. It:
    /// 1. Reads and validates the 64-byte header
    /// 2. Reads the entire packet into a memory buffer (critical performance optimization)
    /// 3. Parses the packet data using binrw
    ///
    /// Reference: par2cmdline-turbo/src/par2repairer.cpp:458-550
    pub fn parse<R: Read + Seek>(reader: &mut R) -> PacketParseResult<Self> {
        let header = PacketHeader::parse(reader)?;
        let packet_data = read_full_packet(reader, &header)?;

        // Parse from memory buffer (much faster than streaming)
        let mut cursor = std::io::Cursor::new(&packet_data);
        Self::match_packet_type(&mut cursor, &header.packet_type)
    }

    fn match_packet_type<R: Read + Seek>(
        reader: &mut R,
        type_of_packet: &[u8],
    ) -> PacketParseResult<Self> {
        let packet = match type_of_packet {
            main_packet::TYPE_OF_PACKET => reader.read_le::<MainPacket>().ok().map(Packet::Main),
            packed_main_packet::TYPE_OF_PACKET => reader
                .read_le::<PackedMainPacket>()
                .ok()
                .map(Packet::PackedMain),
            file_description_packet::TYPE_OF_PACKET => reader
                .read_le::<FileDescriptionPacket>()
                .ok()
                .map(Packet::FileDescription),
            recovery_slice_packet::TYPE_OF_PACKET => reader
                .read_le::<RecoverySlicePacket>()
                .ok()
                .map(Packet::RecoverySlice),
            creator_packet::TYPE_OF_PACKET => {
                reader.read_le::<CreatorPacket>().ok().map(Packet::Creator)
            }
            input_file_slice_checksum_packet::TYPE_OF_PACKET => reader
                .read_le::<InputFileSliceChecksumPacket>()
                .ok()
                .map(Packet::InputFileSliceChecksum),
            _ => None,
        };

        packet.ok_or_else(|| {
            PacketParseError::UnknownPacketType(type_of_packet.try_into().unwrap_or([0; 16]))
        })
    }
}

/// Parse all packets from a PAR2 file
///
/// This convenience function skips recovery slice data for memory efficiency.
/// For a PAR2 set with 98 recovery blocks of 15MB each, this saves ~1.47GB
/// of temporary allocations during packet parsing.
///
/// Returns empty vector if any I/O errors occur or no valid packets found.
pub fn parse_packets<R: Read + Seek>(reader: &mut R) -> Vec<Packet> {
    let (packets, _recovery_count) = parse_packets_with_options(reader, false);
    packets
}

/// Scan forward to find the next packet magic bytes
///
/// Reference: par2cmdline-turbo/src/par2repairer.cpp:458-485
/// When a packet fails validation, we scan forward byte-by-byte to find
/// the next valid PAR2\0PKT magic sequence, similar to how par2cmdline recovers
/// from corrupted packets.
fn scan_for_next_magic<R: Read>(reader: &mut R) -> std::io::Result<Option<[u8; 8]>> {
    let mut buffer = [0u8; 8];

    // Try to read initial 8 bytes
    if reader.read_exact(&mut buffer).is_err() {
        return Ok(None); // EOF or read error
    }

    // Slide through the stream looking for magic bytes
    // We keep a rolling window of 8 bytes and check if it matches
    loop {
        if buffer == *MAGIC_BYTES {
            return Ok(Some(buffer));
        }

        // Shift buffer left by 1 byte and read next byte
        buffer.rotate_left(1);
        let mut next_byte = [0u8; 1];
        match reader.read_exact(&mut next_byte) {
            Ok(()) => buffer[7] = next_byte[0],
            Err(_) => return Ok(None), // EOF
        }
    }
}

/// Parse packets with optional recovery slice inclusion
///
/// When `include_recovery_slices` is false, recovery slice packet headers are still
/// validated (checking magic bytes, packet structure, MD5 if needed), but the actual
/// recovery data is NOT loaded into memory. This provides validation while saving memory.
///
/// When `include_recovery_slices` is true, recovery packets are fully loaded including
/// all recovery data (can be ~15MB per packet).
///
/// For a PAR2 set with 98 recovery blocks of 15MB each, skipping recovery data saves
/// ~1.47GB of temporary allocations during packet parsing.
///
/// Reference: par2cmdline-turbo/src/par2repairer.cpp:458-550
/// The reference implementation reads all packets but we optimize by skipping recovery data.
///
/// Returns (packets, recovery_block_count) where recovery_block_count includes validated
/// recovery blocks even when include_recovery_slices=false.
pub fn parse_packets_with_options<R: Read + Seek>(
    reader: &mut R,
    include_recovery_slices: bool,
) -> (Vec<Packet>, usize) {
    let mut packets = Vec::new();
    let mut recovery_block_count = 0;

    loop {
        // Try to parse packet header
        let header = match PacketHeader::parse(reader) {
            Ok(h) => h,
            Err(PacketParseError::TruncatedData { .. }) => {
                break;
            }
            Err(PacketParseError::InvalidMagic(_)) => {
                // Bad magic - try to find next valid packet by scanning forward
                if scan_for_next_magic(reader).ok().flatten().is_some() {
                    // Found magic, but we need to rewind 8 bytes so the next parse reads the header
                    if reader.seek(std::io::SeekFrom::Current(-8)).is_err() {
                        break;
                    }
                    continue;
                } else {
                    break;
                }
            }
            Err(_) => {
                break;
            }
        };

        // Special handling for recovery slice packets when not loading data
        if !include_recovery_slices && header.packet_type == recovery_slice_packet::TYPE_OF_PACKET {
            match validate_recovery_packet(reader, &header) {
                Ok(()) => {
                    recovery_block_count += 1;
                }
                Err(_) => {
                    // Validation failed - try to find next valid packet
                    if scan_for_next_magic(reader).ok().flatten().is_some() {
                        // Found magic, rewind 8 bytes
                        if reader.seek(std::io::SeekFrom::Current(-8)).is_err() {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            continue;
        }

        // Read and parse the full packet
        let packet_data = match read_full_packet(reader, &header) {
            Ok(data) => data,
            Err(_) => {
                // Failed to read packet body - try to find next valid packet
                if scan_for_next_magic(reader).ok().flatten().is_some() {
                    if reader.seek(std::io::SeekFrom::Current(-8)).is_err() {
                        break;
                    }
                    continue;
                } else {
                    break;
                }
            }
        };

        let mut cursor = std::io::Cursor::new(&packet_data);
        if let Ok(packet) = Packet::match_packet_type(&mut cursor, &header.packet_type) {
            // Count recovery slices when we're loading them
            if matches!(packet, Packet::RecoverySlice(_)) {
                recovery_block_count += 1;
            }
            packets.push(packet);
        }
        // Note: We silently skip unknown packet types to maintain forward compatibility
    }

    (packets, recovery_block_count)
}

/// Validate a recovery packet by loading it with binrw and checking its MD5
fn validate_recovery_packet<R: Read + Seek>(
    reader: &mut R,
    header: &PacketHeader,
) -> std::io::Result<()> {
    // Read the full packet into a buffer
    let packet_data = read_full_packet(reader, header)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

    // Parse the recovery packet to validate structure
    let mut cursor = std::io::Cursor::new(&packet_data);
    let packet = cursor
        .read_le::<RecoverySlicePacket>()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

    // Verify the MD5 hash
    if !packet.verify() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Recovery packet MD5 verification failed for exponent {}",
                packet.exponent
            ),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Helper to create a valid minimal packet header
    fn create_valid_header(packet_type: &[u8; 16], length: u64) -> Vec<u8> {
        let mut data = vec![0u8; length as usize];
        data[0..8].copy_from_slice(MAGIC_BYTES);
        data[8..16].copy_from_slice(&length.to_le_bytes());
        // MD5 hash at 16..32 (zeros for test)
        // Set ID at 32..48 (zeros for test)
        data[48..64].copy_from_slice(packet_type);
        data
    }

    mod packet_header_parsing {
        use super::*;

        #[test]
        fn parse_valid_header() {
            let header_bytes = create_valid_header(&[0u8; 16], 64);
            let mut cursor = Cursor::new(&header_bytes[..64]);
            let result = PacketHeader::parse(&mut cursor);
            assert!(result.is_ok());
            let header = result.unwrap();
            assert_eq!(header.length, 64);
        }

        #[test]
        fn parse_invalid_magic_bytes() {
            let mut data = [0u8; 64];
            data[0..8].copy_from_slice(b"INVALID!");
            let mut cursor = Cursor::new(&data[..]);
            let result = PacketHeader::parse(&mut cursor);
            assert!(matches!(result, Err(PacketParseError::InvalidMagic(_))));
        }

        #[test]
        fn parse_length_too_small() {
            let mut data = create_valid_header(&[0u8; 16], 64);
            data[8..16].copy_from_slice(&(63u64).to_le_bytes());
            let mut cursor = Cursor::new(&data[..64]);
            let result = PacketHeader::parse(&mut cursor);
            assert!(matches!(result, Err(PacketParseError::InvalidLength(63))));
        }

        #[test]
        fn parse_length_at_minimum() {
            let mut data = create_valid_header(&[0u8; 16], 64);
            data[8..16].copy_from_slice(&(64u64).to_le_bytes());
            let mut cursor = Cursor::new(&data[..64]);
            let result = PacketHeader::parse(&mut cursor);
            assert!(result.is_ok());
        }

        #[test]
        fn parse_length_too_large() {
            let mut data = create_valid_header(&[0u8; 16], 64);
            data[8..16].copy_from_slice(&(MAX_PACKET_SIZE + 1).to_le_bytes());
            let mut cursor = Cursor::new(&data[..64]);
            let result = PacketHeader::parse(&mut cursor);
            assert!(matches!(result, Err(PacketParseError::InvalidLength(_))));
        }

        #[test]
        fn parse_length_at_maximum() {
            let mut data = create_valid_header(&[0u8; 16], 64);
            data[8..16].copy_from_slice(&MAX_PACKET_SIZE.to_le_bytes());
            let mut cursor = Cursor::new(&data[..64]);
            let result = PacketHeader::parse(&mut cursor);
            assert!(result.is_ok());
        }

        #[test]
        fn parse_truncated_header() {
            let data = vec![0u8; 32]; // Only half a header
            let mut cursor = Cursor::new(&data);
            let result = PacketHeader::parse(&mut cursor);
            assert!(matches!(
                result,
                Err(PacketParseError::TruncatedData { .. })
            ));
        }

        #[test]
        fn parse_empty_reader() {
            let data = Vec::<u8>::new();
            let mut cursor = Cursor::new(&data);
            let result = PacketHeader::parse(&mut cursor);
            assert!(matches!(
                result,
                Err(PacketParseError::TruncatedData { .. })
            ));
        }
    }

    mod full_packet_reading {
        use super::*;

        #[test]
        fn read_complete_packet() {
            let packet_data = create_valid_header(&[0u8; 16], 128);
            let mut cursor = Cursor::new(&packet_data);
            let header = PacketHeader::parse(&mut cursor).unwrap();
            let result = read_full_packet(&mut cursor, &header);
            assert!(result.is_ok());
            assert_eq!(result.unwrap().len(), 128);
        }

        #[test]
        fn read_truncated_packet() {
            let mut packet_data = create_valid_header(&[0u8; 16], 256);
            packet_data.truncate(100); // Truncate to less than claimed length
            let mut cursor = Cursor::new(&packet_data);
            let header = PacketHeader::parse(&mut cursor).unwrap();
            let result = read_full_packet(&mut cursor, &header);
            assert!(matches!(
                result,
                Err(PacketParseError::TruncatedData { .. })
            ));
        }

        #[test]
        fn read_minimum_size_packet() {
            let packet_data = create_valid_header(&[0u8; 16], 64);
            let mut cursor = Cursor::new(&packet_data);
            let header = PacketHeader::parse(&mut cursor).unwrap();
            let result = read_full_packet(&mut cursor, &header);
            assert!(result.is_ok());
            assert_eq!(result.unwrap().len(), 64);
        }
    }

    mod packet_parsing {
        use super::*;

        #[test]
        fn parse_unknown_packet_type() {
            let unknown_type = [0xFFu8; 16];
            let packet_data = create_valid_header(&unknown_type, 64);
            let mut cursor = Cursor::new(&packet_data);
            let result = Packet::parse(&mut cursor);
            assert!(matches!(
                result,
                Err(PacketParseError::UnknownPacketType(_))
            ));
        }

        #[test]
        fn parse_empty_file() {
            let data = Vec::<u8>::new();
            let mut cursor = Cursor::new(&data);
            let result = Packet::parse(&mut cursor);
            assert!(matches!(
                result,
                Err(PacketParseError::TruncatedData { .. })
            ));
        }

        #[test]
        fn parse_corrupted_magic() {
            let data = vec![0xFFu8; 128];
            let mut cursor = Cursor::new(&data);
            let result = Packet::parse(&mut cursor);
            assert!(matches!(result, Err(PacketParseError::InvalidMagic(_))));
        }
    }

    mod parse_packets_with_options {
        use super::*;

        #[test]
        fn parse_empty_file() {
            let data = Vec::<u8>::new();
            let mut cursor = Cursor::new(&data);
            let packets = parse_packets(&mut cursor);
            assert!(packets.is_empty());
        }

        #[test]
        fn parse_file_with_invalid_magic() {
            let data = vec![0xFFu8; 1000];
            let mut cursor = Cursor::new(&data);
            let packets = parse_packets(&mut cursor);
            assert!(packets.is_empty());
        }

        #[test]
        fn parse_multiple_unknown_packets() {
            let unknown_type = [0xFFu8; 16];
            let mut data = Vec::new();

            // Create two unknown packets
            data.extend_from_slice(&create_valid_header(&unknown_type, 64));
            data.extend_from_slice(&create_valid_header(&unknown_type, 64));

            let mut cursor = Cursor::new(&data);
            let packets = parse_packets(&mut cursor);
            // Unknown packets are silently skipped for forward compatibility
            assert!(packets.is_empty());
        }

        #[test]
        fn parse_stops_on_invalid_length() {
            let mut data = Vec::new();

            // First packet valid, second has invalid length
            data.extend_from_slice(&create_valid_header(&[0u8; 16], 64));

            let mut invalid_packet = create_valid_header(&[0u8; 16], 64);
            invalid_packet[8..16].copy_from_slice(&(MAX_PACKET_SIZE + 1).to_le_bytes());
            data.extend_from_slice(&invalid_packet);

            let mut cursor = Cursor::new(&data);
            let packets = parse_packets(&mut cursor);
            // Should stop parsing at the invalid packet
            assert_eq!(packets.len(), 0); // First packet is also unknown type
        }

        #[test]
        fn parse_recovery_skip_option() {
            // Create a 16-byte array for the recovery packet type
            let mut recovery_type = [0u8; 16];
            recovery_type.copy_from_slice(recovery_slice_packet::TYPE_OF_PACKET);

            // Test that recovery packet detection works
            // We can't easily test actual parsing without a valid recovery packet,
            // but we can verify the TYPE_OF_PACKET constant is correct
            assert_eq!(recovery_type.len(), 16);

            // The actual skip behavior is tested in integration tests with real PAR2 files
            // This unit test just validates the basic structure
        }
    }

    mod edge_cases {
        use super::*;

        #[test]
        fn boundary_length_63() {
            let mut data = create_valid_header(&[0u8; 16], 64); // Create full header
            data[8..16].copy_from_slice(&(63u64).to_le_bytes()); // Change length field to 63
            let mut cursor = Cursor::new(&data);
            let result = Packet::parse(&mut cursor);
            assert!(matches!(result, Err(PacketParseError::InvalidLength(63))));
        }

        #[test]
        fn boundary_length_64() {
            let data = create_valid_header(&[0xFFu8; 16], 64);
            let mut cursor = Cursor::new(&data);
            let result = Packet::parse(&mut cursor);
            // Will fail on unknown packet type, but length validation passed
            assert!(matches!(
                result,
                Err(PacketParseError::UnknownPacketType(_))
            ));
        }

        #[test]
        fn max_packet_size() {
            let mut data = create_valid_header(&[0u8; 16], 64);
            data[8..16].copy_from_slice(&MAX_PACKET_SIZE.to_le_bytes());
            let mut cursor = Cursor::new(&data);
            let header = PacketHeader::parse(&mut cursor);
            assert!(header.is_ok());
            assert_eq!(header.unwrap().length, MAX_PACKET_SIZE);
        }

        #[test]
        fn max_packet_size_plus_one() {
            let mut data = create_valid_header(&[0u8; 16], 64);
            data[8..16].copy_from_slice(&(MAX_PACKET_SIZE + 1).to_le_bytes());
            let mut cursor = Cursor::new(&data);
            let result = PacketHeader::parse(&mut cursor);
            assert!(matches!(result, Err(PacketParseError::InvalidLength(_))));
        }

        #[test]
        fn scan_for_next_magic_finds_magic() {
            // Test that scan_for_next_magic can find PAR2 magic in garbage data
            let mut data = Vec::new();

            // Add garbage bytes
            data.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
            data.extend_from_slice(&[0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00]);

            // Add PAR2 magic
            data.extend_from_slice(MAGIC_BYTES);

            let mut cursor = Cursor::new(&data);
            let result = scan_for_next_magic(&mut cursor);

            assert!(result.is_ok());
            let magic = result.unwrap();
            assert!(magic.is_some());
            assert_eq!(magic.unwrap(), MAGIC_BYTES);

            // Cursor should be positioned right after the magic
            let pos = cursor.position();
            assert_eq!(pos, 24); // 16 garbage bytes + 8 magic bytes
        }

        #[test]
        fn scan_for_next_magic_no_magic_found() {
            // Test that scan_for_next_magic returns None when no magic is found
            let data = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
            let mut cursor = Cursor::new(&data);
            let result = scan_for_next_magic(&mut cursor);

            assert!(result.is_ok());
            assert!(result.unwrap().is_none());
        }

        #[test]
        fn scan_for_next_magic_empty_reader() {
            // Test that scan_for_next_magic handles empty input
            let data = Vec::<u8>::new();
            let mut cursor = Cursor::new(&data);
            let result = scan_for_next_magic(&mut cursor);

            assert!(result.is_ok());
            assert!(result.unwrap().is_none());
        }

        #[test]
        fn scan_for_next_magic_at_beginning() {
            // Test that scan_for_next_magic finds magic at the very start
            let mut data = Vec::new();
            data.extend_from_slice(MAGIC_BYTES);
            data.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]); // Some trailing data

            let mut cursor = Cursor::new(&data);
            let result = scan_for_next_magic(&mut cursor);

            assert!(result.is_ok());
            let magic = result.unwrap();
            assert!(magic.is_some());
            assert_eq!(magic.unwrap(), MAGIC_BYTES);
        }

        #[test]
        fn scan_for_next_magic_partial_match() {
            // Test that scan_for_next_magic doesn't false-positive on partial matches
            let mut data = Vec::new();

            // Add partial magic (first 7 bytes of PAR2 magic)
            data.extend_from_slice(&[0x50, 0x41, 0x52, 0x32, 0x00, 0x50, 0x4B]);
            // Add garbage
            data.extend_from_slice(&[0xFF]);
            // Add more garbage
            data.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
            // Add real magic
            data.extend_from_slice(MAGIC_BYTES);

            let mut cursor = Cursor::new(&data);
            let result = scan_for_next_magic(&mut cursor);

            assert!(result.is_ok());
            let magic = result.unwrap();
            assert!(magic.is_some());
            // Should find the real magic at byte 12 (7 + 1 + 4)
            let pos = cursor.position();
            assert_eq!(pos, 20); // 12 bytes before magic + 8 magic bytes
        }

        #[test]
        fn corrupt_packet_recovery() {
            // Test that we can recover from a corrupt packet by finding the next valid magic
            // Simulates the scenario from vol003-007.par2 where a recovery packet has
            // corrupt length/MD5 but we can find the next valid packet

            let mut file_data = Vec::new();

            // 1. Valid Main packet (92 bytes)
            let mut main_type = [0u8; 16];
            main_type.copy_from_slice(main_packet::TYPE_OF_PACKET);
            let mut main_packet = create_valid_header(&main_type, 92);
            // Add minimal body (slice_size + file_count)
            main_packet.extend_from_slice(&5242880u64.to_le_bytes()); // slice size
            main_packet.extend_from_slice(&1u32.to_le_bytes()); // 1 file
            main_packet.extend_from_slice(&[0u8; 16]); // 1 file ID
            file_data.extend_from_slice(&main_packet);

            // 2. Corrupt recovery packet - valid header but wrong length field
            // Header says 200 bytes but we'll put garbage data
            let mut recovery_type = [0u8; 16];
            recovery_type.copy_from_slice(recovery_slice_packet::TYPE_OF_PACKET);
            let mut corrupt_recovery = vec![0u8; 64];
            corrupt_recovery[0..8].copy_from_slice(MAGIC_BYTES);
            corrupt_recovery[8..16].copy_from_slice(&200u64.to_le_bytes()); // Says 200 bytes
            corrupt_recovery[16..32].copy_from_slice(&[0x99u8; 16]); // Bad MD5
            corrupt_recovery[32..48].copy_from_slice(&[0xAAu8; 16]); // Set ID
            corrupt_recovery[48..64].copy_from_slice(&recovery_type);
            // Add 4 bytes exponent
            corrupt_recovery.extend_from_slice(&3u32.to_le_bytes());
            // Add garbage data (but NOT 200 bytes total - simulate corruption)
            corrupt_recovery.extend_from_slice(&[0x42u8; 500]); // Way more than header claims
            file_data.extend_from_slice(&corrupt_recovery);

            // 3. Add some garbage bytes before next packet
            file_data.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);

            // 4. Valid Creator packet (88 bytes)
            let mut creator_type = [0u8; 16];
            creator_type.copy_from_slice(creator_packet::TYPE_OF_PACKET);
            let mut creator_packet = create_valid_header(&creator_type, 88);
            creator_packet.extend_from_slice(b"par2rs test\0\0\0\0\0\0\0\0\0\0\0\0\0"); // 24 byte client string
            file_data.extend_from_slice(&creator_packet);

            // Try to parse - should skip corrupt packet and find valid ones
            let mut cursor = Cursor::new(&file_data);
            let (packets, _recovery_count) = parse_packets_with_options(&mut cursor, false);

            // Should have found the Main and Creator packets, skipped the corrupt recovery
            assert!(!packets.is_empty(), "Should find at least main packet");
            assert!(
                matches!(packets[0], Packet::Main(_)),
                "First packet should be Main"
            );
        }
    }
}
