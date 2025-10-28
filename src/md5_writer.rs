//! MD5Writer - Compute MD5 hash while writing data
//!
//! This module provides a writer wrapper that computes MD5 hash incrementally
//! as data is written, eliminating the need to re-read files for verification.
//! The MD5 computation happens inline with writes and does not block I/O.

use md5::{Digest, Md5};
use std::io::{self, Write};

/// A writer that computes MD5 hash of all data written through it.
///
/// The hash computation happens inline during write operations, adding minimal
/// overhead while eliminating the need to re-read files for verification.
///
/// # Example
///
/// ```no_run
/// use std::fs::File;
/// use std::io::{BufWriter, Write};
/// # use par2rs::md5_writer::Md5Writer;
///
/// let file = File::create("output.dat")?;
/// let mut writer = Md5Writer::new(BufWriter::new(file));
///
/// writer.write_all(b"some data")?;
/// writer.write_all(b"more data")?;
///
/// let (inner, hash) = writer.finalize();
/// println!("MD5: {:02x?}", hash);
/// # Ok::<(), std::io::Error>(())
/// ```
pub struct Md5Writer<W: Write> {
    inner: W,
    hasher: Md5,
}

impl<W: Write> Md5Writer<W> {
    /// Create a new Md5Writer wrapping the given writer
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: crate::checksum::new_md5_hasher(),
        }
    }

    /// Finalize the hash and return the inner writer and computed MD5 hash
    ///
    /// This consumes the Md5Writer and returns the wrapped writer along with
    /// the computed MD5 hash as a 16-byte array.
    pub fn finalize(self) -> (W, [u8; 16]) {
        let hash = self.hasher.finalize();
        (self.inner, hash.into())
    }

    /// Get a reference to the inner writer
    pub fn get_ref(&self) -> &W {
        &self.inner
    }

    /// Get a mutable reference to the inner writer
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.inner
    }
}

impl<W: Write> Write for Md5Writer<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Write to inner first to ensure we only hash what was actually written
        let n = self.inner.write(buf)?;
        // Update hash with what was written
        // This is inline and doesn't block - MD5 is ~500MB/s even on single thread
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        // Optimized path for write_all to avoid double hashing
        self.inner.write_all(buf)?;
        self.hasher.update(buf);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md5_writer_computes_correct_hash() {
        let mut writer = Md5Writer::new(Vec::new());
        writer.write_all(b"hello world").unwrap();

        let (data, hash) = writer.finalize();

        // Known MD5 of "hello world"
        let expected = [
            0x5e, 0xb6, 0x3b, 0xbb, 0xe0, 0x1e, 0xee, 0xd0, 0x93, 0xcb, 0x22, 0xbb, 0x8f, 0x5a,
            0xcd, 0xc3,
        ];
        assert_eq!(hash, expected);
        assert_eq!(data, b"hello world");
    }

    #[test]
    fn test_md5_writer_multiple_writes() {
        let mut writer = Md5Writer::new(Vec::new());
        writer.write_all(b"hello").unwrap();
        writer.write_all(b" ").unwrap();
        writer.write_all(b"world").unwrap();

        let (data, hash) = writer.finalize();

        // Same hash as single write
        let expected = [
            0x5e, 0xb6, 0x3b, 0xbb, 0xe0, 0x1e, 0xee, 0xd0, 0x93, 0xcb, 0x22, 0xbb, 0x8f, 0x5a,
            0xcd, 0xc3,
        ];
        assert_eq!(hash, expected);
        assert_eq!(data, b"hello world");
    }

    #[test]
    fn test_md5_writer_with_partial_write() {
        // Use a fixed-size buffer that will cause partial writes
        struct PartialWriter {
            written: usize,
        }

        impl Write for PartialWriter {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                // Only write 5 bytes max
                let n = buf.len().min(5);
                self.written += n;
                Ok(n)
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut partial = PartialWriter { written: 0 };
        let mut writer = Md5Writer::new(&mut partial);

        // This will only write 5 bytes
        let n = writer.write(b"hello world").unwrap();
        assert_eq!(n, 5);

        let (_, hash) = writer.finalize();

        // Hash of "hello" only (what was actually written)
        let mut expected_hello = crate::checksum::new_md5_hasher();
        expected_hello.update(b"hello");
        let expected: [u8; 16] = expected_hello.finalize().into();
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_md5_writer_empty() {
        let writer = Md5Writer::new(Vec::new());
        let (data, hash) = writer.finalize();

        // MD5 of empty data
        let expected: [u8; 16] = crate::checksum::new_md5_hasher().finalize().into();
        assert_eq!(hash, expected);
        assert!(data.is_empty());
    }
}
