//! Tests for args parsing and domain types
//!
//! Tests for command-line argument parsing and domain type functionality
//! including FileId, Md5Hash, RecoverySetId, slice indices, and error handling.

use par2rs::domain::*;

// ============================================================================
// FileId Tests
// ============================================================================

#[test]
fn test_fileid_new() {
    let id = FileId::new([1u8; 16]);
    assert_eq!(id.as_bytes(), &[1u8; 16]);
}

#[test]
fn test_fileid_equality() {
    let id1 = FileId::new([1u8; 16]);
    let id2 = FileId::new([1u8; 16]);
    let id3 = FileId::new([2u8; 16]);

    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
}

#[test]
fn test_fileid_debug_display() {
    let id = FileId::new([
        255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0,
    ]);
    let debug_str = format!("{:?}", id);
    assert!(!debug_str.is_empty());
}

#[test]
fn test_fileid_copy() {
    let id1 = FileId::new([42u8; 16]);
    let id2 = id1;
    assert_eq!(id1, id2);
}

#[test]
fn test_fileid_hash() {
    use std::collections::HashMap;

    let id1 = FileId::new([1u8; 16]);
    let id2 = FileId::new([1u8; 16]);
    let id3 = FileId::new([2u8; 16]);

    let mut map = HashMap::new();
    map.insert(id1, "first");
    map.insert(id2, "second");
    map.insert(id3, "third");

    assert_eq!(map.len(), 2); // id1 and id2 should map to same key
}

// ============================================================================
// Md5Hash Tests
// ============================================================================

#[test]
fn test_md5hash_new() {
    let hash = Md5Hash::new([0; 16]);
    assert_eq!(hash.as_bytes(), &[0; 16]);
}

#[test]
fn test_md5hash_all_zeros() {
    let hash = Md5Hash::new([0; 16]);
    assert_eq!(hash.as_bytes(), &[0; 16]);
}

#[test]
fn test_md5hash_all_ones() {
    let hash = Md5Hash::new([255; 16]);
    assert_eq!(hash.as_bytes(), &[255; 16]);
}

#[test]
fn test_md5hash_mixed_values() {
    let bytes = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let hash = Md5Hash::new(bytes);
    assert_eq!(hash.as_bytes(), &bytes);
}

#[test]
fn test_md5hash_equality() {
    let hash1 = Md5Hash::new([42u8; 16]);
    let hash2 = Md5Hash::new([42u8; 16]);
    let hash3 = Md5Hash::new([43u8; 16]);

    assert_eq!(hash1, hash2);
    assert_ne!(hash1, hash3);
}

#[test]
fn test_md5hash_clone() {
    let hash1 = Md5Hash::new([0xBB; 16]);
    let hash2 = hash1;
    assert_eq!(hash1, hash2);
}

// ============================================================================
// RecoverySetId Tests
// ============================================================================

#[test]
fn test_recovery_set_id_new() {
    let id = RecoverySetId::new([5u8; 16]);
    assert_eq!(id.as_bytes(), &[5u8; 16]);
}

#[test]
fn test_recovery_set_id_equality() {
    let id1 = RecoverySetId::new([10u8; 16]);
    let id2 = RecoverySetId::new([10u8; 16]);
    let id3 = RecoverySetId::new([11u8; 16]);

    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
}

#[test]
fn test_recovery_set_id_clone() {
    let id1 = RecoverySetId::new([10u8; 16]);
    let id2 = id1;
    assert_eq!(id1, id2);
}

// ============================================================================
// LocalSliceIndex Tests
// ============================================================================

#[test]
fn test_local_slice_index_new() {
    let idx = LocalSliceIndex::new(42);
    assert_eq!(idx.as_usize(), 42);
}

#[test]
fn test_local_slice_index_zero() {
    let idx = LocalSliceIndex::new(0);
    assert_eq!(idx.as_usize(), 0);
}

#[test]
fn test_local_slice_index_large_value() {
    let idx = LocalSliceIndex::new(1_000_000);
    assert_eq!(idx.as_usize(), 1_000_000);
}

#[test]
fn test_local_slice_index_equality() {
    let idx1 = LocalSliceIndex::new(100);
    let idx2 = LocalSliceIndex::new(100);
    let idx3 = LocalSliceIndex::new(101);

    assert_eq!(idx1, idx2);
    assert_ne!(idx1, idx3);
}

#[test]
fn test_local_slice_index_clone() {
    let idx1 = LocalSliceIndex::new(42);
    let idx2 = idx1;
    assert_eq!(idx1, idx2);
}

#[test]
fn test_local_slice_index_debug() {
    let idx = LocalSliceIndex::new(42);
    let debug_str = format!("{:?}", idx);
    assert!(!debug_str.is_empty());
}

// ============================================================================
// GlobalSliceIndex Tests
// ============================================================================

#[test]
fn test_global_slice_index_new() {
    let idx = GlobalSliceIndex::new(100);
    assert_eq!(idx.as_usize(), 100);
}

#[test]
fn test_global_slice_index_zero() {
    let idx = GlobalSliceIndex::new(0);
    assert_eq!(idx.as_usize(), 0);
}

