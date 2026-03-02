//! Packet generation for PAR2 creation
//!
//! This module handles generation of all PAR2 packets needed for file creation:
//! - Main packet (recovery set metadata)
//! - Creator packet (client identification)
//! - FileDescription packets (file metadata)
//! - FileVerification packets (file checksums)
//! - Recovery packets (Reed-Solomon data)
//!
//! Reference: par2cmdline-turbo/src/par2creator.cpp CreateMainPacket(),
//! CreateCreatorPacket(), WriteCriticalPackets()

use crate::domain::{Crc32Value, FileId, Md5Hash, RecoverySetId};
use crate::packets::{
    creator_packet::CreatorPacket,
    file_description_packet::{self, FileDescriptionPacket},
    input_file_slice_checksum_packet::InputFileSliceChecksumPacket,
    main_packet::MainPacket,
};
use binrw::BinWrite;

use super::error::{CreateError, CreateResult};
use super::source_file::SourceFileInfo;

/// Generate a Main packet for the PAR2 set
///
/// The Main packet contains:
/// - Recovery set ID (derived from MD5 of packet body)
/// - Slice size (block size)
/// - File count
/// - List of file IDs
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp CreateMainPacket()
pub fn generate_main_packet(
    recovery_set_id: RecoverySetId,
    block_size: u64,
    source_files: &[SourceFileInfo],
) -> CreateResult<MainPacket> {
    let file_count = source_files.len() as u32;
    let file_ids: Vec<FileId> = source_files.iter().map(|f| f.file_id).collect();

    // Calculate packet length: header (64) + slice_size (8) + file_count (4) + file_ids
    let packet_length = 64 + 8 + 4 + (file_ids.len() * 16) as u64;

    // For now, compute MD5 as zeros - will be computed when writing
    let md5 = Md5Hash::new([0u8; 16]);

    Ok(MainPacket {
        length: packet_length,
        md5,
        set_id: recovery_set_id,
        slice_size: block_size,
        file_count,
        file_ids,
        non_recovery_file_ids: Vec::new(), // Empty for standard PAR2 sets
    })
}

/// Generate a Creator packet identifying par2rs
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp CreateCreatorPacket()
pub fn generate_creator_packet(recovery_set_id: RecoverySetId) -> CreateResult<CreatorPacket> {
    let creator_info = format!("par2rs-{}", env!("CARGO_PKG_VERSION")).into_bytes();

    // Calculate packet length: header (64) + creator_info
    let packet_length = 64 + creator_info.len() as u64;

    // MD5 will be computed when writing
    let md5 = Md5Hash::new([0u8; 16]);

    Ok(CreatorPacket {
        length: packet_length,
        md5,
        set_id: recovery_set_id,
        creator_info,
    })
}

/// Generate a FileDescription packet for a source file
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp (FileDescription packet creation)
pub fn generate_file_description_packet(
    recovery_set_id: RecoverySetId,
    source_file: &SourceFileInfo,
) -> CreateResult<FileDescriptionPacket> {
    let filename_bytes = source_file
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            CreateError::Other(format!("Invalid filename: {}", source_file.path.display()))
        })?
        .as_bytes()
        .to_vec();

    // Ensure filename is not empty
    if filename_bytes.is_empty() {
        return Err(CreateError::Other("Empty filename".to_string()));
    }

    // Add null terminator and pad to multiple of 4
    let mut name_with_null = filename_bytes.clone();
    name_with_null.push(0);
    while name_with_null.len() % 4 != 0 {
        name_with_null.push(0);
    }

    // Calculate packet length: header (64) + file_id (16) + md5 (16) + hash16k (16) + length (8) + name
    let packet_length = 64 + 16 + 16 + 16 + 8 + name_with_null.len() as u64;

    // MD5 will be computed when writing
    let md5 = Md5Hash::new([0u8; 16]);

    Ok(FileDescriptionPacket {
        length: packet_length,
        md5,
        set_id: recovery_set_id,
        packet_type: file_description_packet::TYPE_OF_PACKET
            .try_into()
            .expect("TYPE_OF_PACKET is exactly 16 bytes"),
        file_id: source_file.file_id,
        md5_hash: source_file.hash,
        md5_16k: source_file.hash_16k,
        file_length: source_file.size,
        file_name: name_with_null,
    })
}

