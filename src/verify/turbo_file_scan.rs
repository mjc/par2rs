//! Turbo-style file scanner for verify hot paths.
//!
//! This mirrors the structure of par2cmdline-turbo's `FileCheckSummer`
//! closely enough to preserve its scanning semantics while staying pure Rust.

use crate::checksum::{
    compute_block_checksums_padded, compute_crc32, compute_crc32_padded, compute_md5_only,
    finalize_md5, new_md5_hasher,
};
use crate::domain::{Crc32Value, Md5Hash};
use crate::parpar_hasher::hasher_input_dyn::HasherInputDyn;
use md5::Digest;
use md5::Md5;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use crate::checksum::rolling_crc::RollingCrcTable;

const HASH_16K_THRESHOLD: u64 = 16 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct FileHashResults {
    pub hash_16k: Md5Hash,
    pub hash_full: Md5Hash,
    pub file_size: u64,
}

struct First16kHash {
    hasher_16k: Md5,
    total_bytes_read: u64,
}

impl First16kHash {
    fn new() -> Self {
        Self {
            hasher_16k: new_md5_hasher(),
            total_bytes_read: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        if self.total_bytes_read < HASH_16K_THRESHOLD {
            let bytes_to_hash = data
                .len()
                .min((HASH_16K_THRESHOLD - self.total_bytes_read) as usize);
            self.hasher_16k.update(&data[..bytes_to_hash]);
        }
        self.total_bytes_read += data.len() as u64;
    }

    fn finalize(self) -> Md5Hash {
        finalize_md5(self.hasher_16k)
    }
}

pub struct TurboFileScanner {
    file: File,
    block_size: usize,
    file_size: u64,
    current_offset: u64,
    read_offset: u64,
    buffer: Vec<u8>,
    out_pos: usize,
    in_pos: usize,
    tail_pos: usize,
    checksum: Crc32Value,
    block_hash: Option<Md5Hash>,
    hash_16k: First16kHash,
    hasher: HasherInputDyn,
    block_hash_synced: bool,
    rolling_table: RollingCrcTable,
}

impl TurboFileScanner {
    pub fn open(path: &Path, block_size: usize) -> io::Result<Self> {
        let file = File::open(path)?;
        let file_size = file.metadata()?.len();

        Ok(Self {
            file,
            block_size,
            file_size,
            current_offset: 0,
            read_offset: 0,
            buffer: vec![0u8; block_size * 2],
            out_pos: 0,
            in_pos: block_size,
            tail_pos: 0,
            checksum: Crc32Value::new(0),
            block_hash: None,
            hash_16k: First16kHash::new(),
            hasher: HasherInputDyn::new(),
            block_hash_synced: true,
            rolling_table: RollingCrcTable::new(block_size),
        })
    }

    pub fn start(&mut self) -> io::Result<()> {
        self.current_offset = 0;
        self.read_offset = 0;
        self.out_pos = 0;
        self.in_pos = self.block_size;
        self.tail_pos = 0;
        self.checksum = Crc32Value::new(0);
        self.block_hash = None;
        self.hasher = HasherInputDyn::new();
        self.block_hash_synced = true;

        if self.file_size == 0 {
            self.buffer[..self.block_size].fill(0);
            return Ok(());
        }

        self.fill(false)?;
        self.compute_current_checksum(true);
        Ok(())
    }

    pub fn offset(&self) -> u64 {
        self.current_offset
    }

    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    pub fn checksum(&self) -> Crc32Value {
        self.checksum
    }

    pub fn block_length(&self) -> usize {
        self.block_size.min(
            self.file_size
                .saturating_sub(self.current_offset)
                .try_into()
                .unwrap_or(usize::MAX),
        )
    }

    pub fn short_block(&self) -> bool {
        self.block_length() < self.block_size
    }

    pub fn current_md5(&mut self) -> Md5Hash {
        if let Some(hash) = self.block_hash {
            return hash;
        }

        let hash = if self.short_block() {
            compute_block_checksums_padded(self.current_data(), self.block_size).0
        } else {
            compute_md5_only(self.current_window())
        };
        self.block_hash = Some(hash);
        hash
    }

