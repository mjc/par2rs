use par2rs::file_ops;
use par2rs::repair::RepairContext;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn debug_repair_issue() {
    let _ = env_logger::builder().is_test(true).try_init();

    // Create temp environment
    let temp_dir = TempDir::new().unwrap();
    let fixtures = PathBuf::from("tests/fixtures");

    // Copy test files
    fs::copy(fixtures.join("testfile"), temp_dir.path().join("testfile")).unwrap();
    for entry in fs::read_dir(&fixtures).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("par2") {
            fs::copy(&path, temp_dir.path().join(path.file_name().unwrap())).unwrap();
        }
    }

    let test_file = temp_dir.path().join("testfile");
    let par2_file = temp_dir.path().join("testfile.par2");

    // Read original file
    let mut original_data = Vec::new();
    File::open(&test_file)
        .unwrap()
        .read_to_end(&mut original_data)
        .unwrap();
    println!("Original file size: {} bytes", original_data.len());

    use md5::Digest;
    let original_md5: [u8; 16] = md5::Md5::digest(&original_data).into();
    println!("Original MD5: {:02x?}", original_md5);

    // Create repair context
    let par2_files = file_ops::collect_par2_files(&par2_file);
    let metadata = file_ops::parse_recovery_slice_metadata(&par2_files, false);
    let packets = file_ops::load_par2_packets(&par2_files, false);
    let context =
        RepairContext::new_with_metadata(packets, metadata, temp_dir.path().to_path_buf()).unwrap();

    println!("Recovery set info:");
    println!("  Slice size: {}", context.recovery_set.slice_size);
    println!("  File count: {}", context.recovery_set.files.len());
    println!(
        "  Recovery slices: {}",
        context.recovery_set.recovery_slices_metadata.len()
    );

    // Check file info
    let file_info = &context.recovery_set.files[0];
    println!("File info:");
    println!("  Name: {}", file_info.file_name);
    println!("  Length: {}", file_info.file_length);
    println!("  Slice count: {}", file_info.slice_count);
    println!("  Expected MD5: {:02x?}", file_info.md5_hash);

    // Verify the file is initially correct
    let status = context.check_file_status();
    println!("Initial file status: {:?}", status.get("testfile"));

    // Now corrupt specific bytes (same as failing test)
    let mut file = File::options().write(true).open(&test_file).unwrap();
    file.seek(SeekFrom::Start(1000)).unwrap();
    file.write_all(&[0xFFu8; 100]).unwrap();
    drop(file);

    // Read corrupted file
    let mut corrupted_data = Vec::new();
    File::open(&test_file)
        .unwrap()
        .read_to_end(&mut corrupted_data)
        .unwrap();
    let corrupted_md5: [u8; 16] = md5::Md5::digest(&corrupted_data).into();
    println!("Corrupted MD5: {:02x?}", corrupted_md5);

    // Check which specific slices are affected
    let valid_slices = context.validate_file_slices(file_info).unwrap();
    println!(
        "Valid slices after corruption: {} of {}",
        valid_slices.len(),
        file_info.slice_count
    );

    let missing_slices: Vec<usize> = (0..file_info.slice_count)
        .filter(|idx| !valid_slices.contains(idx))
        .collect();
    println!("Missing slice indices: {:?}", missing_slices);

    // Show which bytes each missing slice should contain
    for &slice_idx in &missing_slices {
        let offset = slice_idx * context.recovery_set.slice_size as usize;
        let size = if slice_idx == file_info.slice_count - 1 {
            let remaining = file_info.file_length % context.recovery_set.slice_size;
            if remaining == 0 {
                context.recovery_set.slice_size as usize
            } else {
                remaining as usize
            }
        } else {
            context.recovery_set.slice_size as usize
        };

        println!(
            "Missing slice {}: offset {}, size {}",
            slice_idx, offset, size
        );
        println!(
            "  Original data: {:02x?}",
            &original_data[offset..offset + 16.min(size)]
        );
        println!(
            "  Corrupted data: {:02x?}",
            &corrupted_data[offset..offset + 16.min(size)]
        );
    }

    // Attempt repair
    println!("\nAttempting repair...");
    let result = context.repair();
    match result {
        Ok(repair_result) => {
            println!("Repair result: {:?}", repair_result);

            // Read repaired file
            let mut repaired_data = Vec::new();
            File::open(&test_file)
                .unwrap()
                .read_to_end(&mut repaired_data)
                .unwrap();
            let repaired_md5: [u8; 16] = md5::Md5::digest(&repaired_data).into();
            println!("Repaired MD5: {:02x?}", repaired_md5);

            // Compare the repaired slices with original
            for &slice_idx in &missing_slices {
                let offset = slice_idx * context.recovery_set.slice_size as usize;
                let size = if slice_idx == file_info.slice_count - 1 {
                    let remaining = file_info.file_length % context.recovery_set.slice_size;
                    if remaining == 0 {
                        context.recovery_set.slice_size as usize
                    } else {
                        remaining as usize
                    }
                } else {
                    context.recovery_set.slice_size as usize
                };

                println!(
                    "Repaired slice {}: offset {}, size {}",
                    slice_idx, offset, size
                );
                println!(
                    "  Original:  {:02x?}",
                    &original_data[offset..offset + 16.min(size)]
                );
                println!(
                    "  Repaired:  {:02x?}",
                    &repaired_data[offset..offset + 16.min(size)]
                );

                // Compare entire slice, not just first 16 bytes
                let slice_matches =
                    original_data[offset..offset + size] == repaired_data[offset..offset + size];
                println!("  Matches original: {}", slice_matches);

                if !slice_matches {
                    // Find the first difference
                    for i in 0..size {
                        if original_data[offset + i] != repaired_data[offset + i] {
                            println!("  First difference at byte {} ({}): original={:02x}, repaired={:02x}", 
                                     i, offset + i, original_data[offset + i], repaired_data[offset + i]);

                            // Show some context around the difference
                            let start = (i.saturating_sub(8)).max(0);
                            let end = (i + 8).min(size);
                            println!(
                                "  Context original: {:02x?}",
                                &original_data[offset + start..offset + end]
                            );
                            println!(
                                "  Context repaired: {:02x?}",
                                &repaired_data[offset + start..offset + end]
                            );
                            break;
                        }
                    }
                }
            }
        }
        Err(e) => {
            println!("Repair failed: {}", e);
        }
    }
}