/// Generate an InputFileSliceChecksum packet (FileVerification) for a source file
///
/// This packet contains MD5 hashes and CRC32 checksums for all blocks in the file.
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp (IFSC packet creation)
pub fn generate_file_verification_packet(
    recovery_set_id: RecoverySetId,
    source_file: &SourceFileInfo,
) -> CreateResult<InputFileSliceChecksumPacket> {
    // Build pairs of (md5, crc32) for each block
    let pairs: Vec<(Md5Hash, Crc32Value)> = source_file
        .block_checksums
        .iter()
        .map(|bc| (bc.hash, Crc32Value::new(bc.crc32)))
        .collect();

    // Calculate packet length: header (64) + file_id (16) + pairs
    let packet_length = 64 + 16 + (pairs.len() * 20) as u64;

    // MD5 will be computed when writing
    let md5 = Md5Hash::new([0u8; 16]);

    Ok(InputFileSliceChecksumPacket {
        length: packet_length,
        md5,
        set_id: recovery_set_id,
        file_id: source_file.file_id,
        slice_checksums: pairs,
    })
}

/// Generate recovery set ID from main packet data
///
/// The recovery set ID is the MD5 hash of the main packet body (excluding header)
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp CreateMainPacket()
pub fn generate_recovery_set_id(
    block_size: u64,
    source_files: &[SourceFileInfo],
) -> CreateResult<RecoverySetId> {
    use crate::packets::main_packet::TYPE_OF_PACKET;

    let file_count = source_files.len() as u32;
    let file_ids: Vec<FileId> = source_files.iter().map(|f| f.file_id).collect();

    // Build the packet body that will be hashed
    let mut body = Vec::new();

    // Add a placeholder recovery set ID (16 zero bytes) - this gets replaced with actual hash
    body.extend_from_slice(&[0u8; 16]);

    // Add packet type
    body.extend_from_slice(TYPE_OF_PACKET);

    // Add slice size and file count
    body.extend_from_slice(&block_size.to_le_bytes());
    body.extend_from_slice(&file_count.to_le_bytes());

    // Add all file IDs
    for file_id in &file_ids {
        body.extend_from_slice(file_id.as_bytes());
    }

    // Compute MD5 of the body
    let set_id_bytes = crate::checksum::compute_md5_bytes(&body);

    Ok(RecoverySetId::new(set_id_bytes))
}

/// Helper to serialize a packet, compute MD5, and update the MD5 field in the bytes
///
/// The MD5 hash is computed over the packet body (everything after offset 32:
/// magic (8 bytes) + length (8 bytes) + md5 field (16 bytes) + body...)
///
/// Returns the serialized bytes with the correct MD5 already inserted.
fn finalize_packet_bytes(mut bytes: Vec<u8>) -> CreateResult<Vec<u8>> {
    // MD5 is computed over everything after offset 32
    // Layout: magic (8) + length (8) + md5 (16) + rest...
    //         ^0         ^8          ^16        ^32
    if bytes.len() < 32 {
        return Err(CreateError::PacketGenerationError(
            "Packet too small for MD5 computation".to_string(),
        ));
    }

    let body = &bytes[32..];
    let md5_bytes = crate::checksum::compute_md5_bytes(body);

    // Update the MD5 field in the serialized bytes (at offset 16-32)
    bytes[16..32].copy_from_slice(&md5_bytes);

    Ok(bytes)
}

/// Write a MainPacket to a writer with computed MD5
pub fn write_main_packet<W: std::io::Write>(
    writer: &mut W,
    packet: &MainPacket,
) -> CreateResult<()> {
    use std::io::Cursor;

    // Serialize with placeholder MD5
    let mut buffer = Cursor::new(Vec::new());
    packet.write_le(&mut buffer).map_err(|e| {
        CreateError::PacketGenerationError(format!("Failed to serialize MainPacket: {}", e))
    })?;

    // Finalize and write
    let bytes = finalize_packet_bytes(buffer.into_inner())?;
    writer.write_all(&bytes).map_err(CreateError::IoError)?;

    Ok(())
}

/// Write a CreatorPacket to a writer with computed MD5
pub fn write_creator_packet<W: std::io::Write>(
    writer: &mut W,
    packet: &CreatorPacket,
) -> CreateResult<()> {
    use std::io::Cursor;

    // Serialize with placeholder MD5
    let mut buffer = Cursor::new(Vec::new());
    packet.write_le(&mut buffer).map_err(|e| {
        CreateError::PacketGenerationError(format!("Failed to serialize CreatorPacket: {}", e))
    })?;

    // Finalize and write
    let bytes = finalize_packet_bytes(buffer.into_inner())?;
    writer.write_all(&bytes).map_err(CreateError::IoError)?;

    Ok(())
}

