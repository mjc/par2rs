use super::types::{
    Par1FileEntry, Par1Set, Par1Volume, PAR1_FILE_ENTRY_FIXED_SIZE, PAR1_FILE_VERSION,
    PAR1_HEADER_SIZE, PAR1_MAGIC,
};
use crate::checksum::compute_md5_only;
use crate::domain::Md5Hash;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Par1ParseError {
    TooSmall,
    InvalidMagic,
    UnsupportedVersion(u32),
    InvalidControlHash,
    InvalidVolumeNumber(u64),
    InvalidFileList,
    InvalidEntry,
}

impl fmt::Display for Par1ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooSmall => write!(f, "PAR1 file is too small"),
            Self::InvalidMagic => write!(f, "invalid PAR1 magic"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported PAR1 file version {version:#x}")
            }
            Self::InvalidControlHash => write!(f, "invalid PAR1 control hash"),
            Self::InvalidVolumeNumber(volume) => write!(f, "invalid PAR1 volume number {volume}"),
            Self::InvalidFileList => write!(f, "invalid PAR1 file list"),
            Self::InvalidEntry => write!(f, "invalid PAR1 file entry"),
        }
    }
}

impl std::error::Error for Par1ParseError {}

#[derive(Debug, Clone, Copy)]
struct Par1Header {
    program_version: u32,
    control_hash: Md5Hash,
    set_hash: Md5Hash,
    volume_number: u64,
    number_of_files: u64,
    file_list_offset: u64,
    file_list_size: u64,
    data_offset: u64,
    data_size: u64,
}

pub fn parse_par1_bytes(bytes: &[u8]) -> Result<Par1Set, Par1ParseError> {
    let header = parse_header(bytes)?;
    validate_header(&header, bytes)?;
    let files = parse_file_list(
        &bytes[header.file_list_offset as usize
            ..(header.file_list_offset + header.file_list_size) as usize],
        header.number_of_files,
    )?;

    let volume = if header.volume_number > 0 {
        Some(Par1Volume {
            exponent: (header.volume_number - 1) as u32,
            data_offset: header.data_offset,
            data_size: header.data_size,
            recovery_data: bytes
                [header.data_offset as usize..(header.data_offset + header.data_size) as usize]
                .to_vec(),
        })
    } else {
        None
    };

    Ok(Par1Set {
        set_hash: header.set_hash,
        program_version: header.program_version,
        volume_number: header.volume_number,
        files,
        volume,
    })
}

fn parse_header(bytes: &[u8]) -> Result<Par1Header, Par1ParseError> {
    if bytes.len() < PAR1_HEADER_SIZE {
        return Err(Par1ParseError::TooSmall);
    }
    if &bytes[0..8] != PAR1_MAGIC {
        return Err(Par1ParseError::InvalidMagic);
    }

    let file_version = read_u32(bytes, 8)?;
    if file_version != PAR1_FILE_VERSION {
        return Err(Par1ParseError::UnsupportedVersion(file_version));
    }

    Ok(Par1Header {
        program_version: read_u32(bytes, 12)?,
        control_hash: read_md5(bytes, 16)?,
        set_hash: read_md5(bytes, 32)?,
        volume_number: read_u64(bytes, 48)?,
        number_of_files: read_u64(bytes, 56)?,
        file_list_offset: read_u64(bytes, 64)?,
        file_list_size: read_u64(bytes, 72)?,
        data_offset: read_u64(bytes, 80)?,
        data_size: read_u64(bytes, 88)?,
    })
}

