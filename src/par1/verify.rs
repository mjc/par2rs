use super::parser::{parse_par1_bytes, Par1ParseError};
use super::types::{Par1FileEntry, PAR1_STATUS_IN_PARITY_VOLUME};
use crate::checksum::{calculate_file_md5, calculate_file_md5_16k};
use crate::domain::FileId;
use crate::verify::{
    BlockVerificationResult, FileStatus, FileVerificationResult, VerificationResults,
};
use rustc_hash::FxHashMap as HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum Par1VerifyError {
    Io(std::io::Error),
    Parse(Par1ParseError),
}

impl fmt::Display for Par1VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Parse(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for Par1VerifyError {}

impl From<std::io::Error> for Par1VerifyError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<Par1ParseError> for Par1VerifyError {
    fn from(error: Par1ParseError) -> Self {
        Self::Parse(error)
    }
}

pub fn verify_par1_file(path: &Path) -> Result<VerificationResults, Par1VerifyError> {
    let par1_files = crate::par2_files::collect_par1_files(path);
    let recovery_file = par1_files.first().map(PathBuf::as_path).unwrap_or(path);
    let bytes = std::fs::read(recovery_file)?;
    let set = parse_par1_bytes(&bytes)?;
    let base_dir = recovery_file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));

    let file_results: Vec<_> = set
        .files
        .iter()
        .filter(|entry| entry.status & PAR1_STATUS_IN_PARITY_VOLUME != 0)
        .map(|entry| verify_entry(base_dir, entry))
        .collect();

    let block_results = file_results
        .iter()
        .map(|file| BlockVerificationResult {
            block_number: 0,
            file_id: file.file_id,
            is_valid: file.status == FileStatus::Present,
            expected_hash: None,
            expected_crc: None,
        })
        .collect();

    Ok(VerificationResults::from_file_results(
        file_results,
        block_results,
        0,
    ))
}

pub(crate) fn verify_entry(base_dir: &Path, entry: &Par1FileEntry) -> FileVerificationResult {
    let file_name = local_file_name(&entry.name).to_string();
    let file_path = base_dir.join(&file_name);
    let mut status = FileStatus::Missing;

    if let Ok(metadata) = std::fs::metadata(&file_path) {
        let hash_matches = metadata.len() == entry.file_size
            && calculate_file_md5_16k(&file_path).ok() == Some(entry.hash_16k)
            && calculate_file_md5(&file_path).ok() == Some(entry.hash_full);
        status = if hash_matches {
            FileStatus::Present
        } else {
            FileStatus::Corrupted
        };
    }

    let blocks_available = usize::from(status == FileStatus::Present);
    let damaged_blocks = if status == FileStatus::Present {
        Vec::new()
    } else {
        vec![0]
    };

    FileVerificationResult {
        file_name,
        file_id: FileId::new(*entry.hash_full.as_bytes()),
        status,
        blocks_available,
        total_blocks: 1,
        damaged_blocks,
        block_positions: HashMap::default(),
    }
}

pub(crate) fn local_file_name(name: &str) -> &str {
    name.rsplit(['/', '\\', ':']).next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum::compute_md5;
    use crate::domain::Md5Hash;

    fn entry_for(name: &str, data: &[u8]) -> Par1FileEntry {
        Par1FileEntry {
            status: PAR1_STATUS_IN_PARITY_VOLUME,
            file_size: data.len() as u64,
            hash_full: compute_md5(data),
            hash_16k: compute_md5(&data[..data.len().min(16 * 1024)]),
            name: name.to_string(),
        }
    }

    #[test]
    fn verifies_present_file_entry() {
        let temp = tempfile::tempdir().unwrap();
        let data = b"par1 source data";
        std::fs::write(temp.path().join("source.bin"), data).unwrap();

        let result = verify_entry(temp.path(), &entry_for("source.bin", data));

        assert_eq!(result.status, FileStatus::Present);
        assert_eq!(result.blocks_available, 1);
        assert!(result.damaged_blocks.is_empty());
    }

    #[test]
    fn verifies_missing_file_entry() {
        let temp = tempfile::tempdir().unwrap();

        let result = verify_entry(temp.path(), &entry_for("source.bin", b"missing"));

        assert_eq!(result.status, FileStatus::Missing);
        assert_eq!(result.blocks_available, 0);
        assert_eq!(result.damaged_blocks, vec![0]);
    }

    #[test]
    fn verifies_corrupted_file_entry() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("source.bin"), b"wrong").unwrap();

        let result = verify_entry(temp.path(), &entry_for("source.bin", b"expected"));

        assert_eq!(result.status, FileStatus::Corrupted);
        assert_eq!(result.blocks_available, 0);
    }

    #[test]
    fn strips_par1_paths_to_local_file_name() {
        assert_eq!(local_file_name("dir/source.bin"), "source.bin");
        assert_eq!(local_file_name("dir\\source.bin"), "source.bin");
        assert_eq!(local_file_name("C:source.bin"), "source.bin");
    }

    #[test]
    fn protected_entry_uses_full_hash_as_file_id() {
        let entry = Par1FileEntry {
            status: PAR1_STATUS_IN_PARITY_VOLUME,
            file_size: 0,
            hash_full: Md5Hash::new([7; 16]),
            hash_16k: Md5Hash::new([7; 16]),
            name: "source.bin".to_string(),
        };

        let result = verify_entry(Path::new("."), &entry);

        assert_eq!(result.file_id, FileId::new([7; 16]));
    }
}
