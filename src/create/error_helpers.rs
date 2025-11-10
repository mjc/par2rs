//! Error handling helpers to reduce boilerplate in PAR2 creation
//!
//! This module provides type-safe helper functions for common error patterns,
//! eliminating repetitive `.map_err()` calls throughout the codebase.

use super::error::{CreateError, CreateResult};
use std::fs::File;
use std::io;
use std::path::Path;

/// Open a file for reading, wrapping I/O errors with file context
///
/// # Example
/// ```no_run
/// use par2rs::create::error_helpers::open_for_reading;
/// use std::path::Path;
///
/// let file = open_for_reading(Path::new("test.dat"))?;
/// # Ok::<(), par2rs::create::CreateError>(())
/// ```
pub fn open_for_reading(path: impl AsRef<Path>) -> CreateResult<File> {
    let path = path.as_ref();
    File::open(path).map_err(|e| CreateError::FileReadError {
        file: path.to_string_lossy().to_string(),
        source: e,
    })
}

/// Get file metadata, wrapping I/O errors with file context
///
/// # Example
/// ```no_run
/// use par2rs::create::error_helpers::get_metadata;
/// use std::path::Path;
///
/// let metadata = get_metadata(Path::new("test.dat"))?;
/// # Ok::<(), par2rs::create::CreateError>(())
/// ```
pub fn get_metadata(path: impl AsRef<Path>) -> CreateResult<std::fs::Metadata> {
    let path = path.as_ref();
    std::fs::metadata(path).map_err(|e| CreateError::FileReadError {
        file: path.to_string_lossy().to_string(),
        source: e,
    })
}

/// Create a file for writing, wrapping I/O errors with file context
///
/// # Example
/// ```no_run
/// use par2rs::create::error_helpers::create_file;
/// use std::path::Path;
///
/// let file = create_file(Path::new("output.par2"))?;
/// # Ok::<(), par2rs::create::CreateError>(())
/// ```
pub fn create_file(path: impl AsRef<Path>) -> CreateResult<File> {
    let path = path.as_ref();
    File::create(path).map_err(|e| CreateError::FileCreateError {
        file: path.to_string_lossy().to_string(),
        source: e,
    })
}

/// Helper to wrap packet write errors with descriptive context
///
/// # Example
/// ```no_run
/// use par2rs::create::error_helpers::packet_write_error;
///
/// let err = std::io::Error::new(std::io::ErrorKind::Other, "disk full");
/// let create_err = packet_write_error("MainPacket", err);
/// ```
pub fn packet_write_error(packet_type: &str, error: impl std::fmt::Display) -> CreateError {
    CreateError::Other(format!("Failed to write {}: {}", packet_type, error))
}

/// Wrapper for read operations that automatically maps to FileReadError
///
/// This is useful for chaining read operations where you want consistent error handling.
///
/// # Example
/// ```no_run
/// use par2rs::create::error_helpers::ReadContext;
/// use std::io::Read;
/// use std::path::Path;
///
/// let path = Path::new("test.dat");
/// let mut file = std::fs::File::open(path).unwrap();
/// let mut buffer = vec![0u8; 1024];
/// let mut ctx = ReadContext::new(path);
///
/// // This will wrap any I/O error with file context
/// ctx.read(&mut file, &mut buffer)?;
/// # Ok::<(), par2rs::create::CreateError>(())
/// ```
pub struct ReadContext {
    file_path: String,
}

