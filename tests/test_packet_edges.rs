//! Organized packet and slice provider tests

use binrw::BinReaderExt;
use par2rs::domain::{FileId, Md5Hash, RecoverySetId};
use par2rs::packets::{InputFileSliceChecksumPacket, Packet, RecoverySlicePacket};
use par2rs::slice_provider::{ChunkedSliceProvider, SliceLocation, SliceProvider};
use std::fs::File;
use std::io::Cursor;

mod packet_parsing {
    use super::*;

    #[test]
    fn missing_magic_bytes() {
        let data = vec![0xFF; 8];
        let mut cursor = Cursor::new(data);
        assert!(Packet::parse(&mut cursor).is_none());
    }

    #[test]
    fn invalid_length_too_small() {
        let mut data = vec![0u8; 128];
        data[0..8].copy_from_slice(b"PAR2\0PKT");
        data[8..16].copy_from_slice(&(32u64).to_le_bytes());
        data[48..64].copy_from_slice(b"PAR 2.0\0MainPack");
        let mut cursor = Cursor::new(data);
        assert!(Packet::parse(&mut cursor).is_none());
    }

    #[test]
    fn invalid_length_too_large() {
        let mut data = vec![0u8; 128];
        data[0..8].copy_from_slice(b"PAR2\0PKT");
        data[8..16].copy_from_slice(&(200_000_000u64).to_le_bytes());
        data[48..64].copy_from_slice(b"PAR 2.0\0MainPack");
        let mut cursor = Cursor::new(data);
        assert!(Packet::parse(&mut cursor).is_none());
    }

    #[test]
    fn incomplete_data() {
        let data = vec![0u8; 10];
        let mut cursor = Cursor::new(data);
        assert!(Packet::parse(&mut cursor).is_none());
    }

    #[test]
    fn verify_dispatch() {
        use std::io::Read;
        if let Ok(mut file) = File::open("tests/fixtures/packets/MainPacket.par2") {
            let mut buffer = Vec::new();
            if file.read_to_end(&mut buffer).is_ok() {
                let mut cursor = Cursor::new(&buffer);
                if let Some(packet) = Packet::parse(&mut cursor) {
                    let _ = packet.verify();
                }
            }
        }
    }
}

mod slice_provider {
    use super::*;

    #[test]
    fn slice_not_found() {
        let provider = ChunkedSliceProvider::new(1024);
        assert_eq!(provider.get_slice_size(999), None);
    }

    #[test]
    fn available_slices_empty() {
        let provider = ChunkedSliceProvider::new(1024);
        assert_eq!(provider.available_slices(), vec![]);
    }

    #[test]
    fn is_slice_available() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().unwrap();
        let test_data = vec![0x55u8; 512];
        temp_file.write_all(&test_data).unwrap();
        temp_file.flush().unwrap();

        let mut provider = ChunkedSliceProvider::new(1024);
        provider.add_slice(
            10,
            SliceLocation {
                file_path: temp_file.path().to_path_buf(),
                offset: 0,
                actual_size: ActualDataSize::new(512),
                logical_size: LogicalSliceSize::new(512),
                expected_crc: None,
            },
        );

        assert!(provider.is_slice_available(10));
        assert!(!provider.is_slice_available(11));
    }
}

mod recovery_packets {
    use super::*;

    #[test]
    fn recovery_slice_basic() {
        if let Ok(mut file) = File::open("tests/fixtures/packets/RecoverySlicePacket.par2") {
            if let Ok(packet) = file.read_le::<RecoverySlicePacket>() {
                let _ = packet.verify();
            }
        }
    }

    #[test]
    fn input_checksum_basic() {
        if let Ok(mut file) = File::open("tests/fixtures/packets/InputFileSliceChecksumPacket.par2")
        {
            if let Ok(packet) = file.read_le::<InputFileSliceChecksumPacket>() {
                let _ = packet.verify();
            }
        }
    }
}

mod packet_parser {
    use super::*;

    #[test]
    fn parse_multiple_valid() {
        if let Ok(mut file) = File::open("tests/fixtures/packets/MainPacket.par2") {
            let packets = par2rs::packets::parse_packets(&mut file);
            assert!(!packets.is_empty());
        }
    }

    #[test]
    fn parse_empty_file() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let packets = par2rs::packets::parse_packets(&mut cursor);
        assert!(packets.is_empty());
    }

    #[test]
    fn parse_invalid_data() {
        let mut cursor = Cursor::new(vec![0xFF; 1000]);
        let packets = par2rs::packets::parse_packets(&mut cursor);
        assert!(packets.is_empty());
    }
}

mod recovery_set_info {
    use super::*;
    use par2rs::repair::RecoverySetInfo;

    #[test]
    fn total_blocks_empty() {
        let info = RecoverySetInfo {
            set_id: RecoverySetId::new([0u8; 16]),
            slice_size: 1024,
            files: Vec::new(),
            recovery_slices_metadata: Vec::new(),
            file_slice_checksums: Default::default(),
        };
        assert_eq!(info.total_blocks(), 0);
    }

    #[test]
    fn total_size_empty() {
        let info = RecoverySetInfo {
            set_id: RecoverySetId::new([0u8; 16]),
            slice_size: 1024,
            files: Vec::new(),
            recovery_slices_metadata: Vec::new(),
            file_slice_checksums: Default::default(),
        };
        assert_eq!(info.total_size(), 0);
    }
}

mod file_info {
    use super::*;
    use par2rs::domain::{GlobalSliceIndex, LocalSliceIndex};
    use par2rs::repair::FileInfo;

    #[test]
    fn local_to_global() {
        let file = FileInfo {
            file_id: FileId::new([0u8; 16]),
            file_name: "test.bin".to_string(),
            file_length: 10240,
            md5_hash: Md5Hash::new([0u8; 16]),
            md5_16k: Md5Hash::new([0u8; 16]),
            slice_count: 10,
            global_slice_offset: GlobalSliceIndex::new(100),
        };

        assert_eq!(
            file.local_to_global(LocalSliceIndex::new(0)).as_usize(),
            100
        );
        assert_eq!(
            file.local_to_global(LocalSliceIndex::new(5)).as_usize(),
            105
        );
    }

    #[test]
    fn global_to_local() {
        let file = FileInfo {
            file_id: FileId::new([0u8; 16]),
            file_name: "test.bin".to_string(),
            file_length: 10240,
            md5_hash: Md5Hash::new([0u8; 16]),
            md5_16k: Md5Hash::new([0u8; 16]),
            slice_count: 10,
            global_slice_offset: GlobalSliceIndex::new(50),
        };

        assert_eq!(
            file.global_to_local(GlobalSliceIndex::new(50))
                .unwrap()
                .as_usize(),
            0
        );
        assert_eq!(
            file.global_to_local(GlobalSliceIndex::new(55))
                .unwrap()
                .as_usize(),
            5
        );
        assert_eq!(file.global_to_local(GlobalSliceIndex::new(40)), None);
        assert_eq!(file.global_to_local(GlobalSliceIndex::new(60)), None);
    }
}
