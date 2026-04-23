use super::parser::{parse_par1_bytes, Par1ParseError};
use super::types::{Par1FileEntry, PAR1_STATUS_IN_PARITY_VOLUME};
use crate::checksum::{calculate_file_md5, calculate_file_md5_16k};
use crate::domain::FileId;
use crate::verify::{
    BlockVerificationResult, FileStatus, FileVerificationResult, VerificationResults,
};
use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;
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

#[derive(Debug, Clone, Default)]
pub struct Par1VerifyOptions {
    pub extra_files: Vec<PathBuf>,
    pub purge: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Par1ScanResult {
    pub(crate) results: VerificationResults,
    pub(crate) matches: Vec<Par1FileMatch>,
}

#[derive(Debug, Clone)]
pub(crate) struct Par1FileMatch {
    pub(crate) target_name: String,
    pub(crate) target_path: PathBuf,
    pub(crate) matched_path: Option<PathBuf>,
    pub(crate) status: FileStatus,
}

pub fn verify_par1_file(path: &Path) -> Result<VerificationResults, Par1VerifyError> {
    verify_par1_file_with_options(path, &Par1VerifyOptions::default())
}

pub fn verify_par1_file_with_options(
    path: &Path,
    options: &Par1VerifyOptions,
) -> Result<VerificationResults, Par1VerifyError> {
    let par1_files = crate::par2_files::collect_par1_files(path);
    let recovery_file = par1_files.first().map(PathBuf::as_path).unwrap_or(path);
    let bytes = std::fs::read(recovery_file)?;
    let set = parse_par1_bytes(&bytes)?;
    let base_dir = recovery_file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));

    let entries: Vec<_> = set
        .files
        .iter()
        .filter(|entry| entry.status & PAR1_STATUS_IN_PARITY_VOLUME != 0)
        .collect();
    let scan = scan_par1_files(base_dir, &entries, &options.extra_files)?;

    if options.purge && par1_results_all_present(&scan.results) {
        super::purge::purge_par1_files(&par1_files)?;
    }

    Ok(scan.results)
}