fn validate_header(header: &Par1Header, bytes: &[u8]) -> Result<(), Par1ParseError> {
    let actual_control_hash = compute_md5_only(&bytes[32..]);
    if actual_control_hash != header.control_hash {
        return Err(Par1ParseError::InvalidControlHash);
    }
    if header.volume_number >= 256 {
        return Err(Par1ParseError::InvalidVolumeNumber(header.volume_number));
    }
    if header.number_of_files == 0
        || header.file_list_offset < PAR1_HEADER_SIZE as u64
        || header.file_list_size == 0
    {
        return Err(Par1ParseError::InvalidFileList);
    }

    let file_list_end = header
        .file_list_offset
        .checked_add(header.file_list_size)
        .ok_or(Par1ParseError::InvalidFileList)?;
    if file_list_end > bytes.len() as u64 {
        return Err(Par1ParseError::InvalidFileList);
    }

    if header.data_size > 0 {
        let data_end = header
            .data_offset
            .checked_add(header.data_size)
            .ok_or(Par1ParseError::InvalidFileList)?;
        if header.data_offset < PAR1_HEADER_SIZE as u64 || data_end > bytes.len() as u64 {
            return Err(Par1ParseError::InvalidFileList);
        }
        let file_list_range = header.file_list_offset..file_list_end;
        let data_range = header.data_offset..data_end;
        if file_list_range.contains(&header.data_offset)
            || data_range.contains(&header.file_list_offset)
        {
            return Err(Par1ParseError::InvalidFileList);
        }
    }

    Ok(())
}

fn parse_file_list(
    mut bytes: &[u8],
    expected_file_count: u64,
) -> Result<Vec<Par1FileEntry>, Par1ParseError> {
    let mut entries = Vec::new();

    for _ in 0..expected_file_count {
        if bytes.len() < PAR1_FILE_ENTRY_FIXED_SIZE {
            return Err(Par1ParseError::InvalidEntry);
        }

        let entry_size = read_u64(bytes, 0)? as usize;
        if entry_size <= PAR1_FILE_ENTRY_FIXED_SIZE || entry_size > bytes.len() {
            return Err(Par1ParseError::InvalidEntry);
        }
        if !(entry_size - PAR1_FILE_ENTRY_FIXED_SIZE).is_multiple_of(2) {
            return Err(Par1ParseError::InvalidEntry);
        }

        let name = decode_utf16le_name(&bytes[PAR1_FILE_ENTRY_FIXED_SIZE..entry_size]);
        entries.push(Par1FileEntry {
            status: read_u64(bytes, 8)?,
            file_size: read_u64(bytes, 16)?,
            hash_full: read_md5(bytes, 24)?,
            hash_16k: read_md5(bytes, 40)?,
            name,
        });

        bytes = &bytes[entry_size..];
    }

    if !bytes.is_empty() {
        return Err(Par1ParseError::InvalidEntry);
    }

    Ok(entries)
}

