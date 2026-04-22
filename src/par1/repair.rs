use super::parser::{parse_par1_bytes, Par1ParseError};
use super::types::{Par1FileEntry, Par1Set};
use super::verify::{local_file_name, verify_entry};
use crate::domain::{Md5Hash, RecoverySetId};
use crate::packets::RecoverySlicePacket;
use crate::reed_solomon::codec::ReconstructionEngine;
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

pub fn repair_par1_file(path: &Path) -> Result<VerificationResults, Par1RepairError> {
    repair_par1_file_with_memory_limit(path, None)
}

pub fn repair_par1_file_with_memory_limit(
    path: &Path,
    memory_limit: Option<usize>,
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
    let block_size = source_files
        .iter()
        .map(|entry| entry.file_size as usize)
        .max()
        .unwrap_or(0);

    let initial_results = verify_entries(&base_dir, &source_files);
    let missing_indices: Vec<_> = initial_results
        .files
        .iter()
        .enumerate()
        .filter_map(|(index, file)| (file.status != FileStatus::Present).then_some(index))
        .collect();
    if missing_indices.is_empty() {
        return Ok(initial_results);
    }

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
        memory_limit,
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

    Ok(verify_entries(&base_dir, &source_files))
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

fn verify_entries(base_dir: &Path, entries: &[Par1FileEntry]) -> VerificationResults {
    let file_results: Vec<_> = entries
        .iter()
        .map(|entry| verify_entry(base_dir, entry))
        .collect();
    let block_results = file_results
        .iter()
        .map(|file| crate::verify::BlockVerificationResult {
            block_number: 0,
            file_id: file.file_id,
            is_valid: file.status == FileStatus::Present,
            expected_hash: None,
            expected_crc: None,
        })
        .collect();
    VerificationResults::from_file_results(file_results, block_results, 0)
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

    #[test]
    fn repairs_missing_file_from_real_par1_fixture() {
        let source_dir = Path::new("tests/fixtures/par1/flatdata");
        let temp = tempfile::tempdir().unwrap();
        for entry in std::fs::read_dir(source_dir).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), temp.path().join(entry.file_name())).unwrap();
        }

        std::fs::remove_file(temp.path().join("test-3.data")).unwrap();

        let results = repair_par1_file(&temp.path().join("testdata.par")).unwrap();

        assert_eq!(results.present_file_count, 10);
        assert_eq!(results.missing_file_count, 0);
        assert!(temp.path().join("test-3.data").exists());
    }

    #[test]
    fn repairs_missing_file_from_real_par1_fixture_with_memory_limit() {
        let source_dir = Path::new("tests/fixtures/par1/flatdata");
        let temp = tempfile::tempdir().unwrap();
        for entry in std::fs::read_dir(source_dir).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), temp.path().join(entry.file_name())).unwrap();
        }

        std::fs::remove_file(temp.path().join("test-3.data")).unwrap();

        let results =
            repair_par1_file_with_memory_limit(&temp.path().join("testdata.par"), Some(64 * 1024))
                .unwrap();

        assert_eq!(results.present_file_count, 10);
        assert_eq!(results.missing_file_count, 0);
        assert_eq!(
            std::fs::read(temp.path().join("test-3.data")).unwrap(),
            std::fs::read(source_dir.join("test-3.data")).unwrap()
        );
    }

    #[test]
    fn repair_rejects_zero_memory_limit() {
        let source_dir = Path::new("tests/fixtures/par1/flatdata");
        let temp = tempfile::tempdir().unwrap();
        for entry in std::fs::read_dir(source_dir).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), temp.path().join(entry.file_name())).unwrap();
        }

        std::fs::remove_file(temp.path().join("test-3.data")).unwrap();

        let error = repair_par1_file_with_memory_limit(&temp.path().join("testdata.par"), Some(0))
            .unwrap_err();

        assert!(matches!(error, Par1RepairError::ReconstructionFailed(_)));
    }

    #[test]
    fn repairs_corrupted_file_from_real_par1_fixture() {
        let source_dir = Path::new("tests/fixtures/par1/flatdata");
        let temp = tempfile::tempdir().unwrap();
        for entry in std::fs::read_dir(source_dir).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), temp.path().join(entry.file_name())).unwrap();
        }

        std::fs::write(temp.path().join("test-2.data"), b"corrupted").unwrap();

        let results = repair_par1_file(&temp.path().join("testdata.par")).unwrap();

        assert_eq!(results.present_file_count, 10);
        assert_eq!(results.corrupted_file_count, 0);
        assert_eq!(
            std::fs::read(temp.path().join("test-2.data")).unwrap(),
            std::fs::read(source_dir.join("test-2.data")).unwrap()
        );
    }

    #[test]
    fn repair_fails_when_not_enough_par1_recovery_blocks() {
        let source_dir = Path::new("tests/fixtures/par1/flatdata");
        let temp = tempfile::tempdir().unwrap();
        for entry in std::fs::read_dir(source_dir).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), temp.path().join(entry.file_name())).unwrap();
        }

        std::fs::remove_file(temp.path().join("test-1.data")).unwrap();
        std::fs::remove_file(temp.path().join("test-2.data")).unwrap();
        std::fs::remove_file(temp.path().join("test-3.data")).unwrap();

        let error = repair_par1_file(&temp.path().join("testdata.par")).unwrap_err();

        assert!(matches!(error, Par1RepairError::NotEnoughRecoveryBlocks));
    }
}
