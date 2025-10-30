//! Tests for recovery_loader module
//!
//! Tests for the pluggable recovery data loading system including
//! FileSystemLoader and trait implementations.

use par2rs::repair::{FileSystemLoader, RecoveryDataLoader};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use tempfile::TempDir;

// Helper: Create test file with specific content
fn create_test_file(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(content)?;
    Ok(())
}

mod filesystem_loader_tests {
    use super::*;

    #[test]
    fn loads_full_data() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test_data.bin");
        let content = b"Hello, World! This is test data.";

        create_test_file(&test_file, content).unwrap();

        let loader = FileSystemLoader {
            file_path: test_file.clone(),
            data_offset: 0,
            data_size: content.len(),
        };

        let loaded_data = loader.load_data().unwrap();
        assert_eq!(loaded_data, content);
    }

    #[test]
    fn loads_data_with_offset() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test_offset.bin");
        // "[HEADER]...[DATA_START]This is the actual data[DATA_END]...[FOOTER]"
        // Prefix: "[HEADER]...[DATA_START]" = 22 bytes
        let prefix = b"[HEADER]...[DATA_START]";
        let data_content = b"This is the actual data";
        let suffix = b"[DATA_END]...[FOOTER]";

        let mut full_content = Vec::new();
        full_content.extend_from_slice(prefix);
        full_content.extend_from_slice(data_content);
        full_content.extend_from_slice(suffix);

        create_test_file(&test_file, &full_content).unwrap();

        let data_start = prefix.len() as u64;
        let data_size = data_content.len();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: data_start,
            data_size,
        };

        let loaded_data = loader.load_data().unwrap();
        assert_eq!(loaded_data.len(), data_size);
        assert_eq!(&loaded_data[..], data_content);
    }

    #[test]
    fn fails_on_nonexistent_file() {
        let loader = FileSystemLoader {
            file_path: Path::new("/nonexistent/path/file.bin").to_path_buf(),
            data_offset: 0,
            data_size: 100,
        };

        let result = loader.load_data();
        assert!(result.is_err());
    }

    #[test]
    fn fails_on_insufficient_data() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("short.bin");
        create_test_file(&test_file, b"short").unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: 1000, // More than file contains
        };

        let result = loader.load_data();
        assert!(result.is_err());
    }

    #[test]
    fn handles_exact_data_boundary() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("exact.bin");
        let content = b"EXACT SIZE DATA";

        create_test_file(&test_file, content).unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: content.len(),
        };

        let loaded_data = loader.load_data().unwrap();
        assert_eq!(loaded_data, content);
    }

    #[test]
    fn loads_zero_bytes() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("zero.bin");
        create_test_file(&test_file, b"").unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: 0,
        };

        let loaded_data = loader.load_data().unwrap();
        assert!(loaded_data.is_empty());
    }

    #[test]
    fn handles_large_files() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("large.bin");

        // Create 1MB file
        let large_content = vec![0xABu8; 1024 * 1024];
        create_test_file(&test_file, &large_content).unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: large_content.len(),
        };

        let loaded_data = loader.load_data().unwrap();
        assert_eq!(loaded_data.len(), 1024 * 1024);
        assert!(loaded_data.iter().all(|&b| b == 0xABu8));
    }

    #[test]
    fn loads_chunk_from_beginning() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("chunk_begin.bin");
        let content = b"CHUNK TEST DATA FULL";

        create_test_file(&test_file, content).unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: content.len(),
        };

        let chunk = loader.load_chunk(0, 5).unwrap();
        assert_eq!(chunk, b"CHUNK");
    }

    #[test]
    fn loads_chunk_from_middle() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("chunk_middle.bin");
        let content = b"START[MIDDLE]END";

        create_test_file(&test_file, content).unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: content.len(),
        };

        let chunk = loader.load_chunk(5, 8).unwrap();
        assert_eq!(chunk, b"[MIDDLE]");
    }

    #[test]
    fn loads_chunk_to_end() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("chunk_end.bin");
        let content = b"COMPLETE MESSAGE";

        create_test_file(&test_file, content).unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: content.len(),
        };

        let chunk = loader.load_chunk(9, 20).unwrap();
        assert_eq!(chunk, b"MESSAGE"); // Only 7 bytes remain, not 20
    }

    #[test]
    fn chunk_beyond_data_returns_empty() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("chunk_beyond.bin");
        let content = b"SHORT";

        create_test_file(&test_file, content).unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: content.len(),
        };

        let chunk = loader.load_chunk(100, 50).unwrap();
        assert!(chunk.is_empty());
    }

    #[test]
    fn chunk_with_file_offset() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("chunk_file_offset.bin");
        let full_content = b"HEADER_DATA[ACTUAL_DATA]FOOTER";

        create_test_file(&test_file, full_content).unwrap();

        // Skip "HEADER_DATA" (11 bytes), access from there
        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 11,
            data_size: 11, // Length of "[ACTUAL_DATA]"
        };

        let chunk = loader.load_chunk(0, 7).unwrap();
        assert_eq!(chunk, b"[ACTUAL");
    }

    #[test]
    fn chunk_respects_data_size_boundary() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("chunk_boundary.bin");
        let full_content = b"HEADER[LIMITED]DATA[EXTRA_NOT_USED]";

        create_test_file(&test_file, full_content).unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 6, // Start at "[LIMITED"
            data_size: 9,   // Only read "[LIMITED]" (9 bytes)
        };

        // Try to read 100 bytes - should only get 9 due to data_size limit
        let chunk = loader.load_chunk(0, 100).unwrap();
        assert_eq!(chunk, b"[LIMITED]");
    }

    #[test]
    fn data_size_method_returns_correct_size() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("size_test.bin");
        let content: Vec<u8> = "x".repeat(12345).into_bytes();
        create_test_file(&test_file, &content).unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 100,
            data_size: 5000,
        };

        assert_eq!(loader.data_size(), 5000);
    }

    #[test]
    fn data_size_zero() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("zero_size.bin");
        create_test_file(&test_file, b"data").unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: 0,
        };

        assert_eq!(loader.data_size(), 0);
    }

    #[test]
    fn clone_creates_independent_loader() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("clone_test.bin");
        create_test_file(&test_file, b"test data").unwrap();

        let loader1 = FileSystemLoader {
            file_path: test_file.clone(),
            data_offset: 0,
            data_size: 4,
        };

        let loader2 = loader1.clone();

        assert_eq!(loader1.data_size(), loader2.data_size());
        assert_eq!(
            loader1.load_chunk(0, 2).unwrap(),
            loader2.load_chunk(0, 2).unwrap()
        );
    }

    #[test]
    fn debug_format_contains_useful_info() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("debug_test.bin");
        create_test_file(&test_file, b"test").unwrap();

        let loader = FileSystemLoader {
            file_path: test_file.clone(),
            data_offset: 10,
            data_size: 100,
        };

        let debug_str = format!("{:?}", loader);
        assert!(debug_str.contains("FileSystemLoader"));
        assert!(debug_str.contains("10")); // offset
        assert!(debug_str.contains("100")); // size
    }
}