    pub fn short_checksum(&self, block_length: usize) -> Crc32Value {
        if block_length == self.block_length() && self.short_block() {
            self.checksum
        } else if block_length == self.block_size {
            self.checksum
        } else {
            compute_crc32_padded(&self.current_window()[..block_length], self.block_size)
        }
    }

    pub fn short_hash(&mut self, block_length: usize) -> Md5Hash {
        if block_length == self.block_length() && self.short_block() {
            self.current_md5()
        } else if block_length == self.block_size {
            self.current_md5()
        } else {
            compute_block_checksums_padded(&self.current_window()[..block_length], self.block_size)
                .0
        }
    }

    pub fn step(&mut self) -> io::Result<()> {
        if self.current_offset >= self.file_size {
            return Ok(());
        }

        self.stop_hasher();
        self.block_hash = None;

        self.current_offset += 1;
        if self.current_offset >= self.file_size {
            self.set_eof_state();
            return Ok(());
        }

        if self.tail_pos <= self.in_pos {
            self.fill(true)?;
        }

        let byte_in = self.buffer[self.in_pos];
        let byte_out = self.buffer[self.out_pos];
        self.in_pos += 1;
        self.out_pos += 1;
        self.checksum = Crc32Value::new(self.rolling_table.slide(
            self.checksum.as_u32(),
            byte_in,
            byte_out,
        ));

        if self.out_pos == self.block_size {
            let keep = self.tail_pos.saturating_sub(self.out_pos);
            self.buffer.copy_within(self.out_pos..self.tail_pos, 0);
            self.tail_pos = keep;
            self.in_pos = self.block_size;
            self.out_pos = 0;
        }

        Ok(())
    }

    pub fn jump(&mut self, mut distance: u64) -> io::Result<()> {
        if self.current_offset >= self.file_size || distance == 0 {
            return Ok(());
        }

        if distance == 1 {
            return self.step();
        }

        if distance > self.block_size as u64 {
            distance = self.block_size as u64;
        }

        if distance != self.block_size as u64 && self.current_offset + distance < self.file_size {
            self.stop_hasher();
        }

        self.block_hash = None;
        self.current_offset += distance;
        if self.current_offset >= self.file_size {
            self.set_eof_state();
            return Ok(());
        }

        self.out_pos += distance as usize;
        let keep = self.tail_pos.saturating_sub(self.out_pos);
        if keep > 0 {
            self.buffer.copy_within(self.out_pos..self.tail_pos, 0);
        }
        self.tail_pos = keep;
        self.out_pos = 0;
        self.in_pos = self.block_size;

        self.fill(false)?;
        self.compute_current_checksum(distance == self.block_size as u64);
        Ok(())
    }

    pub fn stop_hasher(&mut self) {
        if !self.block_hash_synced {
            return;
        }
        if self.tail_pos > self.in_pos {
            self.hasher.update(&self.buffer[self.in_pos..self.tail_pos]);
        }
        self.block_hash_synced = false;
    }

    pub fn finish(self) -> FileHashResults {
        let hash_16k = self.hash_16k.finalize();
        let hash_full = Md5Hash::new(self.hasher.end());
        FileHashResults {
            hash_16k,
            hash_full: if self.file_size < HASH_16K_THRESHOLD {
                hash_16k
            } else {
                hash_full
            },
            file_size: self.file_size,
        }
    }

    fn compute_current_checksum(&mut self, do_md5: bool) {
        if self.current_offset >= self.file_size {
            self.checksum = Crc32Value::new(0);
            self.block_hash = None;
            return;
        }

        if self.block_hash_synced {
            let hasher = &mut self.hasher;
            let block_len = self.block_size.min(
                self.file_size
                    .saturating_sub(self.current_offset)
                    .try_into()
                    .unwrap_or(usize::MAX),
            );
            let zero_pad = (self.block_size - block_len) as u64;
            let window = &self.buffer[self.out_pos..self.out_pos + self.block_size];
            hasher.update(&window[..block_len]);
            let block = hasher.get_block(zero_pad);
            self.checksum = Crc32Value::new(block.crc32);
            self.block_hash = Some(Md5Hash::new(block.md5));
            return;
        }

        if do_md5 {
            let (md5, crc) = if self.short_block() {
                compute_block_checksums_padded(self.current_data(), self.block_size)
            } else {
                (
                    compute_md5_only(self.current_window()),
                    compute_crc32(self.current_window()),
                )
            };
            self.checksum = crc;
            self.block_hash = Some(md5);
        } else {
            self.checksum = if self.short_block() {
                compute_crc32_padded(self.current_data(), self.block_size)
            } else {
                compute_crc32(self.current_window())
            };
            self.block_hash = None;
        }
    }