/// Write a FileDescriptionPacket to a writer with computed MD5
///
/// Note: FileDescriptionPacket uses binrw derive macros which don't automatically
/// write the magic bytes even though they're specified with #[br(magic)].
/// We need to prepend them manually.
pub fn write_file_description_packet<W: std::io::Write>(
    writer: &mut W,
    packet: &FileDescriptionPacket,
) -> CreateResult<()> {
    use std::io::Cursor;

    // Write magic bytes first
    writer
        .write_all(crate::packets::MAGIC_BYTES)
        .map_err(CreateError::IoError)?;

    // Serialize the rest with placeholder MD5
    let mut buffer = Cursor::new(Vec::new());
    packet.write_le(&mut buffer).map_err(|e| {
        CreateError::PacketGenerationError(format!(
            "Failed to serialize FileDescriptionPacket: {}",
            e
        ))
    })?;

    let mut bytes = buffer.into_inner();

    // Compute MD5 over body (after length + md5 fields)
    // The serialized bytes start at offset 0 (length field), so body is at offset 24
    if bytes.len() < 24 {
        return Err(CreateError::PacketGenerationError(
            "FileDescriptionPacket too small for MD5 computation".to_string(),
        ));
    }

    let body = &bytes[24..]; // Skip length (8) + md5 (16)
    let md5_bytes = crate::checksum::compute_md5_bytes(body);

    // Update MD5 field (at offset 8-24 in serialized bytes)
    bytes[8..24].copy_from_slice(&md5_bytes);

    // Write the corrected bytes
    writer.write_all(&bytes).map_err(CreateError::IoError)?;

    Ok(())
}