mod recovery_data_loader_trait_tests {
    use super::*;

    #[test]
    fn trait_object_works_with_filesystem_loader() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("trait_test.bin");
        let content = b"TRAIT TEST";

        create_test_file(&test_file, content).unwrap();

        let loader: Box<dyn RecoveryDataLoader> = Box::new(FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: content.len(),
        });

        let loaded = loader.load_data().unwrap();
        assert_eq!(loaded, content);
    }

    #[test]
    fn trait_object_chunk_loading() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("trait_chunk.bin");
        let content = b"ABCDEFGHIJ";

        create_test_file(&test_file, content).unwrap();

        let loader: Box<dyn RecoveryDataLoader> = Box::new(FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: content.len(),
        });

        let chunk = loader.load_chunk(2, 4).unwrap();
        assert_eq!(chunk, b"CDEF");
    }

    #[test]
    fn trait_object_data_size() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("trait_size.bin");
        create_test_file(&test_file, b"test").unwrap();

        let loader: Box<dyn RecoveryDataLoader> = Box::new(FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: 2048,
        });

        assert_eq!(loader.data_size(), 2048);
    }
}

mod edge_case_tests {
    use super::*;

    #[test]
    fn handles_zero_chunk_size() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("zero_chunk.bin");
        create_test_file(&test_file, b"data").unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: 4,
        };

        let chunk = loader.load_chunk(0, 0).unwrap();
        assert!(chunk.is_empty());
    }

    #[test]
    fn handles_chunk_at_exact_boundary() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("boundary.bin");
        let content = b"EXACT";

        create_test_file(&test_file, content).unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: content.len(),
        };

        let chunk = loader.load_chunk(5, 10).unwrap();
        assert!(chunk.is_empty()); // Offset beyond data
    }

    #[test]
    fn handles_very_large_offset() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("huge_offset.bin");
        create_test_file(&test_file, b"small").unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: u64::MAX - 100,
            data_size: 50,
        };

        let result = loader.load_data();
        assert!(result.is_err()); // Should fail trying to seek beyond file
    }

    #[test]
    fn handles_multiple_chunk_reads() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("multi_chunk.bin");
        let content = b"ABCDEFGHIJKLMNOPQRST";

        create_test_file(&test_file, content).unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: content.len(),
        };

        let chunk1 = loader.load_chunk(0, 5).unwrap();
        let chunk2 = loader.load_chunk(5, 5).unwrap();
        let chunk3 = loader.load_chunk(10, 5).unwrap();

        assert_eq!(chunk1, b"ABCDE");
        assert_eq!(chunk2, b"FGHIJ");
        assert_eq!(chunk3, b"KLMNO");
    }

    #[test]
    fn sequential_chunks_combine_correctly() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("sequential.bin");
        let content = b"The quick brown fox";

        create_test_file(&test_file, content).unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: content.len(),
        };

        let mut combined = Vec::new();
        for offset in (0..content.len()).step_by(5) {
            let chunk = loader.load_chunk(offset, 5).unwrap();
            combined.extend_from_slice(&chunk);
        }

        assert_eq!(&combined, content);
    }

    #[test]
    fn send_sync_trait_bounds() {
        // Verify that FileSystemLoader implements Send + Sync
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<FileSystemLoader>();
    }

    #[test]
    fn partial_read_at_file_end() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("partial_end.bin");
        create_test_file(&test_file, b"12345").unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: 5,
        };

        // Request 10 bytes starting at offset 3, but only 2 bytes remain
        let chunk = loader.load_chunk(3, 10).unwrap();
        assert_eq!(chunk, b"45");
    }

    #[test]
    fn offset_and_chunk_offset_interact_correctly() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("offset_interact.bin");
        let skip_part = b"[SKIP_ME]";
        let data_part = b"ACTUAL_DATA";

        let mut full_content = Vec::new();
        full_content.extend_from_slice(skip_part);
        full_content.extend_from_slice(data_part);

        create_test_file(&test_file, &full_content).unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: skip_part.len() as u64,
            data_size: data_part.len(),
        };

        let chunk = loader.load_chunk(0, 6).unwrap();
        assert_eq!(chunk, b"ACTUAL");

        let chunk = loader.load_chunk(6, 5).unwrap();
        assert_eq!(chunk, b"_DATA");
    }
}