pub(crate) fn scan_par1_files(
    base_dir: &Path,
    entries: &[&Par1FileEntry],
    extra_files: &[PathBuf],
) -> Result<Par1ScanResult, Par1VerifyError> {
    let mut matches: Vec<_> = entries
        .iter()
        .map(|entry| {
            let result = verify_entry(base_dir, entry);
            Par1FileMatch {
                target_path: base_dir.join(&result.file_name),
                target_name: result.file_name,
                matched_path: None,
                status: result.status,
            }
        })
        .collect();

    if matches
        .iter()
        .any(|file_match| file_match.status != FileStatus::Present)
    {
        scan_extra_files(entries, &mut matches, extra_files)?;
    }

    Ok(build_scan_result(entries, matches))
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

pub(crate) fn par1_results_all_present(results: &VerificationResults) -> bool {
    results.renamed_file_count == 0
        && results.missing_file_count == 0
        && results.corrupted_file_count == 0
}

pub(crate) fn local_file_name(name: &str) -> &str {
    name.rsplit(['/', '\\', ':']).next().unwrap_or(name)
}

fn scan_extra_files(
    entries: &[&Par1FileEntry],
    matches: &mut [Par1FileMatch],
    extra_files: &[PathBuf],
) -> Result<(), Par1VerifyError> {
    for extra_path in canonical_extra_files(extra_files) {
        let mut match_index = None;
        for (index, entry) in entries.iter().enumerate() {
            let file_match = &matches[index];
            if file_match.status != FileStatus::Present
                && file_match.matched_path.is_none()
                && extra_file_matches_entry(&extra_path, entry)?
            {
                match_index = Some(index);
                break;
            }
        }

        if let Some(index) = match_index {
            matches[index].status = FileStatus::Renamed;
            matches[index].matched_path = Some(extra_path);
        }
    }
    Ok(())
}

fn canonical_extra_files(extra_files: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::default();
    extra_files
        .iter()
        .filter(|path| !is_par1_recovery_path(path))
        .filter_map(|path| std::fs::canonicalize(path).ok())
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn is_par1_recovery_path(path: &Path) -> bool {
    crate::par2_files::detect_recovery_format(path) == Some(crate::par2_files::RecoveryFormat::Par1)
}

fn extra_file_matches_entry(path: &Path, entry: &Par1FileEntry) -> Result<bool, Par1VerifyError> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || metadata.len() != entry.file_size {
        return Ok(false);
    }

    Ok(calculate_file_md5(path)? == entry.hash_full
        && calculate_file_md5_16k(path)? == entry.hash_16k)
}

fn build_scan_result(entries: &[&Par1FileEntry], matches: Vec<Par1FileMatch>) -> Par1ScanResult {
    let file_results: Vec<_> = entries
        .iter()
        .zip(matches.iter())
        .map(|(entry, file_match)| file_result_from_match(entry, file_match))
        .collect();
    let block_results = file_results
        .iter()
        .map(|file| BlockVerificationResult {
            block_number: 0,
            file_id: file.file_id,
            is_valid: matches!(file.status, FileStatus::Present | FileStatus::Renamed),
            expected_hash: None,
            expected_crc: None,
        })
        .collect();

    Par1ScanResult {
        results: VerificationResults::from_file_results(file_results, block_results, 0),
        matches,
    }
}

fn file_result_from_match(
    entry: &Par1FileEntry,
    file_match: &Par1FileMatch,
) -> FileVerificationResult {
    let blocks_available = usize::from(matches!(
        file_match.status,
        FileStatus::Present | FileStatus::Renamed
    ));
    let damaged_blocks = if blocks_available == 1 {
        Vec::new()
    } else {
        vec![0]
    };

    FileVerificationResult {
        file_name: file_match.target_name.clone(),
        file_id: FileId::new(*entry.hash_full.as_bytes()),
        status: file_match.status,
        blocks_available,
        total_blocks: 1,
        damaged_blocks,
        block_positions: HashMap::default(),
    }
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

    #[test]
    fn scan_marks_missing_target_as_renamed_when_exact_extra_exists() {
        let temp = tempfile::tempdir().unwrap();
        let data = b"renamed par1 data";
        let extra = temp.path().join("wrong-name.bin");
        std::fs::write(&extra, data).unwrap();
        let entry = entry_for("source.bin", data);

        let scan = scan_par1_files(temp.path(), &[&entry], &[extra]).unwrap();

        assert_eq!(scan.results.renamed_file_count, 1);
        assert_eq!(scan.results.files[0].status, FileStatus::Renamed);
        assert_eq!(scan.results.files[0].blocks_available, 1);
        assert!(scan.results.files[0].damaged_blocks.is_empty());
        assert_eq!(scan.results.files[0].file_name, "source.bin");
    }

    #[test]
    fn scan_marks_corrupted_target_as_renamed_when_exact_extra_exists() {
        let temp = tempfile::tempdir().unwrap();
        let data = b"renamed par1 data";
        std::fs::write(temp.path().join("source.bin"), b"corrupted").unwrap();
        let extra = temp.path().join("wrong-name.bin");
        std::fs::write(&extra, data).unwrap();
        let entry = entry_for("source.bin", data);

        let scan = scan_par1_files(temp.path(), &[&entry], &[extra]).unwrap();

        assert_eq!(scan.results.renamed_file_count, 1);
        assert_eq!(scan.results.files[0].status, FileStatus::Renamed);
    }

    #[test]
    fn scan_rejects_extra_file_with_same_size_but_wrong_full_md5() {
        let temp = tempfile::tempdir().unwrap();
        let extra = temp.path().join("wrong-name.bin");
        std::fs::write(&extra, b"wrong").unwrap();
        let entry = entry_for("source.bin", b"right");

        let scan = scan_par1_files(temp.path(), &[&entry], &[extra]).unwrap();

        assert_eq!(scan.results.renamed_file_count, 0);
        assert_eq!(scan.results.files[0].status, FileStatus::Missing);
    }

    #[test]
    fn scan_rejects_extra_file_with_matching_full_md5_but_wrong_16k_md5() {
        let temp = tempfile::tempdir().unwrap();
        let data = b"right";
        let extra = temp.path().join("wrong-name.bin");
        std::fs::write(&extra, data).unwrap();
        let mut entry = entry_for("source.bin", data);
        entry.hash_16k = Md5Hash::new([9; 16]);

        let scan = scan_par1_files(temp.path(), &[&entry], &[extra]).unwrap();

        assert_eq!(scan.results.renamed_file_count, 0);
        assert_eq!(scan.results.files[0].status, FileStatus::Missing);
    }

    #[test]
    fn scan_ignores_par1_recovery_extra_paths() {
        let temp = tempfile::tempdir().unwrap();
        let data = b"renamed par1 data";
        let par_extra = temp.path().join("source.par");
        let volume_extra = temp.path().join("source.P01");
        std::fs::write(&par_extra, data).unwrap();
        std::fs::write(&volume_extra, data).unwrap();
        let entry = entry_for("source.bin", data);

        let scan = scan_par1_files(temp.path(), &[&entry], &[par_extra, volume_extra]).unwrap();

        assert_eq!(scan.results.renamed_file_count, 0);
        assert_eq!(scan.results.files[0].status, FileStatus::Missing);
    }

    #[test]
    fn scan_assigns_one_extra_file_to_only_one_protected_entry() {
        let temp = tempfile::tempdir().unwrap();
        let data = b"duplicate protected data";
        let extra = temp.path().join("wrong-name.bin");
        std::fs::write(&extra, data).unwrap();
        let first = entry_for("first.bin", data);
        let second = entry_for("second.bin", data);

        let scan = scan_par1_files(temp.path(), &[&first, &second], &[extra]).unwrap();

        assert_eq!(scan.results.renamed_file_count, 1);
        assert_eq!(scan.results.missing_file_count, 1);
    }
}
