use crate::domain::{Crc32Value, FileId, Md5Hash};
use crate::packets::{FileDescriptionPacket, MainPacket, Packet};
use rustc_hash::FxHashMap as HashMap;

/// Functional packet processing utilities
///
/// This module provides functional, reusable utilities for extracting and processing
/// information from PAR2 packets. These functions are designed to be pure, efficient,
/// and composable.
/// Extract main packet information
///
/// Returns the first MainPacket found, or None if no MainPacket exists
pub fn extract_main_packet(packets: &[Packet]) -> Option<&MainPacket> {
    packets.iter().find_map(|p| {
        if let Packet::Main(main) = p {
            Some(main)
        } else {
            None
        }
    })
}

/// Count recovery blocks available
///
/// Returns the total number of RecoverySlice packets
pub fn count_recovery_blocks(packets: &[Packet]) -> usize {
    packets
        .iter()
        .filter(|p| matches!(p, Packet::RecoverySlice(_)))
        .count()
}

/// Extract file descriptions (deduplicated by file_id)
///
/// Returns unique FileDescriptionPackets, with later packets overwriting earlier ones
/// for the same file_id (handles volume duplication automatically)
pub fn extract_file_descriptions(packets: &[Packet]) -> Vec<&FileDescriptionPacket> {
    packets
        .iter()
        .filter_map(|p| {
            if let Packet::FileDescription(fd) = p {
                Some((fd.file_id, fd))
            } else {
                None
            }
        })
        .collect::<HashMap<FileId, &FileDescriptionPacket>>()
        .into_values()
        .collect()
}

/// Extract slice checksums indexed by file ID
///
/// Returns a map from FileId to vectors of (MD5, CRC32) checksum pairs for each block
pub fn extract_slice_checksums(packets: &[Packet]) -> HashMap<FileId, Vec<(Md5Hash, Crc32Value)>> {
    packets
        .iter()
        .filter_map(|p| {
            if let Packet::InputFileSliceChecksum(ifsc) = p {
                Some((ifsc.file_id, ifsc.slice_checksums.clone()))
            } else {
                None
            }
        })
        .collect()
}

/// Extract main packet statistics (block size and total blocks)
///
/// Returns (block_size, total_blocks) calculated from MainPacket and FileDescriptions
/// This is more efficient than calculating separately
pub fn extract_main_stats(packets: &[Packet]) -> (u64, usize) {
    let main_packet = extract_main_packet(packets);
    let block_size = main_packet.map(|m| m.slice_size).unwrap_or(0);

    let total_blocks = if block_size > 0 {
        extract_file_descriptions(packets)
            .into_iter()
            .map(|fd| fd.file_length.div_ceil(block_size) as usize)
            .sum()
    } else {
        0
    };

    (block_size, total_blocks)
}

/// Extract file information as a map from filename to (file_id, md5_hash, file_length)
///
/// This is useful for file analysis and verification operations
pub fn extract_file_info(packets: &[Packet]) -> HashMap<String, (FileId, Md5Hash, u64)> {
    extract_file_descriptions(packets)
        .into_iter()
        .filter_map(|fd| {
            std::str::from_utf8(&fd.file_name).ok().map(|file_name| {
                let clean_name = file_name.trim_end_matches('\0').to_string();
                (clean_name, (fd.file_id, fd.md5_hash, fd.file_length))
            })
        })
        .collect()
}

/// Extract file descriptions in main packet order
///
/// Returns FileDescriptions in the same order as specified in MainPacket.file_ids
/// This is critical for correct global slice indexing in repair operations
pub fn extract_ordered_file_descriptions(packets: &[Packet]) -> Vec<FileDescriptionPacket> {
    let main_packet = match extract_main_packet(packets) {
        Some(main) => main,
        None => return Vec::new(),
    };

    // Build lookup map for O(1) access
    let fd_map: HashMap<FileId, FileDescriptionPacket> = packets
        .iter()
        .filter_map(|p| {
            if let Packet::FileDescription(fd) = p {
                Some((fd.file_id, fd.clone()))
            } else {
                None
            }
        })
        .collect();

    // Return files in main packet order
    main_packet
        .file_ids
        .iter()
        .filter_map(|file_id| fd_map.get(file_id).cloned())
        .collect()
}

/// Separate packets by type for efficient processing
///
/// Returns (main_packet, file_descriptions, slice_checksums, recovery_count)
/// This is useful when you need to process multiple packet types efficiently
pub fn separate_packets(
    packets: Vec<Packet>,
) -> (
    Option<MainPacket>,
    Vec<FileDescriptionPacket>,
    Vec<crate::packets::InputFileSliceChecksumPacket>,
    usize,
) {
    let mut main_packet = None;
    let mut file_descriptions = Vec::new();
    let mut slice_checksums = Vec::new();
    let mut recovery_count = 0;

    for packet in packets {
        match packet {
            Packet::Main(main) => main_packet = Some(main),
            Packet::FileDescription(fd) => file_descriptions.push(fd),
            Packet::InputFileSliceChecksum(ifsc) => slice_checksums.push(ifsc),
            Packet::RecoverySlice(_) => recovery_count += 1,
            _ => {} // Ignore other packet types
        }
    }

    (
        main_packet,
        file_descriptions,
        slice_checksums,
        recovery_count,
    )
}

