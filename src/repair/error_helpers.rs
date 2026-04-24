//! Error handling helpers to reduce boilerplate in PAR2 repair operations
//!
//! This module provides type-safe helper functions for common error patterns,
//! eliminating repetitive `.map_err()` calls throughout the repair codebase.

use super::error::{RepairError, Result as RepairResult};
use super::slice_provider::error::{Result as SliceProviderResult, SliceProviderError};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Open a file for reading, wrapping I/O errors with file context
///
/// # Example
/// ```no_run
/// use par2rs::repair::error_helpers::open_for_reading;
/// use std::path::Path;
///
/// let file = open_for_reading(Path::new("test.dat"))?;
/// # Ok::<(), par2rs::repair::RepairError>(())
/// ```
pub fn open_for_reading(path: impl AsRef<Path>) -> RepairResult<File> {
    let path = path.as_ref();
    File::open(path).map_err(|source| RepairError::FileOpenError {
        file: path.to_path_buf(),
        source,
    })
}

/// Create a file for writing, wrapping I/O errors with file context
///
/// # Example
/// ```no_run
/// use par2rs::repair::error_helpers::create_file;
/// use std::path::Path;
///
/// let file = create_file(Path::new("output.dat"))?;
/// # Ok::<(), par2rs::repair::RepairError>(())
/// ```
pub fn create_file(path: impl AsRef<Path>) -> RepairResult<File> {
    let path = path.as_ref();
    File::create(path).map_err(|source| RepairError::FileCreateError {
        file: path.to_path_buf(),
        source,
    })
}

/// Move a file into place, falling back to copy+sync+remove across filesystems
///
/// # Example
/// ```no_run
/// use par2rs::repair::error_helpers::move_file_into_place;
/// use std::path::Path;
///
/// move_file_into_place(Path::new("temp.dat"), Path::new("final.dat"))?;
/// # Ok::<(), std::io::Error>(())
/// ```
pub fn move_file_into_place(
    temp_path: impl AsRef<Path>,
    final_path: impl AsRef<Path>,
) -> io::Result<()> {
    let temp_path = temp_path.as_ref();
    let final_path = final_path.as_ref();
    move_file_into_place_impl(temp_path, final_path, |source, dest| {
        std::fs::rename(source, dest)
    })
}

/// Rename a file, wrapping I/O errors with repair path context.
pub fn rename_file(temp_path: impl AsRef<Path>, final_path: impl AsRef<Path>) -> RepairResult<()> {
    let temp_path = temp_path.as_ref();
    let final_path = final_path.as_ref();
    move_file_into_place(temp_path, final_path).map_err(|source| RepairError::FileRenameError {
        temp_path: temp_path.to_path_buf(),
        final_path: final_path.to_path_buf(),
        source,
    })
}

fn move_file_into_place_impl<F>(temp_path: &Path, final_path: &Path, rename_fn: F) -> io::Result<()>
where
    F: Fn(&Path, &Path) -> io::Result<()>,
{
    match rename_fn(temp_path, final_path) {
        Ok(()) => Ok(()),
        Err(source) if is_cross_device_rename(&source) => {
            copy_file_for_cross_device_move(temp_path, final_path)?;
            std::fs::remove_file(temp_path)
        }
        Err(source) => Err(source),
    }
}

fn is_cross_device_rename(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::CrossesDevices) || error.raw_os_error() == Some(18)
}

fn copy_file_for_cross_device_move(temp_path: &Path, final_path: &Path) -> io::Result<()> {
    let mut source = File::open(temp_path)?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(final_path)?;

    match io::copy(&mut source, &mut destination) {
        Ok(_) => destination.sync_all(),
        Err(error) => {
            let _ = std::fs::remove_file(final_path);
            Err(error)
        }
    }
}

/// Delete a file, wrapping I/O errors with file context
///
/// # Example
/// ```no_run
/// use par2rs::repair::error_helpers::delete_file;
/// use std::path::Path;
///
/// delete_file(Path::new("temp.dat"))?;
/// # Ok::<(), par2rs::repair::RepairError>(())
/// ```
pub fn delete_file(path: impl AsRef<Path>) -> RepairResult<()> {
    let path = path.as_ref();
    std::fs::remove_file(path).map_err(|source| RepairError::FileDeleteError {
        file: path.to_path_buf(),
        source,
    })
}

/// Flush a writer, wrapping I/O errors with file context
///
/// # Example
/// ```no_run
/// use par2rs::repair::error_helpers::flush_writer;
/// use std::fs::File;
/// use std::io::BufWriter;
/// use std::path::Path;
///
/// let file = File::create("output.dat")?;
/// let mut writer = BufWriter::new(file);
/// // ... write data ...
/// flush_writer(&mut writer, Path::new("output.dat"))?;
/// # Ok::<(), par2rs::repair::RepairError>(())
/// ```
pub fn flush_writer<W: Write>(writer: &mut W, path: impl AsRef<Path>) -> RepairResult<()> {
    let path = path.as_ref();
    writer
        .flush()
        .map_err(|source| RepairError::FileFlushError {
            file: path.to_path_buf(),
            source,
        })
}

