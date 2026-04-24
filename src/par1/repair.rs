use super::parser::{parse_par1_bytes, Par1ParseError};
use super::types::{Par1FileEntry, Par1Set};
use super::verify::{
    local_file_name, par1_results_all_present, scan_par1_files, verify_entry, Par1ScanResult,
    Par1VerifyError,
};
use crate::domain::{Md5Hash, RecoverySetId};
use crate::packets::RecoverySlicePacket;
use crate::reed_solomon::codec::ReconstructionEngine;
use crate::repair::error_helpers::move_file_into_place;
use crate::verify::{FileStatus, VerificationResults};
use rustc_hash::FxHashMap as HashMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum Par1RepairError {
    Io(std::io::Error),
    Parse(Par1ParseError),
    MissingRecoverySet,
    InconsistentRecoverySet,
    NotEnoughRecoveryBlocks,
    ReconstructionFailed(String),
}

impl fmt::Display for Par1RepairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Parse(err) => write!(f, "{err}"),
            Self::MissingRecoverySet => write!(f, "no valid PAR1 recovery files found"),
            Self::InconsistentRecoverySet => write!(f, "inconsistent PAR1 recovery set"),
            Self::NotEnoughRecoveryBlocks => write!(f, "not enough PAR1 recovery blocks"),
            Self::ReconstructionFailed(message) => {
                write!(f, "PAR1 reconstruction failed: {message}")
            }
        }
    }
}

impl std::error::Error for Par1RepairError {}

impl From<std::io::Error> for Par1RepairError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<Par1ParseError> for Par1RepairError {
    fn from(error: Par1ParseError) -> Self {
        Self::Parse(error)
    }
}

