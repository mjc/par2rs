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