mod integration_tests {
    use super::*;

    #[test]
    fn simulates_recovery_slice_loading() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("recovery_simulation.bin");

        // Simulate PAR2 file with header + recovery data
        let header = b"PAR2_HEADER";
        let recovery_data = b"RECOVERY_SLICE_DATA_CONTENT_HERE";
        let mut full_content = header.to_vec();
        full_content.extend_from_slice(recovery_data);

        create_test_file(&test_file, &full_content).unwrap();

        // Create loader for recovery data portion
        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: header.len() as u64,
            data_size: recovery_data.len(),
        };

        let loaded_recovery = loader.load_data().unwrap();
        assert_eq!(loaded_recovery, recovery_data);
    }

    #[test]
    fn loads_recovery_slice_chunks_progressively() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("progressive_load.bin");

        // Create 100KB recovery data
        let recovery_data = vec![0xAAu8; 100 * 1024];
        create_test_file(&test_file, &recovery_data).unwrap();

        let loader = FileSystemLoader {
            file_path: test_file,
            data_offset: 0,
            data_size: recovery_data.len(),
        };

        // Load in 10KB chunks
        let chunk_size = 10 * 1024;
        let mut total_loaded = 0;

        for i in 0..10 {
            let offset = i * chunk_size;
            let chunk = loader.load_chunk(offset, chunk_size).unwrap();

            assert_eq!(chunk.len(), chunk_size);
            assert!(chunk.iter().all(|&b| b == 0xAAu8));
            total_loaded += chunk.len();
        }

        assert_eq!(total_loaded, recovery_data.len());
    }

    #[test]
    fn handles_multiple_loaders_same_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("multi_loader.bin");
        let content = b"SECTION_A|SECTION_B|SECTION_C";

        create_test_file(&test_file, content).unwrap();

        // Create separate loaders for each section
        let loader_a = FileSystemLoader {
            file_path: test_file.clone(),
            data_offset: 0,
            data_size: 9, // "SECTION_A"
        };

        let loader_b = FileSystemLoader {
            file_path: test_file.clone(),
            data_offset: 10,
            data_size: 9, // "SECTION_B"
        };

        let loader_c = FileSystemLoader {
            file_path: test_file,
            data_offset: 20,
            data_size: 9, // "SECTION_C"
        };

        assert_eq!(loader_a.load_data().unwrap(), b"SECTION_A");
        assert_eq!(loader_b.load_data().unwrap(), b"SECTION_B");
        assert_eq!(loader_c.load_data().unwrap(), b"SECTION_C");
    }
}
