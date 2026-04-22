//! Comprehensive tests for repair/types.rs module
//!
//! These tests cover the data types used in PAR2 repair operations.

use par2rs::domain::{
    BlockCount, BlockSize, FileId, FileSize, GlobalSliceIndex, LocalSliceIndex, Md5Hash,
    RecoverySetId,
};
use par2rs::repair::{
    FileInfo, FileStatus, ReconstructedSlices, RecoverySetInfo, RepairResult, ValidationCache,
    VerificationResult,
};
use rustc_hash::FxHashSet;

#[test]
fn test_file_info_local_to_global() {
    let file_info = FileInfo {
        file_id: FileId::new([1u8; 16]),
        file_name: "test.dat".to_string(),
        file_length: FileSize::new(1024),
        md5_hash: Md5Hash::new([0u8; 16]),
        md5_16k: Md5Hash::new([0u8; 16]),
        slice_count: BlockCount::new(10),
        global_slice_offset: GlobalSliceIndex::new(5),
    };

    // Local slice 0 should map to global slice 5
    let global = file_info.local_to_global(LocalSliceIndex::new(0));
    assert_eq!(global.as_usize(), 5);

    // Local slice 3 should map to global slice 8
    let global = file_info.local_to_global(LocalSliceIndex::new(3));
    assert_eq!(global.as_usize(), 8);
}

#[test]
fn test_file_info_global_to_local() {
    let file_info = FileInfo {
        file_id: FileId::new([1u8; 16]),
        file_name: "test.dat".to_string(),
        file_length: FileSize::new(1024),
        md5_hash: Md5Hash::new([0u8; 16]),
        md5_16k: Md5Hash::new([0u8; 16]),
        slice_count: BlockCount::new(10),
        global_slice_offset: GlobalSliceIndex::new(5),
    };

    // Global slice 5 should map to local slice 0
    let local = file_info.global_to_local(GlobalSliceIndex::new(5));
    assert_eq!(local, Some(LocalSliceIndex::new(0)));

    // Global slice 8 should map to local slice 3
    let local = file_info.global_to_local(GlobalSliceIndex::new(8));
    assert_eq!(local, Some(LocalSliceIndex::new(3)));

    // Global slice 14 should map to local slice 9 (last slice)
    let local = file_info.global_to_local(GlobalSliceIndex::new(14));
    assert_eq!(local, Some(LocalSliceIndex::new(9)));

    // Global slice 15 should be out of range
    let local = file_info.global_to_local(GlobalSliceIndex::new(15));
    assert_eq!(local, None);

    // Global slice 4 should be out of range (before this file)
    let local = file_info.global_to_local(GlobalSliceIndex::new(4));
    assert_eq!(local, None);
}

#[test]
fn test_recovery_set_info_total_blocks() {
    let set_info = RecoverySetInfo {
        set_id: RecoverySetId::new([0u8; 16]),
        slice_size: BlockSize::new(1024),
        files: vec![
            FileInfo {
                file_id: FileId::new([1u8; 16]),
                file_name: "file1.dat".to_string(),
                file_length: FileSize::new(3072),
                md5_hash: Md5Hash::new([0u8; 16]),
                md5_16k: Md5Hash::new([0u8; 16]),
                slice_count: BlockCount::new(3),
                global_slice_offset: GlobalSliceIndex::new(0),
            },
            FileInfo {
                file_id: FileId::new([2u8; 16]),
                file_name: "file2.dat".to_string(),
                file_length: FileSize::new(5120),
                md5_hash: Md5Hash::new([0u8; 16]),
                md5_16k: Md5Hash::new([0u8; 16]),
                slice_count: BlockCount::new(5),
                global_slice_offset: GlobalSliceIndex::new(3),
            },
        ],
        recovery_slices_metadata: vec![],
        file_slice_checksums: Default::default(),
    };

    assert_eq!(set_info.total_blocks(), 8);
}

#[test]
fn test_recovery_set_info_total_size() {
    let set_info = RecoverySetInfo {
        set_id: RecoverySetId::new([0u8; 16]),
        slice_size: BlockSize::new(1024),
        files: vec![
            FileInfo {
                file_id: FileId::new([1u8; 16]),
                file_name: "file1.dat".to_string(),
                file_length: FileSize::new(3072),
                md5_hash: Md5Hash::new([0u8; 16]),
                md5_16k: Md5Hash::new([0u8; 16]),
                slice_count: BlockCount::new(3),
                global_slice_offset: GlobalSliceIndex::new(0),
            },
            FileInfo {
                file_id: FileId::new([2u8; 16]),
                file_name: "file2.dat".to_string(),
                file_length: FileSize::new(5120),
                md5_hash: Md5Hash::new([0u8; 16]),
                md5_16k: Md5Hash::new([0u8; 16]),
                slice_count: BlockCount::new(5),
                global_slice_offset: GlobalSliceIndex::new(3),
            },
        ],
        recovery_slices_metadata: vec![],
        file_slice_checksums: Default::default(),
    };

    assert_eq!(set_info.total_size(), 3072 + 5120);
}

