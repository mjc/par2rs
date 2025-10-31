//! Utility functions for verification operations

use crate::packets::FileDescriptionPacket;

/// Extract clean file name from FileDescription packet
///
/// Removes null terminators and converts to UTF-8 string
pub fn extract_file_name(file_desc: &FileDescriptionPacket) -> String {
    String::from_utf8_lossy(&file_desc.file_name)
        .trim_end_matches('\0')
        .to_string()
}