/// Seek to a position in a file, wrapping I/O errors with file context
///
/// # Example
/// ```no_run
/// use par2rs::repair::error_helpers::seek_file;
/// use std::fs::File;
/// use std::io::SeekFrom;
/// use std::path::Path;
///
/// let mut file = File::open("test.dat")?;
/// seek_file(&mut file, SeekFrom::Start(1024), Path::new("test.dat"))?;
/// # Ok::<(), par2rs::repair::RepairError>(())
/// ```
pub fn seek_file<F: Seek>(
    file: &mut F,
    pos: SeekFrom,
    path: impl AsRef<Path>,
) -> RepairResult<u64> {
    let path = path.as_ref();
    file.seek(pos).map_err(|source| {
        let offset = seek_target_for_error(file, pos);
        RepairError::FileSeekError {
            file: path.to_path_buf(),
            offset,
            source,
        }
    })
}

/// Read exact number of bytes from a file, wrapping I/O errors with slice context
///
/// # Example
/// ```no_run
/// use par2rs::repair::error_helpers::read_slice_exact;
/// use std::fs::File;
/// use std::path::Path;
///
/// let mut file = File::open("test.dat")?;
/// let mut buffer = vec![0u8; 1024];
/// read_slice_exact(&mut file, &mut buffer, Path::new("test.dat"), 0)?;
/// # Ok::<(), par2rs::repair::RepairError>(())
/// ```
pub fn read_slice_exact<R: Read>(
    reader: &mut R,
    buf: &mut [u8],
    path: impl AsRef<Path>,
    slice_index: usize,
) -> RepairResult<()> {
    let path = path.as_ref();
    reader
        .read_exact(buf)
        .map_err(|source| RepairError::SliceReadError {
            file: path.to_path_buf(),
            slice_index,
            source,
        })
}

/// Write entire buffer to a file, wrapping I/O errors with slice context
///
/// # Example
/// ```no_run
/// use par2rs::repair::error_helpers::write_slice_all;
/// use std::fs::File;
/// use std::path::Path;
///
/// let mut file = File::create("output.dat")?;
/// let buffer = vec![0u8; 1024];
/// write_slice_all(&mut file, &buffer, Path::new("output.dat"), 0)?;
/// # Ok::<(), par2rs::repair::RepairError>(())
/// ```
pub fn write_slice_all<W: Write>(
    writer: &mut W,
    buf: &[u8],
    path: impl AsRef<Path>,
    slice_index: usize,
) -> RepairResult<()> {
    let path = path.as_ref();
    writer
        .write_all(buf)
        .map_err(|source| RepairError::SliceWriteError {
            file: path.to_path_buf(),
            slice_index,
            source,
        })
}

// SliceProvider-specific helpers

/// Open a file for reading in slice provider context
///
/// # Example
/// ```ignore
/// use par2rs::repair::error_helpers::slice_provider_open;
/// let file = slice_provider_open("test.dat")?;
/// ```
pub fn slice_provider_open(path: impl AsRef<Path>) -> SliceProviderResult<File> {
    let path = path.as_ref();
    File::open(path).map_err(|source| SliceProviderError::FileOpenError {
        path: path.to_path_buf(),
        source,
    })
}

/// Seek in slice provider file context
///
/// # Example
/// ```ignore
/// use par2rs::repair::error_helpers::slice_provider_seek;
/// let mut file = File::open("test.dat")?;
/// slice_provider_seek(&mut file, SeekFrom::Start(1024), "test.dat")?;
/// ```
pub fn slice_provider_seek<F: Seek>(
    file: &mut F,
    pos: SeekFrom,
    path: impl AsRef<Path>,
) -> SliceProviderResult<u64> {
    let path = path.as_ref();
    file.seek(pos).map_err(|source| {
        let offset = seek_target_for_error(file, pos);
        SliceProviderError::FileSeekError {
            path: path.to_path_buf(),
            offset,
            source,
        }
    })
}

fn seek_target_for_error<F: Seek>(file: &mut F, pos: SeekFrom) -> u64 {
    match pos {
        SeekFrom::Start(offset) => offset,
        SeekFrom::Current(delta) => file
            .stream_position()
            .map(|current| apply_signed_offset(current, delta))
            .unwrap_or(0),
        SeekFrom::End(delta) => {
            let original = file.stream_position().ok();
            let end = file.seek(SeekFrom::End(0)).ok();
            if let Some(original) = original {
                let _ = file.seek(SeekFrom::Start(original));
            }
            end.map(|end| apply_signed_offset(end, delta)).unwrap_or(0)
        }
    }
}