#[test]
fn test_recovery_set_info_print_statistics() {
    let set_info = RecoverySetInfo {
        set_id: RecoverySetId::new([0u8; 16]),
        slice_size: BlockSize::new(1024),
        files: vec![FileInfo {
            file_id: FileId::new([1u8; 16]),
            file_name: "test.dat".to_string(),
            file_length: FileSize::new(2048),
            md5_hash: Md5Hash::new([0u8; 16]),
            md5_16k: Md5Hash::new([0u8; 16]),
            slice_count: BlockCount::new(2),
            global_slice_offset: GlobalSliceIndex::new(0),
        }],
        recovery_slices_metadata: vec![],
        file_slice_checksums: Default::default(),
    };

    // Just verify it doesn't panic
    set_info.print_statistics();
}

#[test]
fn test_file_status_needs_repair() {
    assert!(!FileStatus::Present.needs_repair());
    assert!(FileStatus::Missing.needs_repair());
    assert!(FileStatus::Corrupted.needs_repair());
}

#[test]
fn test_verification_result_equality() {
    assert_eq!(VerificationResult::Verified, VerificationResult::Verified);
    assert_eq!(
        VerificationResult::SizeMismatch {
            expected: 100,
            actual: 90
        },
        VerificationResult::SizeMismatch {
            expected: 100,
            actual: 90
        }
    );
    assert_eq!(
        VerificationResult::HashMismatch,
        VerificationResult::HashMismatch
    );
}

#[test]
fn test_repair_result_success() {
    let result = RepairResult::Success {
        files_repaired: 2,
        files_verified: 3,
        repaired_files: vec!["file1.dat".to_string(), "file2.dat".to_string()],
        verified_files: vec!["file3.dat".to_string()],
        message: "Success".to_string(),
    };

    assert!(result.is_success());
    assert_eq!(result.repaired_files().len(), 2);
    assert_eq!(result.failed_files().len(), 0);
}

#[test]
fn test_repair_result_no_repair_needed() {
    let result = RepairResult::NoRepairNeeded {
        files_verified: 3,
        verified_files: vec![
            "file1.dat".to_string(),
            "file2.dat".to_string(),
            "file3.dat".to_string(),
        ],
        message: "All good".to_string(),
    };

    assert!(result.is_success());
    assert_eq!(result.repaired_files().len(), 0);
    assert_eq!(result.failed_files().len(), 0);
}

#[test]
fn test_repair_result_failed() {
    let result = RepairResult::Failed {
        files_failed: vec!["file1.dat".to_string(), "file2.dat".to_string()],
        files_verified: 1,
        verified_files: vec!["file3.dat".to_string()],
        message: "Insufficient recovery blocks".to_string(),
    };

    assert!(!result.is_success());
    assert_eq!(result.repaired_files().len(), 0);
    assert_eq!(result.failed_files().len(), 2);
    assert_eq!(result.failed_files()[0], "file1.dat");
}

#[test]
fn test_repair_result_print() {
    // Test all variants print without panic
    let success = RepairResult::Success {
        files_repaired: 1,
        files_verified: 2,
        repaired_files: vec!["file1.dat".to_string()],
        verified_files: vec!["file2.dat".to_string()],
        message: "Done".to_string(),
    };
    success.print_result();

    let no_repair = RepairResult::NoRepairNeeded {
        files_verified: 3,
        verified_files: vec!["file1.dat".to_string()],
        message: "All good".to_string(),
    };
    no_repair.print_result();

    let failed = RepairResult::Failed {
        files_failed: vec!["file1.dat".to_string()],
        files_verified: 0,
        verified_files: vec![],
        message: "Not enough blocks".to_string(),
    };
    failed.print_result();
}

#[test]
fn test_validation_cache_new() {
    let cache = ValidationCache::new();
    let file_id = FileId::new([1u8; 16]);

    assert!(!cache.is_valid(&file_id, 0));
    assert_eq!(cache.valid_count(&file_id), 0);
}

