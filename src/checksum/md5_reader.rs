//! MD5Reader - Compute MD5 hash while reading data
//!
//! This module provides a reader wrapper that computes MD5 hash incrementally
//! as data is read, eliminating the need to make separate passes for hashing.

use md5::{Digest, Md5};
use std::io::{self, Read};

/// A reader that computes MD5 hash of all data read through it.
///
/// The hash computation happens inline during read operations, adding minimal
/// overhead while eliminating the need for separate hashing passes.
pub struct Md5Reader<R: Read> {
    inner: R,
    hasher: Md5,
}

impl<R: Read> Md5Reader<R> {
    /// Create a new Md5Reader wrapping the given reader
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: crate::checksum::new_md5_hasher(),
        }
    }

    /// Finalize the hash and return the inner reader and computed MD5 hash
    pub fn finalize(self) -> (R, [u8; 16]) {
        let hash = self.hasher.finalize();
        (self.inner, hash.into())
    }

    /// Get a reference to the inner reader
    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    /// Get a mutable reference to the inner reader
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }
}

impl<R: Read> Read for Md5Reader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_md5_reader_computes_correct_hash() {
        let data = b"hello world";
        let cursor = Cursor::new(data);
        let mut reader = Md5Reader::new(cursor);

        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();

        let (_, hash) = reader.finalize();

        // Known MD5 of "hello world"
        let expected = [
            0x5e, 0xb6, 0x3b, 0xbb, 0xe0, 0x1e, 0xee, 0xd0, 0x93, 0xcb, 0x22, 0xbb, 0x8f, 0x5a,
            0xcd, 0xc3,
        ];
        assert_eq!(hash, expected);
        assert_eq!(buf, data);
    }

    #[test]
    fn test_md5_reader_multiple_reads() {
        let data = b"hello world";
        let cursor = Cursor::new(data);
        let mut reader = Md5Reader::new(cursor);

        let mut buf1 = [0u8; 5];
        let mut buf2 = [0u8; 6];

        reader.read_exact(&mut buf1).unwrap();
        reader.read_exact(&mut buf2).unwrap();

        let (_, hash) = reader.finalize();

        // Same hash as single read
        let expected = [
            0x5e, 0xb6, 0x3b, 0xbb, 0xe0, 0x1e, 0xee, 0xd0, 0x93, 0xcb, 0x22, 0xbb, 0x8f, 0x5a,
            0xcd, 0xc3,
        ];
        assert_eq!(hash, expected);
        assert_eq!(&buf1, b"hello");
        assert_eq!(&buf2, b" world");
    }

    #[test]
    fn test_md5_reader_empty() {
        let cursor = Cursor::new(&[]);
        let reader = Md5Reader::new(cursor);

        let (_, hash) = reader.finalize();

        // MD5 of empty data
        let expected: [u8; 16] = crate::checksum::new_md5_hasher().finalize().into();
        assert_eq!(hash, expected);
    }
}