#[cfg(test)]
mod cross_device_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn rename_file_falls_back_to_copy_on_cross_device_error() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source.bin");
        let dest = dir.path().join("dest.bin");
        std::fs::write(&source, b"repair data").unwrap();

        move_file_into_place_impl(&source, &dest, |_src, _dest| {
            Err(io::Error::from(io::ErrorKind::CrossesDevices))
        })
        .unwrap();

        assert!(!source.exists());
        assert_eq!(std::fs::read(&dest).unwrap(), b"repair data");
    }

    #[test]
    fn rename_file_reports_copy_cleanup_failures() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source.bin");
        let dest = dir.path().join("dest.bin");
        std::fs::write(&source, b"repair data").unwrap();
        std::fs::write(&dest, b"existing").unwrap();

        let error = move_file_into_place_impl(&source, &dest, |_src, _dest| {
            Err(io::Error::from(io::ErrorKind::CrossesDevices))
        })
        .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(source.exists());
        assert_eq!(std::fs::read(&dest).unwrap(), b"existing");
    }
}

fn apply_signed_offset(base: u64, delta: i64) -> u64 {
    if delta >= 0 {
        base.saturating_add(delta as u64)
    } else {
        base.saturating_sub(delta.unsigned_abs())
    }
}

/// Read exact bytes in slice provider context
///
/// # Example
/// ```ignore
/// use par2rs::repair::error_helpers::slice_provider_read_exact;
/// let mut file = File::open("test.dat")?;
/// let mut buffer = vec![0u8; 1024];
/// slice_provider_read_exact(&mut file, &mut buffer, "test.dat")?;
/// ```
pub fn slice_provider_read_exact<R: Read>(
    reader: &mut R,
    buf: &mut [u8],
    path: impl AsRef<Path>,
) -> SliceProviderResult<()> {
    let path = path.as_ref();
    reader
        .read_exact(buf)
        .map_err(|source| SliceProviderError::FileReadError {
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_open_for_reading_success() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test.dat");
        std::fs::write(&file_path, b"test data").unwrap();

        let result = open_for_reading(&file_path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_open_for_reading_not_found() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("nonexistent.dat");

        let result = open_for_reading(&file_path);
        assert!(result.is_err());
        match result.unwrap_err() {
            RepairError::FileOpenError { file, .. } => {
                assert_eq!(file, file_path);
            }
            _ => panic!("Expected FileOpenError"),
        }
    }

    #[test]
    fn test_create_file_success() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("new.dat");

        let result = create_file(&file_path);
        assert!(result.is_ok());
        assert!(file_path.exists());
    }

    #[test]
    fn test_create_file_invalid_path() {
        let result = create_file("/nonexistent/directory/file.dat");
        assert!(result.is_err());
        match result.unwrap_err() {
            RepairError::FileCreateError { file, .. } => {
                assert_eq!(file, PathBuf::from("/nonexistent/directory/file.dat"));
            }
            _ => panic!("Expected FileCreateError"),
        }
    }

    #[test]
    fn test_rename_file_success() {
        let temp_dir = tempdir().unwrap();
        let temp_path = temp_dir.path().join("temp.dat");
        let final_path = temp_dir.path().join("final.dat");
        std::fs::write(&temp_path, b"test").unwrap();

        let result = rename_file(&temp_path, &final_path);
        assert!(result.is_ok());
        assert!(!temp_path.exists());
        assert!(final_path.exists());
    }

    #[test]
    fn test_rename_file_source_not_found() {
        let temp_dir = tempdir().unwrap();
        let temp_path = temp_dir.path().join("nonexistent.dat");
        let final_path = temp_dir.path().join("final.dat");

        let result = rename_file(&temp_path, &final_path);
        assert!(result.is_err());
        match result.unwrap_err() {
            RepairError::FileRenameError {
                temp_path: tp,
                final_path: fp,
                ..
            } => {
                assert_eq!(tp, temp_path);
                assert_eq!(fp, final_path);
            }
            _ => panic!("Expected FileRenameError"),
        }
    }

    #[test]
    fn test_delete_file_success() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("delete_me.dat");
        std::fs::write(&file_path, b"test").unwrap();

        let result = delete_file(&file_path);
        assert!(result.is_ok());
        assert!(!file_path.exists());
    }

    #[test]
    fn test_delete_file_not_found() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("nonexistent.dat");

        let result = delete_file(&file_path);
        assert!(result.is_err());
        match result.unwrap_err() {
            RepairError::FileDeleteError { file, .. } => {
                assert_eq!(file, file_path);
            }
            _ => panic!("Expected FileDeleteError"),
        }
    }

    #[test]
    fn test_flush_writer_success() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("flush.dat");
        let file = File::create(&file_path).unwrap();
        let mut writer = std::io::BufWriter::new(file);
        writer.write_all(b"test data").unwrap();

        let result = flush_writer(&mut writer, &file_path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_seek_file_success() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("seek.dat");
        std::fs::write(&file_path, b"0123456789").unwrap();
        let mut file = File::open(&file_path).unwrap();

        let result = seek_file(&mut file, SeekFrom::Start(5), &file_path);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 5);
    }

    #[test]
    fn test_seek_file_negative_current_error_uses_saturated_offset() {
        let file_path = PathBuf::from("cursor.dat");
        let mut cursor = Cursor::new(Vec::<u8>::new());

        let result = seek_file(&mut cursor, SeekFrom::Current(-1), &file_path);

        assert!(result.is_err());
        match result.unwrap_err() {
            RepairError::FileSeekError { offset, .. } => {
                assert_eq!(offset, 0);
            }
            _ => panic!("Expected FileSeekError"),
        }
    }

    #[test]
    fn test_read_slice_exact_success() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("read.dat");
        std::fs::write(&file_path, b"test data").unwrap();
        let mut file = File::open(&file_path).unwrap();
        let mut buffer = vec![0u8; 4];

        let result = read_slice_exact(&mut file, &mut buffer, &file_path, 0);
        assert!(result.is_ok());
        assert_eq!(&buffer, b"test");
    }

    #[test]
    fn test_read_slice_exact_eof() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("read.dat");
        std::fs::write(&file_path, b"short").unwrap();
        let mut file = File::open(&file_path).unwrap();
        let mut buffer = vec![0u8; 100];

        let result = read_slice_exact(&mut file, &mut buffer, &file_path, 0);
        assert!(result.is_err());
        match result.unwrap_err() {
            RepairError::SliceReadError {
                file, slice_index, ..
            } => {
                assert_eq!(file, file_path);
                assert_eq!(slice_index, 0);
            }
            _ => panic!("Expected SliceReadError"),
        }
    }

    #[test]
    fn test_write_slice_all_success() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("write.dat");
        let mut file = File::create(&file_path).unwrap();
        let buffer = b"test data";

        let result = write_slice_all(&mut file, buffer, &file_path, 0);
        assert!(result.is_ok());

        let content = std::fs::read(&file_path).unwrap();
        assert_eq!(content, b"test data");
    }

    #[test]
    fn test_slice_provider_open_success() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test.dat");
        std::fs::write(&file_path, b"test data").unwrap();

        let result = slice_provider_open(&file_path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_slice_provider_open_not_found() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("nonexistent.dat");

        let result = slice_provider_open(&file_path);
        assert!(result.is_err());
        match result.unwrap_err() {
            SliceProviderError::FileOpenError { path, .. } => {
                assert_eq!(path, file_path);
            }
            _ => panic!("Expected FileOpenError"),
        }
    }

    #[test]
    fn test_slice_provider_seek_success() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("seek.dat");
        std::fs::write(&file_path, b"0123456789").unwrap();
        let mut file = File::open(&file_path).unwrap();

        let result = slice_provider_seek(&mut file, SeekFrom::Start(5), &file_path);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 5);
    }

    #[test]
    fn test_slice_provider_seek_negative_current_error_uses_saturated_offset() {
        let file_path = PathBuf::from("cursor.dat");
        let mut cursor = Cursor::new(Vec::<u8>::new());

        let result = slice_provider_seek(&mut cursor, SeekFrom::Current(-1), &file_path);

        assert!(result.is_err());
        match result.unwrap_err() {
            SliceProviderError::FileSeekError { offset, .. } => {
                assert_eq!(offset, 0);
            }
            _ => panic!("Expected FileSeekError"),
        }
    }

    #[test]
    fn test_slice_provider_read_exact_success() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("read.dat");
        std::fs::write(&file_path, b"test data").unwrap();
        let mut file = File::open(&file_path).unwrap();
        let mut buffer = vec![0u8; 4];

        let result = slice_provider_read_exact(&mut file, &mut buffer, &file_path);
        assert!(result.is_ok());
        assert_eq!(&buffer, b"test");
    }

    #[test]
    fn test_slice_provider_read_exact_eof() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("read.dat");
        std::fs::write(&file_path, b"short").unwrap();
        let mut file = File::open(&file_path).unwrap();
        let mut buffer = vec![0u8; 100];

        let result = slice_provider_read_exact(&mut file, &mut buffer, &file_path);
        assert!(result.is_err());
        match result.unwrap_err() {
            SliceProviderError::FileReadError { path, .. } => {
                assert_eq!(path, file_path);
            }
            _ => panic!("Expected FileReadError"),
        }
    }
}