    fn fill(&mut self, long_fill: bool) -> io::Result<()> {
        if self.read_offset >= self.file_size {
            return Ok(());
        }

        if self.tail_pos >= self.block_size && !long_fill {
            return Ok(());
        }

        let target = if self.tail_pos == 0 {
            self.block_size
        } else {
            self.block_size * 2
        };
        let want =
            ((target - self.tail_pos) as u64).min(self.file_size - self.read_offset) as usize;

        if want > 0 {
            let bytes_read = self.read_fill(self.tail_pos, want)?;
            if bytes_read > 0 {
                let end = self.tail_pos + bytes_read;
                self.hash_16k.update(&self.buffer[self.tail_pos..end]);
                if !self.block_hash_synced {
                    self.hasher.update(&self.buffer[self.tail_pos..end]);
                }
                self.read_offset += bytes_read as u64;
                self.tail_pos = end;
            }
        }

        if self.tail_pos < target {
            self.buffer[self.tail_pos..target].fill(0);
        }

        Ok(())
    }

    fn read_fill(&mut self, start: usize, want: usize) -> io::Result<usize> {
        let mut total = 0usize;
        while total < want {
            let bytes_read = self
                .file
                .read(&mut self.buffer[start + total..start + want])?;
            if bytes_read == 0 {
                break;
            }
            total += bytes_read;
        }
        Ok(total)
    }

    fn current_window(&self) -> &[u8] {
        &self.buffer[self.out_pos..self.out_pos + self.block_size]
    }

    fn current_data(&self) -> &[u8] {
        &self.current_window()[..self.block_length()]
    }

    fn set_eof_state(&mut self) {
        self.current_offset = self.file_size;
        self.tail_pos = 0;
        self.out_pos = 0;
        self.in_pos = self.block_size;
        self.buffer[..self.block_size].fill(0);
        self.checksum = Crc32Value::new(0);
        self.block_hash = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum::compute_block_checksums_padded;
    use tempfile::NamedTempFile;

    fn write_temp(data: &[u8]) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut file, data).unwrap();
        file
    }

    #[test]
    fn start_step_and_jump_follow_expected_offsets() {
        let data: Vec<u8> = (0..64u8).collect();
        let file = write_temp(&data);
        let mut scanner = TurboFileScanner::open(file.path(), 16).unwrap();

        scanner.start().unwrap();
        assert_eq!(scanner.offset(), 0);

        scanner.step().unwrap();
        assert_eq!(scanner.offset(), 1);

        scanner.jump(16).unwrap();
        assert_eq!(scanner.offset(), 17);
    }

    #[test]
    fn short_block_padding_matches_helpers() {
        let data: Vec<u8> = (0..23u8).collect();
        let file = write_temp(&data);
        let mut scanner = TurboFileScanner::open(file.path(), 32).unwrap();

        scanner.start().unwrap();
        let (expected_md5, expected_crc) = compute_block_checksums_padded(&data, 32);
        assert!(scanner.short_block());
        assert_eq!(scanner.checksum(), expected_crc);
        assert_eq!(scanner.current_md5(), expected_md5);
    }

    #[test]
    fn stop_hasher_preserves_file_hash_results() {
        let data: Vec<u8> = (0..100u8).cycle().take(4096).collect();
        let file = write_temp(&data);
        let mut scanner = TurboFileScanner::open(file.path(), 1024).unwrap();

        scanner.start().unwrap();
        scanner.step().unwrap();
        scanner.jump(17).unwrap();
        while scanner.offset() < scanner.file_size() {
            scanner.jump(1024).unwrap();
        }
        let results = scanner.finish();

        let expected =
            crate::checksum::FileCheckSummer::new(file.path().to_string_lossy().into_owned(), 1024)
                .unwrap()
                .compute_file_hashes()
                .unwrap();

        assert_eq!(results.hash_full, expected.hash_full);
        assert_eq!(results.hash_16k, expected.hash_16k);
    }
}
