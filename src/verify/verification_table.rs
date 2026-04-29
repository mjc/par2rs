//! Turbo-style verification table for verify hot paths.

use super::global_table::GlobalBlockTable;
use super::turbo_file_scan::TurboFileScanner;
use crate::domain::{Crc32Value, FileId, Md5Hash};
use crate::packets::FileDescriptionPacket;
use rustc_hash::FxHashMap as HashMap;
use smallvec::SmallVec;

pub type EntryId = usize;

#[derive(Debug, Clone)]
pub struct VerificationEntry {
    pub file_id: FileId,
    pub block_number: u32,
    pub expected_block_length: usize,
    pub is_first_block: bool,
    pub md5: Md5Hash,
    pub crc32: Crc32Value,
    pub next: Option<EntryId>,
}

#[derive(Debug, Default)]
pub struct MatchResult {
    pub matched: Option<EntryId>,
    pub exact_matches: SmallVec<[EntryId; 4]>,
}

impl MatchResult {
    pub fn is_match(&self) -> bool {
        self.matched.is_some()
    }
}

pub struct VerificationTable {
    crc_buckets: HashMap<Crc32Value, SmallVec<[EntryId; 4]>>,
    entries: Vec<VerificationEntry>,
}

impl VerificationTable {
    pub fn from_block_table(
        block_table: &GlobalBlockTable,
        file_descriptions: &[&FileDescriptionPacket],
    ) -> Self {
        let mut entries = Vec::new();
        let mut crc_buckets: HashMap<Crc32Value, SmallVec<[EntryId; 4]>> = HashMap::default();

        for description in file_descriptions {
            let file_blocks = block_table.get_file_blocks(description.file_id);
            let total_blocks = file_blocks.len();

            for (index, block) in file_blocks.into_iter().enumerate() {
                let expected_block_length = if index + 1 == total_blocks {
                    let full_blocks_len = (index as u64) * block_table.block_size();
                    description
                        .file_length
                        .saturating_sub(full_blocks_len)
                        .min(block_table.block_size()) as usize
                } else {
                    block_table.block_size() as usize
                };

                let entry_id = entries.len();
                entries.push(VerificationEntry {
                    file_id: description.file_id,
                    block_number: block.position.block_number,
                    expected_block_length,
                    is_first_block: block.position.is_first_block,
                    md5: block.checksums.md5_hash,
                    crc32: block.checksums.crc32,
                    next: None,
                });
                crc_buckets
                    .entry(block.checksums.crc32)
                    .or_default()
                    .push(entry_id);
            }
        }

        let mut next_by_file_and_block: HashMap<(FileId, u32), EntryId> = HashMap::default();
        for entry_id in (0..entries.len()).rev() {
            let entry = &entries[entry_id];
            next_by_file_and_block.insert((entry.file_id, entry.block_number), entry_id);
        }

        for entry_id in 0..entries.len() {
            let file_id = entries[entry_id].file_id;
            let block_number = entries[entry_id].block_number;
            entries[entry_id].next = next_by_file_and_block
                .get(&(file_id, block_number + 1))
                .copied();
        }

        Self {
            crc_buckets,
            entries,
        }
    }

    pub fn entry(&self, entry_id: EntryId) -> &VerificationEntry {
        &self.entries[entry_id]
    }

    pub fn find_match(
        &self,
        next_expected: Option<EntryId>,
        preferred_file: Option<FileId>,
        scanner: &mut TurboFileScanner,
    ) -> MatchResult {
        if let Some(expected_id) = next_expected {
            let entry = &self.entries[expected_id];
            if entry.expected_block_length == 0 {
                return MatchResult::default();
            }
            if entry.next.is_none() {
                let checksum = scanner.short_checksum(entry.expected_block_length);
                if checksum == entry.crc32 {
                    let hash = scanner.short_hash(entry.expected_block_length);
                    if hash == entry.md5 {
                        let exact_matches = self.collect_exact_matches(
                            checksum,
                            hash,
                            Some(entry.expected_block_length),
                        );
                        return MatchResult {
                            matched: Some(expected_id),
                            exact_matches,
                        };
                    }
                }
            } else if scanner.checksum() == entry.crc32 {
                let hash = scanner.current_md5();
                if hash == entry.md5 {
                    let exact_matches = self.collect_exact_matches(scanner.checksum(), hash, None);
                    return MatchResult {
                        matched: Some(expected_id),
                        exact_matches,
                    };
                }
            }
        }

        let Some(bucket) = self.crc_buckets.get(&scanner.checksum()) else {
            return MatchResult::default();
        };

        let hash = scanner.current_md5();
        let mut exact_matches = SmallVec::<[EntryId; 4]>::new();
        for entry_id in bucket.iter().copied() {
            let entry = &self.entries[entry_id];
            if entry.expected_block_length == 0 {
                continue;
            }
            if entry.md5 != hash {
                continue;
            }
            if scanner.short_block() && scanner.block_length() != entry.expected_block_length {
                continue;
            }
            exact_matches.push(entry_id);
        }

        if exact_matches.is_empty() {
            return MatchResult::default();
        }

        let mut matched = None;
        if let Some(file_id) = preferred_file {
            matched = exact_matches
                .iter()
                .copied()
                .find(|entry_id| self.entries[*entry_id].file_id == file_id);
        }
        if matched.is_none() && scanner.offset() == 0 {
            matched = exact_matches
                .iter()
                .copied()
                .find(|entry_id| self.entries[*entry_id].is_first_block);
        }
        if matched.is_none() {
            matched = exact_matches.first().copied();
        }

        MatchResult {
            matched,
            exact_matches,
        }
    }