#[test]
fn test_global_slice_index_large_value() {
    let idx = GlobalSliceIndex::new(10_000_000);
    assert_eq!(idx.as_usize(), 10_000_000);
}

#[test]
fn test_global_slice_index_equality() {
    let idx1 = GlobalSliceIndex::new(200);
    let idx2 = GlobalSliceIndex::new(200);
    let idx3 = GlobalSliceIndex::new(201);

    assert_eq!(idx1, idx2);
    assert_ne!(idx1, idx3);
}

#[test]
fn test_global_slice_index_clone() {
    let idx1 = GlobalSliceIndex::new(99);
    let idx2 = idx1;
    assert_eq!(idx1, idx2);
}

#[test]
fn test_global_slice_index_ord() {
    let idx1 = GlobalSliceIndex::new(100);
    let idx2 = GlobalSliceIndex::new(200);
    assert!(idx1 < idx2);
    assert!(idx2 > idx1);
}

// ============================================================================
// Crc32Value Tests
// ============================================================================

#[test]
fn test_crc32_value_new() {
    let crc = Crc32Value::new(0xDEADBEEF);
    assert_eq!(crc.as_u32(), 0xDEADBEEF);
}

#[test]
fn test_crc32_value_zero() {
    let crc = Crc32Value::new(0);
    assert_eq!(crc.as_u32(), 0);
}

#[test]
fn test_crc32_value_max() {
    let crc = Crc32Value::new(0xFFFFFFFF);
    assert_eq!(crc.as_u32(), 0xFFFFFFFF);
}

#[test]
fn test_crc32_value_equality() {
    let crc1 = Crc32Value::new(0x12345678);
    let crc2 = Crc32Value::new(0x12345678);
    let crc3 = Crc32Value::new(0x12345679);

    assert_eq!(crc1, crc2);
    assert_ne!(crc1, crc3);
}

#[test]
fn test_crc32_equality() {
    let crc1 = Crc32Value::new(0xDEADBEEF);
    let crc2 = crc1;
    let crc3 = Crc32Value::new(0xCAFEBABE);

    assert_eq!(crc1, crc2);
    assert_ne!(crc1, crc3);
}

// ============================================================================
// Type Collection Tests
// ============================================================================

#[test]
fn test_multiple_ids_in_collection() {
    use std::collections::HashMap;

    let mut files = HashMap::new();

    for i in 0..10 {
        let file_id = FileId::new([i as u8; 16]);
        let md5 = Md5Hash::new([(i + 1) as u8; 16]);
        files.insert(file_id, md5);
    }

    assert_eq!(files.len(), 10);

    let file_id_0 = FileId::new([0u8; 16]);
    assert!(files.contains_key(&file_id_0));
}

#[test]
fn test_mixed_slice_indices() {
    let local_indices: Vec<LocalSliceIndex> = (0..100).map(LocalSliceIndex::new).collect();
    let global_indices: Vec<GlobalSliceIndex> = (0..100).map(GlobalSliceIndex::new).collect();

    assert_eq!(local_indices.len(), 100);
    assert_eq!(global_indices.len(), 100);
}

#[test]
fn test_crc32_collection() {
    let crcs: Vec<Crc32Value> = (0..=10).map(|i| Crc32Value::new(i * 0x11111111)).collect();

    assert_eq!(crcs.len(), 11);
    assert_eq!(crcs[0].as_u32(), 0);
    assert_eq!(crcs[1].as_u32(), 0x11111111);
}

// ============================================================================
// Domain Type Boundary Tests
// ============================================================================

#[test]
fn test_fileid_with_boundary_values() {
    let all_zeros = FileId::new([0u8; 16]);
    let all_ones = FileId::new([255u8; 16]);
    let mixed = FileId::new([128u8; 16]);

    assert_ne!(all_zeros, all_ones);
    assert_ne!(all_zeros, mixed);
    assert_ne!(all_ones, mixed);
}

#[test]
fn test_slice_index_ordering() {
    let mut indices: Vec<GlobalSliceIndex> = vec![
        GlobalSliceIndex::new(50),
        GlobalSliceIndex::new(10),
        GlobalSliceIndex::new(100),
        GlobalSliceIndex::new(1),
    ];

    indices.sort();

    assert_eq!(indices[0].as_usize(), 1);
    assert_eq!(indices[1].as_usize(), 10);
    assert_eq!(indices[2].as_usize(), 50);
    assert_eq!(indices[3].as_usize(), 100);
}

#[test]
fn test_crc32_operations() {
    let crc1 = Crc32Value::new(0xDEADBEEF);
    let crc2 = crc1;

    let _crcs = [crc1, crc2];
}

#[test]
fn test_recovery_set_id_immutability() {
    let id = RecoverySetId::new([42u8; 16]);
    let id_copy = id;

    assert_eq!(id.as_bytes(), id_copy.as_bytes());

    // Create a different ID
    let id2 = RecoverySetId::new([43u8; 16]);
    assert_ne!(id.as_bytes(), id2.as_bytes());
}
