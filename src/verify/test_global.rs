//! Tests for global block table verification

use super::*;
use crate::domain::{Crc32Value, FileId, Md5Hash};
use crate::packets::{FileDescriptionPacket, MainPacket, Packet};
use tempfile::tempdir;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_file_description(
        file_id: FileId,
        name: &str,
        length: u64,
    ) -> FileDescriptionPacket {
        use crate::domain::RecoverySetId;
        let mut packet_type = [0u8; 16];
        packet_type[..16].copy_from_slice(b"PAR 2.0\0FileDesc");
        FileDescriptionPacket {
            length: 120 + name.len() as u64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([1; 16]),
            packet_type,
            file_id,
            md5_hash: Md5Hash::new([0; 16]),
            md5_16k: Md5Hash::new([0; 16]),
            file_length: length,
            file_name: name.as_bytes().to_vec(),
        }
    }

    fn create_test_main_packet(slice_size: u64, file_ids: Vec<[u8; 16]>) -> MainPacket {
        use crate::domain::RecoverySetId;
        MainPacket {
            length: 72 + (file_ids.len() as u64 * 16),
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([1; 16]),
            slice_size,
            file_count: file_ids.len() as u32,
            file_ids: file_ids.into_iter().map(FileId::new).collect(),
            non_recovery_file_ids: vec![],
        }
    }

    fn create_test_slice_checksum_packet(
        file_id: FileId,
        block_checksums: Vec<(Md5Hash, Crc32Value)>,
    ) -> crate::packets::InputFileSliceChecksumPacket {
        use crate::domain::RecoverySetId;
        crate::packets::InputFileSliceChecksumPacket {
            length: 40 + (block_checksums.len() as u64 * 20),
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([1; 16]),
            file_id,
            slice_checksums: block_checksums,
        }
    }

    #[test]
    fn test_global_block_table_creation() {
        let table = GlobalBlockTable::new(1024);
        assert_eq!(table.block_size(), 1024);
        assert!(table.is_empty());
    }

    #[test]
    fn test_global_block_table_add_blocks() {
        let mut table = GlobalBlockTable::new(512);
        let file_id = FileId::new([1; 16]);
        let md5_hash = Md5Hash::new([2; 16]);
        let crc32 = Crc32Value::new(12345);

        table.add_block(file_id, 0, md5_hash, crc32, 512);
        table.add_block(
            file_id,
            1,
            Md5Hash::new([3; 16]),
            Crc32Value::new(54321),
            512,
        );

        assert_eq!(table.stats().total_blocks, 2);
        assert_eq!(table.stats().unique_checksums, 2);
        assert_eq!(table.stats().duplicate_blocks, 0);

        // Test lookup
        let found = table.find_by_crc32(crc32);
        assert!(found.is_some());
        assert_eq!(found.unwrap().len(), 1);
    }

    #[test]
    fn test_global_block_table_duplicates() {
        let mut table = GlobalBlockTable::new(1024);
        let file_id1 = FileId::new([1; 16]);
        let file_id2 = FileId::new([2; 16]);
        let md5_hash = Md5Hash::new([3; 16]);
        let crc32 = Crc32Value::new(99999);

        // Add same block content to different files
        table.add_block(file_id1, 0, md5_hash, crc32, 1024);
        table.add_block(file_id2, 5, md5_hash, crc32, 1024);

        assert_eq!(table.stats().total_blocks, 2);
        assert_eq!(table.stats().unique_checksums, 1);
        assert_eq!(table.stats().duplicate_blocks, 1);

        // Test exact match
        let exact = table.find_exact_match(&md5_hash, crc32);
        assert!(exact.is_some());

        let entry = exact.unwrap();
        let duplicates: Vec<_> = entry.iter_duplicates().collect();
        assert_eq!(duplicates.len(), 2);
    }

    #[test]
    fn test_global_table_builder() {
        let mut builder = GlobalBlockTableBuilder::new(2048);
        let file_id = FileId::new([4; 16]);

        let checksums = vec![
            (Md5Hash::new([5; 16]), Crc32Value::new(111)),
            (Md5Hash::new([6; 16]), Crc32Value::new(222)),
            (Md5Hash::new([7; 16]), Crc32Value::new(333)),
        ];

        builder.add_file_blocks(file_id, &checksums);
        let table = builder.build();

        assert_eq!(table.stats().total_blocks, 3);
        assert_eq!(table.stats().file_count, 1);

        let file_blocks = table.get_file_blocks(file_id);
        assert_eq!(file_blocks.len(), 3);
    }

    #[test]
    fn test_global_verification_engine_from_packets() {
        let file_id = FileId::new([2; 16]);
        let main_packet = create_test_main_packet(1024, vec![[2; 16]]);
        let file_desc = create_test_file_description(file_id, "test.txt", 1024);

        let packets = vec![
            Packet::Main(main_packet),
            Packet::FileDescription(file_desc),
        ];

        let temp_dir = tempdir().unwrap();
        let engine = GlobalVerificationEngine::from_packets(&packets, temp_dir.path());

        assert!(engine.is_ok());
        let engine = engine.unwrap();
        assert_eq!(engine.block_table().block_size(), 1024);
    }

    #[test]
    fn test_comprehensive_verify_with_global_table() {
        let file_id = FileId::new([3; 16]);
        let main_packet = create_test_main_packet(512, vec![[3; 16]]);
        let file_desc = create_test_file_description(file_id, "missing.txt", 512);

        let packets = vec![
            Packet::Main(main_packet),
            Packet::FileDescription(file_desc),
        ];

        let results = comprehensive_verify_files(packets);

        // Since the file doesn't exist, it should be reported as missing
        assert_eq!(results.missing_file_count, 1);
        assert_eq!(results.present_file_count, 0);
        assert_eq!(results.total_block_count, 1); // 512 bytes = 1 block of 512
    }

    #[test]
    fn test_global_verification_with_existing_file() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("existing.txt");

        // Create a test file
        let content = vec![42u8; 1024]; // 1KB file
        std::fs::write(&file_path, &content).unwrap();

        let file_id = FileId::new([4; 16]);
        let main_packet = create_test_main_packet(1024, vec![[4; 16]]);
        let mut file_desc = create_test_file_description(file_id, "existing.txt", 1024);

        // Calculate actual MD5 for the test file
        let actual_md5 = crate::checksum::calculate_file_md5(&file_path).unwrap();
        let actual_md5_16k = crate::checksum::calculate_file_md5_16k(&file_path).unwrap();
        file_desc.md5_hash = actual_md5;
        file_desc.md5_16k = actual_md5_16k;

        // Calculate actual block checksums for the file
        let block_checksums = crate::checksum::compute_block_checksums_padded(&content, 1024);
        let slice_checksum_packet =
            create_test_slice_checksum_packet(file_id, vec![block_checksums]);

        let packets = vec![
            Packet::Main(main_packet),
            Packet::FileDescription(file_desc),
            Packet::InputFileSliceChecksum(slice_checksum_packet),
        ];

        let results = comprehensive_verify_files_in_dir(packets, temp_dir.path());

        // File should be present since MD5 matches
        assert_eq!(results.present_file_count, 1);
        assert_eq!(results.missing_file_count, 0);
        assert_eq!(results.available_block_count, 1); // 1KB = 1 block
    }

    #[test]
    fn test_global_table_cross_file_block_detection() {
        let mut table = GlobalBlockTable::new(256);
        let file1_id = FileId::new([1; 16]);
        let file2_id = FileId::new([2; 16]);

        // Same block content in different files
        let shared_md5 = Md5Hash::new([99; 16]);
        let shared_crc = Crc32Value::new(77777);

        table.add_block(file1_id, 0, shared_md5, shared_crc, 256);
        table.add_block(file2_id, 3, shared_md5, shared_crc, 256);

        // Different content
        table.add_block(
            file1_id,
            1,
            Md5Hash::new([88; 16]),
            Crc32Value::new(88888),
            256,
        );

        assert_eq!(table.stats().total_blocks, 3);
        assert_eq!(table.stats().duplicate_blocks, 1);

        // Verify we can find the shared block from either file
        let exact_match = table.find_exact_match(&shared_md5, shared_crc).unwrap();
        let duplicates: Vec<_> = exact_match.iter_duplicates().collect();

        assert_eq!(duplicates.len(), 2);
        assert!(duplicates
            .iter()
            .any(|d| d.position.file_id == file1_id && d.position.block_number == 0));
        assert!(duplicates
            .iter()
            .any(|d| d.position.file_id == file2_id && d.position.block_number == 3));
    }

    #[test]
    fn test_verify_comprehensive_functions_integration() {
        let file_id = FileId::new([5; 16]);
        let main_packet = create_test_main_packet(1024, vec![[5; 16]]);
        let file_desc = create_test_file_description(file_id, "nonexistent.txt", 1024);

        let packets = vec![
            Packet::Main(main_packet),
            Packet::FileDescription(file_desc),
        ];

        // Test verification using global block table
        let results = comprehensive_verify_files(packets);

        // Should detect the missing file
        assert_eq!(results.missing_file_count, 1);
        assert_eq!(results.present_file_count, 0);

        // With missing files, there are no blocks available but we should have metadata
        assert_eq!(results.total_block_count, 1); // Expected 1 block from packet info
    }
}
