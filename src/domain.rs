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
    pub fn new(bytes: [u8; 16]) -> Self {
        FileId(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
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
    pub fn new(index: usize) -> Self {
        GlobalSliceIndex(index)
    }

    pub fn as_usize(&self) -> usize {
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
    pub fn new(index: usize) -> Self {
        LocalSliceIndex(index)
    }

    pub fn as_usize(&self) -> usize {
        self.0
    }

    /// Convert to global index by adding file's global offset
    pub fn to_global(&self, offset: GlobalSliceIndex) -> GlobalSliceIndex {
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
    pub fn new(bytes: [u8; 16]) -> Self {
        RecoverySetId(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
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
    pub fn new(bytes: [u8; 16]) -> Self {
        Md5Hash(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
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
    pub fn new(value: u32) -> Self {
        Crc32Value(value)
    }

    pub fn as_u32(&self) -> u32 {
        self.0
    }

    pub fn to_le_bytes(&self) -> [u8; 4] {
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