impl From<Par1VerifyError> for Par1RepairError {
    fn from(error: Par1VerifyError) -> Self {
        match error {
            Par1VerifyError::Io(error) => Self::Io(error),
            Par1VerifyError::Parse(error) => Self::Parse(error),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Par1RepairOptions {
    pub memory_limit: Option<usize>,
    pub extra_files: Vec<PathBuf>,
    pub purge: bool,
}

pub fn repair_par1_file(path: &Path) -> Result<VerificationResults, Par1RepairError> {
    repair_par1_file_with_options(path, &Par1RepairOptions::default())
}

pub fn repair_par1_file_with_memory_limit(
    path: &Path,
    memory_limit: Option<usize>,
) -> Result<VerificationResults, Par1RepairError> {
    repair_par1_file_with_options(
        path,
        &Par1RepairOptions {
            memory_limit,
            ..Par1RepairOptions::default()
        },
    )
}

pub fn repair_par1_file_with_options(
    path: &Path,
    options: &Par1RepairOptions,
) -> Result<VerificationResults, Par1RepairError> {
    let par1_files = crate::par2_files::collect_par1_files(path);
    let base_dir = par1_files
        .first()
        .and_then(|path| path.parent())
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf();

    let sets = load_sets(&par1_files)?;
    let main_set = sets.first().ok_or(Par1RepairError::MissingRecoverySet)?;
    let source_files: Vec<_> = main_set
        .files
        .iter()
        .filter(|entry| entry.is_protected_file())
        .cloned()
        .collect();
    let source_refs: Vec<_> = source_files.iter().collect();
    let block_size = source_files
        .iter()
        .map(|entry| entry.file_size as usize)
        .max()
        .unwrap_or(0);

    let mut backups = Vec::new();
    let initial_scan = scan_par1_files(&base_dir, &source_refs, &options.extra_files)?;
    if par1_results_all_present(&initial_scan.results) {
        if options.purge {
            super::purge::purge_par1_files(&par1_files)?;
        }
        return Ok(initial_scan.results);
    }

    rename_matched_extra_files(&initial_scan, &mut backups)?;
    let after_rename_scan = scan_par1_files(&base_dir, &source_refs, &options.extra_files)?;
    if par1_results_all_present(&after_rename_scan.results) {
        if options.purge {
            super::purge::purge_par1_backups(&backups)?;
            super::purge::purge_par1_files(&par1_files)?;
        }
        return Ok(after_rename_scan.results);
    }

    let missing_indices: Vec<_> = after_rename_scan
        .results
        .files
        .iter()
        .enumerate()
        .filter_map(|(index, file)| (file.status != FileStatus::Present).then_some(index))
        .collect();

    let recovery_packets = recovery_packets_from_sets(&sets, block_size, main_set.set_hash)?;
    if recovery_packets.len() < missing_indices.len() {
        return Err(Par1RepairError::NotEnoughRecoveryBlocks);
    }

    let existing_slices = read_existing_slices(&base_dir, &source_files, block_size)?;
    let reconstructed = reconstruct_missing_slices(
        block_size,
        source_files.len(),
        &recovery_packets,
        &existing_slices,
        &missing_indices,
        options.memory_limit,
    )?;

    for missing_index in missing_indices {
        let entry = &source_files[missing_index];
        let data = reconstructed.get(&missing_index).ok_or_else(|| {
            Par1RepairError::ReconstructionFailed(format!(
                "missing reconstructed slice {missing_index}"
            ))
        })?;
        let output_path = base_dir.join(local_file_name(&entry.name));
        std::fs::write(&output_path, &data[..entry.file_size as usize])?;
    }

    let final_scan = scan_par1_files(&base_dir, &source_refs, &[])?;
    if !par1_results_all_present(&final_scan.results) {
        return Err(Par1RepairError::ReconstructionFailed(
            "final verification still reports missing or damaged files".to_string(),
        ));
    }
    if options.purge {
        super::purge::purge_par1_backups(&backups)?;
        super::purge::purge_par1_files(&par1_files)?;
    }
    Ok(final_scan.results)
}

fn rename_matched_extra_files(
    scan: &Par1ScanResult,
    backups: &mut Vec<PathBuf>,
) -> Result<(), Par1RepairError> {
    for file_match in scan
        .matches
        .iter()
        .filter(|file_match| file_match.status == FileStatus::Renamed)
    {
        let Some(matched_path) = &file_match.matched_path else {
            continue;
        };
        if file_match.target_path.exists() {
            let backup_path = next_backup_path(&file_match.target_path);
            move_file_into_place(&file_match.target_path, &backup_path)?;
            backups.push(backup_path);
        }
        move_file_into_place(matched_path, &file_match.target_path)?;
    }
    Ok(())
}

fn next_backup_path(target_path: &Path) -> PathBuf {
    for suffix in 1usize.. {
        let mut candidate = target_path.as_os_str().to_os_string();
        candidate.push(format!(".{suffix}"));
        let candidate = PathBuf::from(candidate);
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("usize suffix iteration should not terminate")
}

fn reconstruct_missing_slices(
    block_size: usize,
    total_source_files: usize,
    recovery_packets: &[RecoverySlicePacket],
    existing_slices: &HashMap<usize, Vec<u8>>,
    missing_indices: &[usize],
    memory_limit: Option<usize>,
) -> Result<HashMap<usize, Vec<u8>>, Par1RepairError> {
    let chunk_size = crate::repair::calculate_repair_chunk_size(block_size, memory_limit)
        .map_err(|error| Par1RepairError::ReconstructionFailed(error.to_string()))?;

    if chunk_size >= block_size {
        let engine =
            ReconstructionEngine::new(block_size, total_source_files, recovery_packets.to_vec());
        let reconstructed = engine.reconstruct_missing_slices_global(
            existing_slices,
            missing_indices,
            total_source_files,
        );
        return reconstruction_result_to_par1_result(reconstructed);
    }

    let mut reconstructed: HashMap<usize, Vec<u8>> = missing_indices
        .iter()
        .map(|&index| (index, Vec::with_capacity(block_size)))
        .collect();

    for chunk_offset in (0..block_size).step_by(chunk_size) {
        let current_chunk_size = (block_size - chunk_offset).min(chunk_size);
        let chunk_existing: HashMap<usize, Vec<u8>> = existing_slices
            .iter()
            .map(|(&index, data)| {
                (
                    index,
                    data[chunk_offset..chunk_offset + current_chunk_size].to_vec(),
                )
            })
            .collect();
        let chunk_recovery: Vec<RecoverySlicePacket> = recovery_packets
            .iter()
            .map(|packet| RecoverySlicePacket {
                length: packet.length,
                md5: packet.md5,
                set_id: packet.set_id,
                type_of_packet: packet.type_of_packet,
                exponent: packet.exponent,
                recovery_data: packet.recovery_data
                    [chunk_offset..chunk_offset + current_chunk_size]
                    .to_vec(),
            })
            .collect();
        let engine =
            ReconstructionEngine::new(current_chunk_size, total_source_files, chunk_recovery);
        let chunk_result =
            reconstruction_result_to_par1_result(engine.reconstruct_missing_slices_global(
                &chunk_existing,
                missing_indices,
                total_source_files,
            ))?;

        for &missing_index in missing_indices {
            let chunk = chunk_result.get(&missing_index).ok_or_else(|| {
                Par1RepairError::ReconstructionFailed(format!(
                    "missing reconstructed chunk for slice {missing_index}"
                ))
            })?;
            reconstructed
                .get_mut(&missing_index)
                .expect("missing buffer initialized")
                .extend_from_slice(chunk);
        }
    }

    Ok(reconstructed)
}

fn reconstruction_result_to_par1_result(
    reconstructed: crate::reed_solomon::codec::ReconstructionResult,
) -> Result<HashMap<usize, Vec<u8>>, Par1RepairError> {
    if reconstructed.success {
        Ok(reconstructed.reconstructed_slices)
    } else {
        Err(Par1RepairError::ReconstructionFailed(
            reconstructed
                .error_message
                .unwrap_or_else(|| "unknown error".to_string()),
        ))
    }
}

fn load_sets(paths: &[PathBuf]) -> Result<Vec<Par1Set>, Par1RepairError> {
    let mut sets: Vec<Par1Set> = Vec::new();
    for path in paths {
        let bytes = std::fs::read(path)?;
        let set = parse_par1_bytes(&bytes)?;
        if let Some(first) = sets.first() {
            if first.set_hash != set.set_hash || first.files != set.files {
                return Err(Par1RepairError::InconsistentRecoverySet);
            }
        }
        sets.push(set);
    }
    Ok(sets)
}

fn recovery_packets_from_sets(
    sets: &[Par1Set],
    block_size: usize,
    set_hash: Md5Hash,
) -> Result<Vec<RecoverySlicePacket>, Par1RepairError> {
    let mut packets = Vec::new();
    for volume in sets.iter().filter_map(|set| set.volume.as_ref()) {
        if volume.recovery_data.len() != block_size {
            return Err(Par1RepairError::InconsistentRecoverySet);
        }
        packets.push(RecoverySlicePacket {
            length: 0,
            md5: Md5Hash::new([0; 16]),
            set_id: RecoverySetId::new(*set_hash.as_bytes()),
            type_of_packet: crate::packets::recovery_slice_packet::TYPE_OF_PACKET
                .try_into()
                .expect("PAR2 recovery slice packet type is 16 bytes"),
            exponent: volume.exponent,
            recovery_data: volume.recovery_data.clone(),
        });
    }
    packets.sort_by_key(|packet| packet.exponent);
    Ok(packets)
}

fn read_existing_slices(
    base_dir: &Path,
    entries: &[Par1FileEntry],
    block_size: usize,
) -> Result<HashMap<usize, Vec<u8>>, Par1RepairError> {
    let mut slices = HashMap::default();
    for (index, entry) in entries.iter().enumerate() {
        let path = base_dir.join(local_file_name(&entry.name));
        if verify_entry(base_dir, entry).status != FileStatus::Present {
            continue;
        }

        let mut data = Vec::new();
        std::fs::File::open(path)?.read_to_end(&mut data)?;
        data.resize(block_size, 0);
        slices.insert(index, data);
    }
    Ok(slices)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAR1_FLATDATA_FILES: [(&str, &[(usize, u8)]); 10] = [
        (
            "test-0.data",
            &[
                (18_593, 1),
                (11_835, 2),
                (10_742, 3),
                (15_039, 4),
                (9_681, 5),
            ],
        ),
        (
            "test-1.data",
            &[
                (8_834, 5),
                (10_703, 6),
                (10_664, 7),
                (18_085, 8),
                (13_203, 9),
                (17_695, 10),
                (19_023, 11),
                (17_421, 12),
                (14_687, 13),
                (17_226, 14),
                (10_820, 15),
                (13_437, 16),
                (5_376, 17),
            ],
        ),
        (
            "test-2.data",
            &[
                (9_506, 17),
                (14_414, 18),
                (18_750, 19),
                (13_750, 20),
                (14_179, 21),
                (18_476, 22),
                (546, 23),
            ],
        ),
        (
            "test-3.data",
            &[
                (12_735, 23),
                (12_500, 24),
                (13_125, 25),
                (18_437, 26),
                (15_390, 27),
                (12_617, 28),
                (16_171, 29),
                (11_562, 30),
                (11_523, 31),
                (10_156, 32),
                (7_913, 33),
            ],
        ),
        (
            "test-4.data",
            &[
                (10_290, 33),
                (13_984, 34),
                (11_445, 35),
                (11_523, 36),
                (13_281, 37),
                (13_945, 38),
                (18_359, 39),
                (9_298, 40),
            ],
        ),
        (
            "test-5.data",
            &[
                (3_436, 40),
                (16_171, 41),
                (17_812, 42),
                (11_445, 43),
                (11_796, 44),
                (16_289, 45),
                (18_125, 46),
                (4_876, 47),
            ],
        ),
        (
            "test-6.data",
            &[
                (6_374, 47),
                (12_968, 48),
                (13_906, 49),
                (14_453, 50),
                (16_992, 51),
                (13_828, 52),
                (19_335, 53),
                (16_757, 54),
                (14_787, 55),
            ],
        ),
        (
            "test-7.data",
            &[
                (2_322, 55),
                (14_921, 56),
                (14_023, 57),
                (11_015, 58),
                (11_679, 59),
                (11_757, 60),
                (2_018, 61),
            ],
        ),
        (
            "test-8.data",
            &[
                (12_747, 61),
                (17_695, 62),
                (17_500, 63),
                (19_218, 64),
                (3_447, 65),
            ],
        ),
        (
            "test-9.data",
            &[
                (16_474, 65),
                (12_304, 66),
                (16_093, 67),
                (18_710, 68),
                (18_281, 69),
                (18_906, 70),
                (3_177, 71),
            ],
        ),
    ];

    fn par1_flatdata_file_bytes(name: &str) -> Vec<u8> {
        let (_, runs) = PAR1_FLATDATA_FILES
            .iter()
            .find(|(file_name, _)| *file_name == name)
            .copied()
            .unwrap_or_else(|| panic!("unknown PAR1 flatdata file {name}"));
        runs.iter()
            .flat_map(|(len, byte)| std::iter::repeat_n(*byte, *len))
            .collect()
    }

    fn copy_par1_fixture(temp: &tempfile::TempDir) {
        let source_dir = Path::new("tests/fixtures/par1/flatdata");
        ["testdata.par", "testdata.p01", "testdata.p02"]
            .iter()
            .for_each(|name| {
                std::fs::copy(source_dir.join(name), temp.path().join(name)).unwrap();
            });
        PAR1_FLATDATA_FILES.iter().for_each(|(name, _)| {
            std::fs::write(temp.path().join(name), par1_flatdata_file_bytes(name)).unwrap();
        });
    }

    fn remove_par1_volumes(temp: &tempfile::TempDir) {
        std::fs::remove_file(temp.path().join("testdata.p01")).unwrap();
        std::fs::remove_file(temp.path().join("testdata.p02")).unwrap();
    }

    #[test]
    fn repairs_missing_file_from_real_par1_fixture() {
        let temp = tempfile::tempdir().unwrap();
        copy_par1_fixture(&temp);

        std::fs::remove_file(temp.path().join("test-3.data")).unwrap();

        let results = repair_par1_file(&temp.path().join("testdata.par")).unwrap();

        assert_eq!(results.present_file_count, 10);
        assert_eq!(results.missing_file_count, 0);
        assert!(temp.path().join("test-3.data").exists());
    }

    #[test]
    fn repairs_missing_file_from_real_par1_fixture_with_memory_limit() {
        let temp = tempfile::tempdir().unwrap();
        copy_par1_fixture(&temp);

        std::fs::remove_file(temp.path().join("test-3.data")).unwrap();

        let results =
            repair_par1_file_with_memory_limit(&temp.path().join("testdata.par"), Some(64 * 1024))
                .unwrap();

        assert_eq!(results.present_file_count, 10);
        assert_eq!(results.missing_file_count, 0);
        assert_eq!(
            std::fs::read(temp.path().join("test-3.data")).unwrap(),
            par1_flatdata_file_bytes("test-3.data")
        );
    }

    #[test]
    fn repair_rejects_zero_memory_limit() {
        let temp = tempfile::tempdir().unwrap();
        copy_par1_fixture(&temp);

        std::fs::remove_file(temp.path().join("test-3.data")).unwrap();

        let error = repair_par1_file_with_memory_limit(&temp.path().join("testdata.par"), Some(0))
            .unwrap_err();

        assert!(matches!(error, Par1RepairError::ReconstructionFailed(_)));
    }

    #[test]
    fn repairs_corrupted_file_from_real_par1_fixture() {
        let temp = tempfile::tempdir().unwrap();
        copy_par1_fixture(&temp);

        std::fs::write(temp.path().join("test-2.data"), b"corrupted").unwrap();

        let results = repair_par1_file(&temp.path().join("testdata.par")).unwrap();

        assert_eq!(results.present_file_count, 10);
        assert_eq!(results.corrupted_file_count, 0);
        assert_eq!(
            std::fs::read(temp.path().join("test-2.data")).unwrap(),
            par1_flatdata_file_bytes("test-2.data")
        );
    }

    #[test]
    fn repair_fails_when_not_enough_par1_recovery_blocks() {
        let temp = tempfile::tempdir().unwrap();
        copy_par1_fixture(&temp);

        std::fs::remove_file(temp.path().join("test-1.data")).unwrap();
        std::fs::remove_file(temp.path().join("test-2.data")).unwrap();
        std::fs::remove_file(temp.path().join("test-3.data")).unwrap();

        let error = repair_par1_file(&temp.path().join("testdata.par")).unwrap_err();

        assert!(matches!(error, Par1RepairError::NotEnoughRecoveryBlocks));
    }

    #[test]
    fn repair_renames_exact_extra_for_missing_target_without_recovery_blocks() {
        let temp = tempfile::tempdir().unwrap();
        copy_par1_fixture(&temp);
        remove_par1_volumes(&temp);
        let target = temp.path().join("test-3.data");
        let extra = temp.path().join("renamed.data");
        std::fs::rename(&target, &extra).unwrap();

        let results = repair_par1_file_with_options(
            &temp.path().join("testdata.par"),
            &Par1RepairOptions {
                extra_files: vec![extra.clone()],
                ..Par1RepairOptions::default()
            },
        )
        .unwrap();

        assert_eq!(results.present_file_count, 10);
        assert_eq!(
            std::fs::read(&target).unwrap(),
            par1_flatdata_file_bytes("test-3.data")
        );
        assert!(!extra.exists());
    }

    #[test]
    fn repair_backs_up_corrupted_target_before_renaming_exact_extra() {
        let temp = tempfile::tempdir().unwrap();
        copy_par1_fixture(&temp);
        remove_par1_volumes(&temp);
        let target = temp.path().join("test-2.data");
        let extra = temp.path().join("renamed.data");
        std::fs::write(&extra, par1_flatdata_file_bytes("test-2.data")).unwrap();
        std::fs::write(&target, b"corrupted").unwrap();

        let results = repair_par1_file_with_options(
            &temp.path().join("testdata.par"),
            &Par1RepairOptions {
                extra_files: vec![extra.clone()],
                ..Par1RepairOptions::default()
            },
        )
        .unwrap();

        assert_eq!(results.present_file_count, 10);
        assert_eq!(
            std::fs::read(&target).unwrap(),
            par1_flatdata_file_bytes("test-2.data")
        );
        assert_eq!(
            std::fs::read(temp.path().join("test-2.data.1")).unwrap(),
            b"corrupted"
        );
        assert!(!extra.exists());
    }

    #[test]
    fn repair_uses_first_free_numbered_backup_suffix() {
        let temp = tempfile::tempdir().unwrap();
        copy_par1_fixture(&temp);
        remove_par1_volumes(&temp);
        let target = temp.path().join("test-2.data");
        let extra = temp.path().join("renamed.data");
        std::fs::write(&extra, par1_flatdata_file_bytes("test-2.data")).unwrap();
        std::fs::write(&target, b"corrupted").unwrap();
        std::fs::write(temp.path().join("test-2.data.1"), b"existing backup").unwrap();

        repair_par1_file_with_options(
            &temp.path().join("testdata.par"),
            &Par1RepairOptions {
                extra_files: vec![extra],
                ..Par1RepairOptions::default()
            },
        )
        .unwrap();

        assert_eq!(
            std::fs::read(temp.path().join("test-2.data.1")).unwrap(),
            b"existing backup"
        );
        assert_eq!(
            std::fs::read(temp.path().join("test-2.data.2")).unwrap(),
            b"corrupted"
        );
    }

    #[test]
    fn wrong_extra_file_does_not_mask_missing_target() {
        let temp = tempfile::tempdir().unwrap();
        copy_par1_fixture(&temp);
        remove_par1_volumes(&temp);
        std::fs::remove_file(temp.path().join("test-1.data")).unwrap();
        let extra = temp.path().join("renamed.data");
        std::fs::write(&extra, b"wrong same size data").unwrap();

        let error = repair_par1_file_with_options(
            &temp.path().join("testdata.par"),
            &Par1RepairOptions {
                extra_files: vec![extra],
                ..Par1RepairOptions::default()
            },
        )
        .unwrap_err();

        assert!(matches!(error, Par1RepairError::NotEnoughRecoveryBlocks));
    }
}