/// Extract clean filenames from file description packets
///
/// Returns a vector of clean filenames (null-terminated strings cleaned up)
pub fn extract_filenames(packets: &[Packet]) -> Vec<String> {
    extract_file_descriptions(packets)
        .into_iter()
        .filter_map(|fd| {
            std::str::from_utf8(&fd.file_name)
                .ok()
                .map(|name| name.trim_end_matches('\0').to_string())
        })
        .collect::<std::collections::HashSet<_>>() // Deduplicate
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FileId, Md5Hash, RecoverySetId};
    use crate::packets::{FileDescriptionPacket, MainPacket};

    fn create_test_main_packet() -> MainPacket {
        MainPacket {
            length: 92,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([1; 16]),
            slice_size: 1024,
            file_count: 2,
            file_ids: vec![FileId::new([1; 16]), FileId::new([2; 16])],
            non_recovery_file_ids: vec![],
        }
    }

    fn create_test_file_description(
        file_id: FileId,
        name: &str,
        length: u64,
    ) -> FileDescriptionPacket {
        FileDescriptionPacket {
            length: 120,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([1; 16]),
            packet_type: *b"PAR 2.0\0FileDesc",
            file_id,
            md5_hash: Md5Hash::new([3; 16]),
            md5_16k: Md5Hash::new([4; 16]),
            file_length: length,
            file_name: name.as_bytes().to_vec(),
        }
    }

    #[test]
    fn test_extract_main_packet() {
        let packets = vec![
            Packet::Main(create_test_main_packet()),
            Packet::FileDescription(create_test_file_description(
                FileId::new([1; 16]),
                "test.txt",
                2048,
            )),
        ];

        let main = extract_main_packet(&packets);
        assert!(main.is_some());
        assert_eq!(main.unwrap().slice_size, 1024);
    }

    #[test]
    fn test_extract_main_stats() {
        let packets = vec![
            Packet::Main(create_test_main_packet()),
            Packet::FileDescription(create_test_file_description(
                FileId::new([1; 16]),
                "file1.txt",
                2048,
            )),
            Packet::FileDescription(create_test_file_description(
                FileId::new([2; 16]),
                "file2.txt",
                3072,
            )),
        ];

        let (block_size, total_blocks) = extract_main_stats(&packets);
        assert_eq!(block_size, 1024);
        assert_eq!(total_blocks, 5); // 2 blocks + 3 blocks
    }

    #[test]
    fn test_extract_file_descriptions_deduplication() {
        let fd1 = create_test_file_description(FileId::new([1; 16]), "test.txt", 1024);
        let fd2 = create_test_file_description(FileId::new([1; 16]), "test.txt", 2048); // Same file_id, different data

        let packets = vec![Packet::FileDescription(fd1), Packet::FileDescription(fd2)];

        let descriptions = extract_file_descriptions(&packets);
        assert_eq!(descriptions.len(), 1); // Should be deduplicated
        assert_eq!(descriptions[0].file_length, 2048); // Should keep the last one
    }

    #[test]
    fn test_extract_filenames() {
        let packets = vec![
            Packet::FileDescription(create_test_file_description(
                FileId::new([1; 16]),
                "file1.txt\0\0",
                1024,
            )),
            Packet::FileDescription(create_test_file_description(
                FileId::new([2; 16]),
                "file2.txt\0",
                2048,
            )),
        ];

        let mut filenames = extract_filenames(&packets);
        filenames.sort(); // Sort to make test deterministic
        assert_eq!(filenames, vec!["file1.txt", "file2.txt"]);
    }

    #[test]
    fn test_extract_ordered_file_descriptions() {
        let main = MainPacket {
            length: 92,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([1; 16]),
            slice_size: 1024,
            file_count: 2,
            file_ids: vec![FileId::new([2; 16]), FileId::new([1; 16])], // Note: order 2, 1
            non_recovery_file_ids: vec![],
        };

        let packets = vec![
            Packet::Main(main),
            Packet::FileDescription(create_test_file_description(
                FileId::new([1; 16]),
                "first.txt",
                1024,
            )),
            Packet::FileDescription(create_test_file_description(
                FileId::new([2; 16]),
                "second.txt",
                2048,
            )),
        ];

        let ordered_fds = extract_ordered_file_descriptions(&packets);
        assert_eq!(ordered_fds.len(), 2);

        // Should be in main packet order: [2; 16] first, then [1; 16]
        assert_eq!(ordered_fds[0].file_id, FileId::new([2; 16]));
        assert_eq!(ordered_fds[1].file_id, FileId::new([1; 16]));
    }
}