/// Write an InputFileSliceChecksumPacket to a writer with computed MD5
pub fn write_file_verification_packet<W: std::io::Write>(
    writer: &mut W,
    packet: &InputFileSliceChecksumPacket,
) -> CreateResult<()> {
    use std::io::Cursor;

    // Serialize with placeholder MD5
    let mut buffer = Cursor::new(Vec::new());
    packet.write_le(&mut buffer).map_err(|e| {
        CreateError::PacketGenerationError(format!(
            "Failed to serialize InputFileSliceChecksumPacket: {}",
            e
        ))
    })?;

    // Finalize and write
    let bytes = finalize_packet_bytes(buffer.into_inner())?;
    writer.write_all(&bytes).map_err(CreateError::IoError)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::source_file::BlockChecksum;
    use std::path::PathBuf;

    #[test]
    fn test_generate_recovery_set_id() {
        let source_files = vec![
            SourceFileInfo {
                file_id: FileId::new([1u8; 16]),
                path: PathBuf::from("test1.dat"),
                size: 1024,
                hash: Md5Hash::new([2u8; 16]),
                hash_16k: Md5Hash::new([0u8; 16]),
                index: 0,
                block_checksums: Vec::new(),
                global_block_offset: 0,
                block_count: 1,
            },
            SourceFileInfo {
                file_id: FileId::new([3u8; 16]),
                path: PathBuf::from("test2.dat"),
                size: 2048,
                hash: Md5Hash::new([4u8; 16]),
                hash_16k: Md5Hash::new([0u8; 16]),
                index: 1,
                block_checksums: Vec::new(),
                global_block_offset: 1,
                block_count: 2,
            },
        ];

        let set_id = generate_recovery_set_id(512, &source_files).unwrap();

        // Should generate a valid non-zero ID
        assert_ne!(set_id.as_bytes(), &[0u8; 16]);
    }

    #[test]
    fn test_generate_main_packet() {
        let set_id = RecoverySetId::new([0xAA; 16]);
        let source_files = vec![SourceFileInfo {
            file_id: FileId::new([0xBB; 16]),
            path: PathBuf::from("test.dat"),
            size: 1024,
            hash: Md5Hash::new([0xCC; 16]),
            hash_16k: Md5Hash::new([0u8; 16]),
            index: 0,
            block_checksums: Vec::new(),
            global_block_offset: 0,
            block_count: 2,
        }];

        let packet = generate_main_packet(set_id, 512, &source_files).unwrap();

        assert_eq!(packet.slice_size, 512);
        assert_eq!(packet.file_count, 1);
        assert_eq!(packet.file_ids.len(), 1);
        assert_eq!(packet.file_ids[0], FileId::new([0xBB; 16]));
        assert_eq!(packet.length, 64 + 8 + 4 + 16); // header + slice_size + file_count + 1 file_id
    }

    #[test]
    fn test_generate_creator_packet() {
        let set_id = RecoverySetId::new([0xAA; 16]);
        let packet = generate_creator_packet(set_id).unwrap();

        assert_eq!(packet.set_id, set_id);
        assert!(!packet.creator_info.is_empty());
        assert!(packet.length >= 64);

        // Check that creator info contains "par2rs"
        let creator_str = String::from_utf8_lossy(&packet.creator_info);
        assert!(creator_str.contains("par2rs"));
    }

    #[test]
    fn test_generate_file_description_packet() {
        let set_id = RecoverySetId::new([0xAA; 16]);
        let source_file = SourceFileInfo {
            file_id: FileId::new([0xBB; 16]),
            path: PathBuf::from("/tmp/test.dat"),
            size: 12345,
            hash: Md5Hash::new([0xCC; 16]),
            hash_16k: Md5Hash::new([0u8; 16]),
            index: 0,
            block_checksums: Vec::new(),
            global_block_offset: 0,
            block_count: 25,
        };

        let packet = generate_file_description_packet(set_id, &source_file).unwrap();

        assert_eq!(packet.set_id, set_id);
        assert_eq!(packet.file_id, FileId::new([0xBB; 16]));
        assert_eq!(packet.md5_hash, Md5Hash::new([0xCC; 16]));
        assert_eq!(packet.file_length, 12345);

        // Name should be "test.dat\0" padded to multiple of 4
        let name_str = std::str::from_utf8(&packet.file_name[..8]).unwrap();
        assert_eq!(name_str, "test.dat");
        assert_eq!(packet.file_name.len() % 4, 0);
    }

    #[test]
    fn test_generate_file_verification_packet() {
        let set_id = RecoverySetId::new([0xAA; 16]);
        let source_file = SourceFileInfo {
            file_id: FileId::new([0xBB; 16]),
            path: PathBuf::from("test.dat"),
            size: 1024,
            hash: Md5Hash::new([0xCC; 16]),
            hash_16k: Md5Hash::new([0u8; 16]),
            index: 0,
            block_checksums: vec![
                BlockChecksum {
                    crc32: 0x12345678,
                    hash: Md5Hash::new([0xDD; 16]),
                    global_index: 0,
                },
                BlockChecksum {
                    crc32: 0x9ABCDEF0,
                    hash: Md5Hash::new([0xEE; 16]),
                    global_index: 1,
                },
            ],
            global_block_offset: 0,
            block_count: 2,
        };

        let packet = generate_file_verification_packet(set_id, &source_file).unwrap();

        assert_eq!(packet.set_id, set_id);
        assert_eq!(packet.file_id, FileId::new([0xBB; 16]));
        assert_eq!(packet.slice_checksums.len(), 2);
        assert_eq!(packet.length, 64 + 16 + 40); // header + file_id + 2*20
    }

    #[test]
    fn test_write_main_packet_with_md5() {
        use binrw::BinReaderExt;

        let set_id = RecoverySetId::new([0xAA; 16]);

        // Generate a main packet
        let source_file = SourceFileInfo {
            file_id: FileId::new([0xBB; 16]),
            path: PathBuf::from("test.dat"),
            size: 512,
            hash: Md5Hash::new([0xCC; 16]),
            hash_16k: Md5Hash::new([0u8; 16]),
            index: 0,
            block_checksums: Vec::new(),
            global_block_offset: 0,
            block_count: 1,
        };
        let packet = generate_main_packet(set_id, 512, &[source_file]).unwrap();

        // Write it with MD5
        let mut buffer = Vec::new();
        write_main_packet(&mut buffer, &packet).unwrap();

        // Read it back
        let mut cursor = std::io::Cursor::new(&buffer);
        let read_packet: MainPacket = cursor.read_le().unwrap();

        // Verify the MD5 is correct by checking against manual computation
        assert_ne!(read_packet.md5, Md5Hash::new([0u8; 16])); // Should not be zero
        assert_eq!(read_packet.slice_size, 512);

        // The MD5 in the buffer at offset 16-32 should match what we expect
        let body = &buffer[32..];
        let expected_md5 = crate::checksum::compute_md5_bytes(body);
        assert_eq!(read_packet.md5.as_bytes(), &expected_md5);
    }

    #[test]
    fn test_write_file_description_packet_with_md5() {
        use binrw::BinReaderExt;

        let set_id = RecoverySetId::new([0xAA; 16]);
        let source_file = SourceFileInfo {
            file_id: FileId::new([0xBB; 16]),
            path: PathBuf::from("/tmp/test.dat"),
            size: 12345,
            hash: Md5Hash::new([0xCC; 16]),
            hash_16k: Md5Hash::new([0u8; 16]),
            index: 0,
            block_checksums: Vec::new(),
            global_block_offset: 0,
            block_count: 25,
        };

        let packet = generate_file_description_packet(set_id, &source_file).unwrap();

        // Write it with MD5
        let mut buffer = Vec::new();
        write_file_description_packet(&mut buffer, &packet).unwrap();

        // Read it back
        let mut cursor = std::io::Cursor::new(&buffer);
        let read_packet: FileDescriptionPacket = cursor.read_le().unwrap();

        // Verify the MD5 is computed correctly
        assert_ne!(read_packet.md5, Md5Hash::new([0u8; 16]));
        let body = &buffer[32..];
        let expected_md5 = crate::checksum::compute_md5_bytes(body);
        assert_eq!(read_packet.md5.as_bytes(), &expected_md5);
    }
}