    fn collect_exact_matches(
        &self,
        checksum: Crc32Value,
        hash: Md5Hash,
        exact_length: Option<usize>,
    ) -> SmallVec<[EntryId; 4]> {
        let mut matches = SmallVec::<[EntryId; 4]>::new();
        let Some(bucket) = self.crc_buckets.get(&checksum) else {
            return matches;
        };

        for entry_id in bucket.iter().copied() {
            let entry = &self.entries[entry_id];
            if entry.md5 != hash {
                continue;
            }
            if exact_length.is_some_and(|len| entry.expected_block_length != len) {
                continue;
            }
            matches.push(entry_id);
        }

        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum::{compute_crc32, compute_md5_only};
    use crate::domain::RecoverySetId;
    use crate::verify::global_table::GlobalBlockTableBuilder;
    use tempfile::NamedTempFile;

    fn test_desc(
        file_id: FileId,
        len: u64,
        name: &str,
        md5: Md5Hash,
        md5_16k: Md5Hash,
    ) -> FileDescriptionPacket {
        FileDescriptionPacket {
            length: 120 + name.len() as u64,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new([1; 16]),
            packet_type: *b"PAR 2.0\0FileDesc",
            file_id,
            md5_hash: md5,
            md5_16k,
            file_length: len,
            file_name: name.as_bytes().to_vec(),
        }
    }

    #[test]
    fn prefers_expected_next_entry() {
        let data = vec![0x42; 16];
        let md5 = compute_md5_only(&data);
        let crc32 = compute_crc32(&data);
        let file_id = FileId::new([1; 16]);

        let mut builder = GlobalBlockTableBuilder::new(16);
        builder.add_file_blocks(file_id, &[(md5, crc32)]);
        let block_table = builder.build();
        let desc = test_desc(file_id, 16, "a.bin", md5, md5);
        let table = VerificationTable::from_block_table(&block_table, &[&desc]);

        let mut temp = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut temp, &data).unwrap();
        let mut scanner = TurboFileScanner::open(temp.path(), 16).unwrap();
        scanner.start().unwrap();

        let result = table.find_match(Some(0), Some(file_id), &mut scanner);
        assert_eq!(result.matched, Some(0));
    }

    #[test]
    fn prefers_requested_file_across_duplicate_content() {
        let data = vec![0x11; 16];
        let md5 = compute_md5_only(&data);
        let crc32 = compute_crc32(&data);
        let file_a = FileId::new([1; 16]);
        let file_b = FileId::new([2; 16]);

        let mut builder = GlobalBlockTableBuilder::new(16);
        builder.add_file_blocks(file_a, &[(md5, crc32)]);
        builder.add_file_blocks(file_b, &[(md5, crc32)]);
        let block_table = builder.build();
        let desc_a = test_desc(file_a, 16, "a.bin", md5, md5);
        let desc_b = test_desc(file_b, 16, "b.bin", md5, md5);
        let table = VerificationTable::from_block_table(&block_table, &[&desc_a, &desc_b]);

        let mut temp = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut temp, &data).unwrap();
        let mut scanner = TurboFileScanner::open(temp.path(), 16).unwrap();
        scanner.start().unwrap();

        let result = table.find_match(None, Some(file_b), &mut scanner);
        let matched = result.matched.unwrap();
        assert_eq!(table.entry(matched).file_id, file_b);
        assert_eq!(result.exact_matches.len(), 2);
    }

    #[test]
    fn ignores_zero_length_entries_from_malformed_metadata() {
        let data = vec![0x22; 16];
        let md5 = compute_md5_only(&data);
        let crc32 = compute_crc32(&data);
        let file_id = FileId::new([3; 16]);

        let mut builder = GlobalBlockTableBuilder::new(16);
        builder.add_file_blocks(file_id, &[(md5, crc32), (md5, crc32)]);
        let block_table = builder.build();

        // Malformed metadata: file length covers only the first block, so the
        // second entry's expected length is driven to zero.
        let desc = test_desc(file_id, 16, "bad.bin", md5, md5);
        let table = VerificationTable::from_block_table(&block_table, &[&desc]);
        assert_eq!(table.entry(1).expected_block_length, 0);

        let mut temp = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut temp, &data).unwrap();
        let mut scanner = TurboFileScanner::open(temp.path(), 16).unwrap();
        scanner.start().unwrap();

        let result = table.find_match(Some(1), Some(file_id), &mut scanner);
        assert!(result.matched.is_none());
        assert!(result.exact_matches.is_empty());
    }
}