#[test]
fn test_validation_cache_insert_and_check() {
    let mut cache = ValidationCache::new();
    let file_id = FileId::new([1u8; 16]);

    let mut valid_slices = FxHashSet::default();
    valid_slices.insert(0);
    valid_slices.insert(2);
    valid_slices.insert(5);

    cache.insert(file_id, valid_slices);

    assert!(cache.is_valid(&file_id, 0));
    assert!(!cache.is_valid(&file_id, 1));
    assert!(cache.is_valid(&file_id, 2));
    assert!(!cache.is_valid(&file_id, 3));
    assert!(cache.is_valid(&file_id, 5));
    assert_eq!(cache.valid_count(&file_id), 3);
}

#[test]
fn test_validation_cache_get() {
    let mut cache = ValidationCache::new();
    let file_id = FileId::new([1u8; 16]);

    let mut valid_slices = FxHashSet::default();
    valid_slices.insert(0);
    valid_slices.insert(1);

    cache.insert(file_id, valid_slices);

    let slices = cache.get(&file_id);
    assert!(slices.is_some());
    assert_eq!(slices.unwrap().len(), 2);

    let other_file = FileId::new([2u8; 16]);
    assert!(cache.get(&other_file).is_none());
}

#[test]
fn test_validation_cache_default() {
    let cache = ValidationCache::default();
    let file_id = FileId::new([1u8; 16]);

    assert_eq!(cache.valid_count(&file_id), 0);
}

#[test]
fn test_reconstructed_slices_new() {
    let slices = ReconstructedSlices::new();
    assert_eq!(slices.len(), 0);
    assert!(slices.is_empty());
}

#[test]
fn test_reconstructed_slices_insert_and_get() {
    let mut slices = ReconstructedSlices::new();

    slices.insert(0, vec![1, 2, 3]);
    slices.insert(2, vec![4, 5, 6, 7]);

    assert_eq!(slices.len(), 2);
    assert!(!slices.is_empty());

    assert_eq!(slices.get(0), Some(&[1, 2, 3][..]));
    assert_eq!(slices.get(1), None);
    assert_eq!(slices.get(2), Some(&[4, 5, 6, 7][..]));
}

#[test]
fn test_reconstructed_slices_iter() {
    let mut slices = ReconstructedSlices::new();

    slices.insert(0, vec![1, 2]);
    slices.insert(5, vec![3, 4, 5]);
    slices.insert(10, vec![6]);

    let mut items: Vec<_> = slices.iter().collect();
    items.sort_by_key(|(idx, _)| *idx);

    assert_eq!(items.len(), 3);
    assert_eq!(items[0], (0, &[1, 2][..]));
    assert_eq!(items[1], (5, &[3, 4, 5][..]));
    assert_eq!(items[2], (10, &[6][..]));
}

#[test]
fn test_reconstructed_slices_default() {
    let slices = ReconstructedSlices::default();
    assert_eq!(slices.len(), 0);
    assert!(slices.is_empty());
}

#[test]
fn test_file_info_roundtrip() {
    // Test converting local -> global -> local
    let file_info = FileInfo {
        file_id: FileId::new([1u8; 16]),
        file_name: "roundtrip.dat".to_string(),
        file_length: FileSize::new(10240),
        md5_hash: Md5Hash::new([0u8; 16]),
        md5_16k: Md5Hash::new([0u8; 16]),
        slice_count: BlockCount::new(10),
        global_slice_offset: GlobalSliceIndex::new(100),
    };

    for local_idx in 0..10 {
        let local = LocalSliceIndex::new(local_idx);
        let global = file_info.local_to_global(local);
        let back_to_local = file_info.global_to_local(global);

        assert_eq!(back_to_local, Some(local));
    }
}

#[test]
fn test_file_status_all_variants() {
    // Ensure all FileStatus variants are tested
    let present = FileStatus::Present;
    let missing = FileStatus::Missing;
    let corrupted = FileStatus::Corrupted;

    // Test Copy trait
    let _present_copy = present;
    let _missing_copy = missing;
    let _corrupted_copy = corrupted;

    // Test PartialEq
    assert_eq!(present, FileStatus::Present);
    assert_eq!(missing, FileStatus::Missing);
    assert_eq!(corrupted, FileStatus::Corrupted);
    assert_ne!(present, missing);
}

#[test]
fn test_empty_recovery_set() {
    let set_info = RecoverySetInfo {
        set_id: RecoverySetId::new([0u8; 16]),
        slice_size: BlockSize::new(1024),
        files: vec![],
        recovery_slices_metadata: vec![],
        file_slice_checksums: Default::default(),
    };

    assert_eq!(set_info.total_blocks(), 0);
    assert_eq!(set_info.total_size(), 0);
}
