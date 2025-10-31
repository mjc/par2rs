use par2rs::verify::{VerificationError, VerificationResult};
use std::error::Error;

#[test]
fn test_verification_error_io_variant() {
    let err = VerificationError::Io("file not found".to_string());
    assert_eq!(err.to_string(), "I/O error: file not found");
}

#[test]
fn test_verification_error_checksum_variant() {
    let err = VerificationError::ChecksumCalculation("invalid hash".to_string());
    assert_eq!(err.to_string(), "Checksum calculation error: invalid hash");
}

#[test]
fn test_verification_error_invalid_metadata_variant() {
    let err = VerificationError::InvalidMetadata("bad length".to_string());
    assert_eq!(err.to_string(), "Invalid metadata: bad length");
}

#[test]
fn test_verification_error_corrupted_data_variant() {
    let err = VerificationError::CorruptedData("crc mismatch".to_string());
    assert_eq!(err.to_string(), "Corrupted data: crc mismatch");
}

#[test]
fn test_verification_error_is_cloneable() {
    let err1 = VerificationError::Io("test".to_string());
    let err2 = err1.clone();
    assert_eq!(err1.to_string(), err2.to_string());
}

#[test]
fn test_verification_error_is_error_trait() {
    let err = VerificationError::Io("test".to_string());
    // Test that it implements Error trait
    let _: &dyn Error = &err;
}

#[test]
fn test_verification_error_from_io_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let ver_err: VerificationError = io_err.into();
    assert!(ver_err.to_string().contains("I/O error"));
    assert!(ver_err.to_string().contains("file not found"));
}

#[test]
fn test_verification_result_ok() {
    let result: VerificationResult<i32> = Ok(42);
    assert!(result.is_ok());
    if let Ok(value) = result {
        assert_eq!(value, 42);
    }
}

#[test]
fn test_verification_result_err() {
    let result: VerificationResult<i32> = Err(VerificationError::Io("error".to_string()));
    assert!(result.is_err());
}

#[test]
fn test_verification_error_debug_format() {
    let err = VerificationError::ChecksumCalculation("test".to_string());
    let debug_str = format!("{:?}", err);
    assert!(debug_str.contains("ChecksumCalculation"));
    assert!(debug_str.contains("test"));
}

#[test]
fn test_all_error_variants_display() {
    let errors: Vec<VerificationError> = vec![
        VerificationError::Io("io msg".to_string()),
        VerificationError::ChecksumCalculation("checksum msg".to_string()),
        VerificationError::InvalidMetadata("metadata msg".to_string()),
        VerificationError::CorruptedData("corrupt msg".to_string()),
    ];

    for err in errors {
        let display_str = err.to_string();
        // Ensure display doesn't panic and produces non-empty string
        assert!(!display_str.is_empty());
    }
}