fn decode_utf16le_name(bytes: &[u8]) -> String {
    let mut code_units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();

    while code_units.last() == Some(&0) {
        code_units.pop();
    }

    String::from_utf16_lossy(&code_units)
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Par1ParseError> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or(Par1ParseError::TooSmall)?;
    Ok(u32::from_le_bytes(value.try_into().unwrap()))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, Par1ParseError> {
    let value = bytes
        .get(offset..offset + 8)
        .ok_or(Par1ParseError::TooSmall)?;
    Ok(u64::from_le_bytes(value.try_into().unwrap()))
}

fn read_md5(bytes: &[u8], offset: usize) -> Result<Md5Hash, Par1ParseError> {
    let value = bytes
        .get(offset..offset + 16)
        .ok_or(Par1ParseError::TooSmall)?;
    Ok(Md5Hash::new(value.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::par1::types::PAR1_STATUS_IN_PARITY_VOLUME;

    fn build_entry(name: &str, status: u64, file_size: u64) -> Vec<u8> {
        let mut name_bytes = Vec::new();
        for code_unit in name.encode_utf16() {
            name_bytes.extend_from_slice(&code_unit.to_le_bytes());
        }

        let entry_size = (PAR1_FILE_ENTRY_FIXED_SIZE + name_bytes.len()) as u64;
        let mut entry = Vec::new();
        entry.extend_from_slice(&entry_size.to_le_bytes());
        entry.extend_from_slice(&status.to_le_bytes());
        entry.extend_from_slice(&file_size.to_le_bytes());
        entry.extend_from_slice(&[0xAA; 16]);
        entry.extend_from_slice(&[0xBB; 16]);
        entry.extend_from_slice(&name_bytes);
        entry
    }

    fn build_file(entries: Vec<Vec<u8>>, volume_number: u64) -> Vec<u8> {
        let file_list: Vec<u8> = entries.into_iter().flatten().collect();
        let file_list_offset = PAR1_HEADER_SIZE as u64;
        let file_list_size = file_list.len() as u64;
        let data_offset = if volume_number > 0 {
            file_list_offset + file_list_size
        } else {
            0
        };
        let data_size = if volume_number > 0 { 1024u64 } else { 0u64 };

        let mut bytes = Vec::new();
        bytes.extend_from_slice(PAR1_MAGIC);
        bytes.extend_from_slice(&PAR1_FILE_VERSION.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&[0x11; 16]);
        bytes.extend_from_slice(&volume_number.to_le_bytes());
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes.extend_from_slice(&file_list_offset.to_le_bytes());
        bytes.extend_from_slice(&file_list_size.to_le_bytes());
        bytes.extend_from_slice(&data_offset.to_le_bytes());
        bytes.extend_from_slice(&data_size.to_le_bytes());
        bytes.extend_from_slice(&file_list);
        bytes.extend(std::iter::repeat_n(0xCC, data_size as usize));

        let control_hash = compute_md5_only(&bytes[32..]);
        bytes[16..32].copy_from_slice(control_hash.as_bytes());
        bytes
    }

    #[test]
    fn parses_valid_par1_main_file() {
        let bytes = build_file(
            vec![build_entry("file.txt", PAR1_STATUS_IN_PARITY_VOLUME, 1234)],
            0,
        );

        let set = parse_par1_bytes(&bytes).unwrap();

        assert_eq!(set.set_hash, Md5Hash::new([0x11; 16]));
        assert_eq!(set.volume_number, 0);
        assert!(set.volume.is_none());
        assert_eq!(set.files.len(), 1);
        assert_eq!(set.files[0].name, "file.txt");
        assert_eq!(set.files[0].file_size, 1234);
        assert!(set.files[0].is_protected_file());
    }

    #[test]
    fn parses_valid_par1_recovery_volume() {
        let bytes = build_file(
            vec![build_entry("file.txt", PAR1_STATUS_IN_PARITY_VOLUME, 1234)],
            2,
        );

        let set = parse_par1_bytes(&bytes).unwrap();

        assert_eq!(set.volume_number, 2);
        assert_eq!(
            set.volume,
            Some(Par1Volume {
                exponent: 1,
                data_offset: PAR1_HEADER_SIZE as u64
                    + set.files[0].name.encode_utf16().count() as u64 * 2
                    + PAR1_FILE_ENTRY_FIXED_SIZE as u64,
                data_size: 1024,
                recovery_data: vec![0xCC; 1024],
            })
        );
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = build_file(
            vec![build_entry("file.txt", PAR1_STATUS_IN_PARITY_VOLUME, 1234)],
            0,
        );
        bytes[0] = b'X';

        assert_eq!(parse_par1_bytes(&bytes), Err(Par1ParseError::InvalidMagic));
    }

    #[test]
    fn rejects_bad_control_hash() {
        let mut bytes = build_file(
            vec![build_entry("file.txt", PAR1_STATUS_IN_PARITY_VOLUME, 1234)],
            0,
        );
        bytes[40] ^= 0xFF;

        assert_eq!(
            parse_par1_bytes(&bytes),
            Err(Par1ParseError::InvalidControlHash)
        );
    }

    #[test]
    fn rejects_truncated_file_list() {
        let mut bytes = build_file(
            vec![build_entry("file.txt", PAR1_STATUS_IN_PARITY_VOLUME, 1234)],
            0,
        );
        bytes.truncate(bytes.len() - 1);

        assert_eq!(
            parse_par1_bytes(&bytes),
            Err(Par1ParseError::InvalidControlHash)
        );
    }
}