impl ReadContext {
    /// Create a new read context for a file path
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            file_path: path.as_ref().to_string_lossy().to_string(),
        }
    }

    /// Perform a read operation, wrapping any I/O error with file context
    pub fn read(&mut self, reader: &mut impl io::Read, buf: &mut [u8]) -> CreateResult<usize> {
        reader.read(buf).map_err(|e| CreateError::FileReadError {
            file: self.file_path.clone(),
            source: e,
        })
    }

    /// Perform a read_exact operation, wrapping any I/O error with file context
    pub fn read_exact(&mut self, reader: &mut impl io::Read, buf: &mut [u8]) -> CreateResult<()> {
        reader
            .read_exact(buf)
            .map_err(|e| CreateError::FileReadError {
                file: self.file_path.clone(),
                source: e,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_open_for_reading_success() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("test.txt");
        std::fs::write(&path, b"test content").unwrap();

        let result = open_for_reading(&path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_open_for_reading_nonexistent() {
        let result = open_for_reading("/nonexistent/file.txt");
        assert!(result.is_err());

        match result.unwrap_err() {
            CreateError::FileReadError { file, .. } => {
                assert!(file.contains("nonexistent"));
            }
            _ => panic!("Wrong error type"),
        }
    }

    #[test]
    fn test_get_metadata_success() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("test.txt");
        std::fs::write(&path, b"test").unwrap();

        let result = get_metadata(&path);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 4);
    }

    #[test]
    fn test_get_metadata_nonexistent() {
        let result = get_metadata("/nonexistent/file.txt");
        assert!(result.is_err());

        match result.unwrap_err() {
            CreateError::FileReadError { file, .. } => {
                assert!(file.contains("nonexistent"));
            }
            _ => panic!("Wrong error type"),
        }
    }

    #[test]
    fn test_create_file_success() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("new.txt");

        let result = create_file(&path);
        assert!(result.is_ok());

        // Verify file was created
        assert!(path.exists());
    }

    #[test]
    fn test_create_file_invalid_directory() {
        let result = create_file("/nonexistent/directory/file.txt");
        assert!(result.is_err());

        match result.unwrap_err() {
            CreateError::FileCreateError { file, .. } => {
                assert!(file.contains("nonexistent"));
            }
            _ => panic!("Wrong error type"),
        }
    }

    #[test]
    fn test_packet_write_error_formatting() {
        let io_err = io::Error::other("disk full");
        let err = packet_write_error("MainPacket", io_err);

        let err_string = format!("{}", err);
        assert!(err_string.contains("MainPacket"));
        assert!(err_string.contains("disk full"));
    }

    #[test]
    fn test_packet_write_error_different_types() {
        let err1 = packet_write_error("CreatorPacket", "serialize error");
        let err2 = packet_write_error("RecoveryPacket", io::Error::from(io::ErrorKind::WriteZero));

        assert!(format!("{}", err1).contains("CreatorPacket"));
        assert!(format!("{}", err2).contains("RecoveryPacket"));
    }

    #[test]
    fn test_read_context_new() {
        let ctx = ReadContext::new("/path/to/file.dat");
        assert_eq!(ctx.file_path, "/path/to/file.dat");
    }

    #[test]
    fn test_read_context_read_success() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("test.dat");
        std::fs::write(&path, b"hello world").unwrap();

        let mut file = File::open(&path).unwrap();
        let mut buffer = vec![0u8; 5];
        let mut ctx = ReadContext::new(&path);

        let result = ctx.read(&mut file, &mut buffer);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 5);
        assert_eq!(&buffer, b"hello");
    }

    #[test]
    fn test_read_context_read_exact_success() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("test.dat");
        std::fs::write(&path, b"exact test").unwrap();

        let mut file = File::open(&path).unwrap();
        let mut buffer = vec![0u8; 10];
        let mut ctx = ReadContext::new(&path);

        let result = ctx.read_exact(&mut file, &mut buffer);
        assert!(result.is_ok());
        assert_eq!(&buffer, b"exact test");
    }

    #[test]
    fn test_read_context_error_includes_path() {
        use std::io::Cursor;

        let mut ctx = ReadContext::new("/test/path.dat");
        let mut reader = Cursor::new(vec![1, 2, 3]);
        let mut buffer = vec![0u8; 10]; // Try to read more than available

        let result = ctx.read_exact(&mut reader, &mut buffer);
        assert!(result.is_err());

        match result.unwrap_err() {
            CreateError::FileReadError { file, .. } => {
                assert_eq!(file, "/test/path.dat");
            }
            _ => panic!("Wrong error type"),
        }
    }

    #[test]
    fn test_helpers_with_pathbuf() {
        use std::path::PathBuf;

        let temp = tempdir().unwrap();
        let path = PathBuf::from(temp.path()).join("pathbuf.txt");
        std::fs::write(&path, b"test").unwrap();

        // All helpers should work with PathBuf
        assert!(open_for_reading(&path).is_ok());
        assert!(get_metadata(&path).is_ok());

        let path2 = PathBuf::from(temp.path()).join("new.txt");
        assert!(create_file(&path2).is_ok());
    }

    #[test]
    fn test_helpers_with_str() {
        let temp = tempdir().unwrap();
        let path_str = temp.path().join("str.txt");
        std::fs::write(&path_str, b"test").unwrap();
        let path_str = path_str.to_str().unwrap();

        // All helpers should work with &str
        assert!(open_for_reading(path_str).is_ok());
        assert!(get_metadata(path_str).is_ok());
    }

    #[test]
    fn test_error_variants_are_correct_type() {
        // Verify that helpers return the expected error variants

        let read_err = open_for_reading("/nonexistent").unwrap_err();
        assert!(matches!(read_err, CreateError::FileReadError { .. }));

        let metadata_err = get_metadata("/nonexistent").unwrap_err();
        assert!(matches!(metadata_err, CreateError::FileReadError { .. }));

        let create_err = create_file("/nonexistent/dir/file").unwrap_err();
        assert!(matches!(create_err, CreateError::FileCreateError { .. }));

        let packet_err = packet_write_error("Test", "error");
        assert!(matches!(packet_err, CreateError::Other(_)));
    }
}
