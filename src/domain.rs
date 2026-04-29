//! Core domain types for PAR2 operations
//!
//! This module contains type-safe wrappers for PAR2 identifiers, hashes, and indices.
//! These newtypes prevent common mistakes by making it impossible to mix different
//! kinds of identifiers at compile time.
//!
//! ## Type Safety Benefits
//!
//! - **FileId, RecoverySetId, Md5Hash**: Prevents mixing 3 different [u8; 16] identifiers
//! - **Crc32Value**: Prevents mixing CRC checksums with sizes/counts/other u32 values
//! - **GlobalSliceIndex, LocalSliceIndex**: Prevents off-by-one errors in multi-file repair
//!
//! These types are intentionally kept in a separate module to avoid circular dependencies
//! and make them easily reusable across the codebase.

/// Type-safe wrapper for PAR2 file identifiers (16-byte MD5)
/// Prevents accidentally mixing file IDs with other 16-byte values like hashes or set IDs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId([u8; 16]);

impl FileId {
    pub const fn new(bytes: [u8; 16]) -> Self {
        FileId(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl From<[u8; 16]> for FileId {
    fn from(bytes: [u8; 16]) -> Self {
        FileId::new(bytes)
    }
}

impl AsRef<[u8; 16]> for FileId {
    fn as_ref(&self) -> &[u8; 16] {
        &self.0
    }
}

impl PartialEq<[u8; 16]> for FileId {
    fn eq(&self, other: &[u8; 16]) -> bool {
        &self.0 == other
    }
}

impl PartialEq<FileId> for [u8; 16] {
    fn eq(&self, other: &FileId) -> bool {
        self == &other.0
    }
}

/// Type-safe wrapper for global slice indices (across all files in recovery set)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlobalSliceIndex(usize);

impl GlobalSliceIndex {
    pub const fn new(index: usize) -> Self {
        GlobalSliceIndex(index)
    }

    pub const fn as_usize(&self) -> usize {
        self.0
    }
}

impl From<usize> for GlobalSliceIndex {
    fn from(index: usize) -> Self {
        GlobalSliceIndex::new(index)
    }
}

impl std::ops::Add<usize> for GlobalSliceIndex {
    type Output = GlobalSliceIndex;

    fn add(self, rhs: usize) -> GlobalSliceIndex {
        GlobalSliceIndex(self.0 + rhs)
    }
}

impl std::ops::Sub for GlobalSliceIndex {
    type Output = usize;

    fn sub(self, rhs: GlobalSliceIndex) -> usize {
        self.0 - rhs.0
    }
}

impl std::fmt::Display for GlobalSliceIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type-safe wrapper for local slice indices (within a single file)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalSliceIndex(usize);

impl LocalSliceIndex {
    pub const fn new(index: usize) -> Self {
        LocalSliceIndex(index)
    }

    pub const fn as_usize(&self) -> usize {
        self.0
    }

    /// Convert to global index by adding file's global offset
    pub const fn to_global(&self, offset: GlobalSliceIndex) -> GlobalSliceIndex {
        GlobalSliceIndex(offset.0 + self.0)
    }
}

impl From<usize> for LocalSliceIndex {
    fn from(index: usize) -> Self {
        LocalSliceIndex::new(index)
    }
}

impl std::fmt::Display for LocalSliceIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type-safe wrapper for recovery set identifiers (16-byte hash)
/// Distinct from FileId and Md5Hash to prevent mixing different ID types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecoverySetId([u8; 16]);

impl RecoverySetId {
    pub const fn new(bytes: [u8; 16]) -> Self {
        RecoverySetId(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl From<[u8; 16]> for RecoverySetId {
    fn from(bytes: [u8; 16]) -> Self {
        RecoverySetId::new(bytes)
    }
}

impl AsRef<[u8; 16]> for RecoverySetId {
    fn as_ref(&self) -> &[u8; 16] {
        &self.0
    }
}

impl PartialEq<[u8; 16]> for RecoverySetId {
    fn eq(&self, other: &[u8; 16]) -> bool {
        &self.0 == other
    }
}

impl PartialEq<RecoverySetId> for [u8; 16] {
    fn eq(&self, other: &RecoverySetId) -> bool {
        self == &other.0
    }
}

/// Type-safe wrapper for MD5 hash values
/// Distinct from FileId to prevent confusion between different hash purposes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Md5Hash([u8; 16]);

impl Md5Hash {
    pub const fn new(bytes: [u8; 16]) -> Self {
        Md5Hash(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    #[allow(clippy::len_without_is_empty)]
    pub const fn len(&self) -> usize {
        16
    }
}

impl From<[u8; 16]> for Md5Hash {
    fn from(bytes: [u8; 16]) -> Self {
        Md5Hash::new(bytes)
    }
}

impl AsRef<[u8; 16]> for Md5Hash {
    fn as_ref(&self) -> &[u8; 16] {
        &self.0
    }
}

impl PartialEq<[u8; 16]> for Md5Hash {
    fn eq(&self, other: &[u8; 16]) -> bool {
        &self.0 == other
    }
}

impl PartialEq<Md5Hash> for [u8; 16] {
    fn eq(&self, other: &Md5Hash) -> bool {
        self == &other.0
    }
}

/// Type-safe wrapper for CRC32 checksum values
/// Prevents mixing CRC values with other u32 values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Crc32Value(u32);

impl Crc32Value {
    pub const fn new(value: u32) -> Self {
        Crc32Value(value)
    }

    pub const fn as_u32(&self) -> u32 {
        self.0
    }

    pub const fn to_le_bytes(&self) -> [u8; 4] {
        self.0.to_le_bytes()
    }
}

impl From<u32> for Crc32Value {
    fn from(value: u32) -> Self {
        Crc32Value::new(value)
    }
}

impl PartialEq<u32> for Crc32Value {
    fn eq(&self, other: &u32) -> bool {
        self.0 == *other
    }
}

impl PartialEq<Crc32Value> for u32 {
    fn eq(&self, other: &Crc32Value) -> bool {
        *self == other.0
    }
}

impl std::fmt::Display for Crc32Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

/// Type-safe wrapper for PAR2 block size (bytes)
/// Prevents mixing block sizes with other u64 values
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockSize(u64);

impl BlockSize {
    pub const fn new(bytes: u64) -> Self {
        BlockSize(bytes)
    }

    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    pub const fn as_usize(&self) -> usize {
        self.0 as usize
    }
}

impl From<u64> for BlockSize {
    fn from(bytes: u64) -> Self {
        BlockSize::new(bytes)
    }
}

impl std::ops::Rem<BlockSize> for u64 {
    type Output = u64;

    fn rem(self, rhs: BlockSize) -> u64 {
        self % rhs.0
    }
}

impl std::ops::Sub<u64> for BlockSize {
    type Output = u64;

    fn sub(self, rhs: u64) -> u64 {
        self.0 - rhs
    }
}

impl PartialEq<u64> for BlockSize {
    fn eq(&self, other: &u64) -> bool {
        self.0 == *other
    }
}

impl PartialOrd<u64> for BlockSize {
    fn partial_cmp(&self, other: &u64) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}

impl std::fmt::Display for BlockSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type-safe wrapper for processing chunk size (bytes)
/// Prevents mixing chunk sizes with block sizes
/// Chunk size is the memory-constrained processing unit size,
/// while block size is the PAR2 format block size
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkSize(usize);

impl ChunkSize {
    pub const fn new(bytes: usize) -> Self {
        ChunkSize(bytes)
    }

    pub const fn as_usize(&self) -> usize {
        self.0
    }
}

impl From<usize> for ChunkSize {
    fn from(bytes: usize) -> Self {
        ChunkSize::new(bytes)
    }
}

impl PartialEq<usize> for ChunkSize {
    fn eq(&self, other: &usize) -> bool {
        self.0 == *other
    }
}

impl PartialOrd<usize> for ChunkSize {
    fn partial_cmp(&self, other: &usize) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}

impl std::fmt::Display for ChunkSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Newtype for target source block count (par2cmdline -b option)
/// This is the TARGET number of source blocks to create.
/// Used to calculate block_size if block_size is not explicitly specified.
/// Reference: par2cmdline-turbo/src/commandline.h blockcount variable
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceBlockCount(u32);

impl SourceBlockCount {
    pub const fn new(count: u32) -> Self {
        SourceBlockCount(count)
    }

    pub const fn as_u32(&self) -> u32 {
        self.0
    }

    pub const fn as_usize(&self) -> usize {
        self.0 as usize
    }

    pub const fn as_u64(&self) -> u64 {
        self.0 as u64
    }
}

impl From<u32> for SourceBlockCount {
    fn from(count: u32) -> Self {
        SourceBlockCount::new(count)
    }
}

/// Type-safe wrapper for block count
/// Prevents mixing block counts with other u32 values
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockCount(u32);

impl BlockCount {
    pub const fn new(count: u32) -> Self {
        BlockCount(count)
    }

    pub const fn as_u32(&self) -> u32 {
        self.0
    }

    pub const fn as_usize(&self) -> usize {
        self.0 as usize
    }
}

impl From<u32> for BlockCount {
    fn from(count: u32) -> Self {
        BlockCount::new(count)
    }
}

impl std::ops::Add for BlockCount {
    type Output = BlockCount;

    fn add(self, rhs: BlockCount) -> BlockCount {
        BlockCount(self.0 + rhs.0)
    }
}

impl std::ops::Sub for BlockCount {
    type Output = BlockCount;

    fn sub(self, rhs: BlockCount) -> BlockCount {
        BlockCount(self.0 - rhs.0)
    }
}

impl std::ops::Sub<usize> for BlockCount {
    type Output = usize;

    fn sub(self, rhs: usize) -> usize {
        self.0 as usize - rhs
    }
}

impl std::ops::AddAssign for BlockCount {
    fn add_assign(&mut self, rhs: BlockCount) {
        self.0 += rhs.0;
    }
}

impl std::iter::Sum for BlockCount {
    fn sum<I: Iterator<Item = BlockCount>>(iter: I) -> BlockCount {
        BlockCount(iter.map(|b| b.0).sum())
    }
}

impl PartialEq<u32> for BlockCount {
    fn eq(&self, other: &u32) -> bool {
        self.0 == *other
    }
}

impl PartialOrd<u32> for BlockCount {
    fn partial_cmp(&self, other: &u32) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}

impl std::fmt::Display for BlockCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type-safe wrapper for file size in bytes
/// Prevents mixing file sizes with block offsets or other u64 values
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileSize(u64);

impl FileSize {
    pub const fn new(bytes: u64) -> Self {
        FileSize(bytes)
    }

    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    pub const fn as_usize(&self) -> usize {
        self.0 as usize
    }
}

impl From<u64> for FileSize {
    fn from(bytes: u64) -> Self {
        FileSize::new(bytes)
    }
}

impl std::ops::Rem<BlockSize> for FileSize {
    type Output = u64;

    fn rem(self, rhs: BlockSize) -> u64 {
        self.0 % rhs.as_u64()
    }
}

impl std::iter::Sum for FileSize {
    fn sum<I: Iterator<Item = FileSize>>(iter: I) -> FileSize {
        FileSize(iter.map(|s| s.0).sum())
    }
}

impl PartialEq<u64> for FileSize {
    fn eq(&self, other: &u64) -> bool {
        self.0 == *other
    }
}

impl PartialOrd<u64> for FileSize {
    fn partial_cmp(&self, other: &u64) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}

impl std::fmt::Display for FileSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn hash_newtypes_expose_their_underlying_bytes() {
        let bytes = [0x5Au8; 16];

        let file_id = FileId::new(bytes);
        let recovery_set_id = RecoverySetId::from(bytes);
        let md5 = Md5Hash::from(bytes);

        assert_eq!(file_id.as_bytes(), &bytes);
        assert_eq!(file_id.as_ref(), &bytes);
        assert_eq!(file_id, bytes);
        assert_eq!(bytes, file_id);

        assert_eq!(recovery_set_id.as_bytes(), &bytes);
        assert_eq!(recovery_set_id.as_ref(), &bytes);
        assert_eq!(recovery_set_id, bytes);
        assert_eq!(bytes, recovery_set_id);

        assert_eq!(md5.as_bytes(), &bytes);
        assert_eq!(md5.as_ref(), &bytes);
        assert_eq!(md5.len(), 16);
        assert_eq!(md5, bytes);
        assert_eq!(bytes, md5);
    }

    #[test]
    fn index_and_count_newtypes_support_expected_arithmetic() {
        let global = GlobalSliceIndex::new(11);
        let local = LocalSliceIndex::new(7);
        let mut total_blocks = BlockCount::new(5);

        assert_eq!((global + 4).as_usize(), 15);
        assert_eq!(global - GlobalSliceIndex::new(3), 8);
        assert_eq!(local.to_global(GlobalSliceIndex::new(20)).as_usize(), 27);
        assert_eq!(local.as_usize(), 7);
        assert_eq!(format!("{global}"), "11");
        assert_eq!(format!("{local}"), "7");

        assert_eq!(SourceBlockCount::new(9).as_u32(), 9);
        assert_eq!(SourceBlockCount::new(9).as_usize(), 9);
        assert_eq!(SourceBlockCount::new(9).as_u64(), 9);

        assert_eq!((BlockCount::new(2) + BlockCount::new(3)).as_u32(), 5);
        assert_eq!((BlockCount::new(9) - BlockCount::new(4)).as_u32(), 5);
        assert_eq!(BlockCount::new(9) - 4usize, 5);
        total_blocks += BlockCount::new(6);
        assert_eq!(total_blocks.as_usize(), 11);
        assert_eq!(
            [BlockCount::new(1), BlockCount::new(2), BlockCount::new(3)]
                .into_iter()
                .sum::<BlockCount>()
                .as_u32(),
            6
        );
        assert_eq!(format!("{total_blocks}"), "11");
    }

    #[test]
    fn numeric_wrapper_types_preserve_conversion_and_formatting_behavior() {
        let crc = Crc32Value::new(0x1234_ABCD);
        let block_size = BlockSize::new(4096);
        let chunk_size = ChunkSize::new(2048);
        let file_size = FileSize::new(10_000);

        assert_eq!(crc.as_u32(), 0x1234_ABCD);
        assert_eq!(crc.to_le_bytes(), 0x1234_ABCDu32.to_le_bytes());
        assert_eq!(crc, 0x1234_ABCD);
        assert_eq!(0x1234_ABCD, crc);
        assert_eq!(format!("{crc}"), "1234abcd");

        assert_eq!(block_size.as_u64(), 4096);
        assert_eq!(block_size.as_usize(), 4096);
        assert_eq!(8193u64 % block_size, 1);
        assert_eq!(block_size - 96, 4000);
        assert_eq!(block_size, 4096);
        assert!(block_size > 1024);
        assert_eq!(format!("{block_size}"), "4096");

        assert_eq!(chunk_size.as_usize(), 2048);
        assert_eq!(chunk_size, 2048);
        assert!(chunk_size > 1024);
        assert_eq!(format!("{chunk_size}"), "2048");

        assert_eq!(file_size.as_u64(), 10_000);
        assert_eq!(file_size.as_usize(), 10_000);
        assert_eq!(file_size % block_size, 1808);
        assert_eq!(
            [FileSize::new(3), FileSize::new(4), FileSize::new(5)]
                .into_iter()
                .sum::<FileSize>()
                .as_u64(),
            12
        );
        assert_eq!(file_size, 10_000);
        assert!(file_size > 9_000);
        assert_eq!(format!("{file_size}"), "10000");
    }

    proptest! {
        #[test]
        fn local_indices_round_trip_through_global_offsets(local in 0usize..1_000_000, offset in 0usize..1_000_000) {
            let local_index = LocalSliceIndex::new(local);
            let global_index = local_index.to_global(GlobalSliceIndex::new(offset));

            prop_assert_eq!(global_index.as_usize(), local + offset);
            prop_assert_eq!(global_index - GlobalSliceIndex::new(offset), local);
        }
    }
}
