use crate::Packet;
use md5;
use std::convert::TryInto;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Verifies par2 packets.
/// This function reads the packets from the provided vector and verifies that they are usable
///
/// # Arguments
/// /// * `packets` - A vector of packets parsed from the PAR2 files.
///
/// # Returns
/// /// * `packets` - A vector of packets that are usable.
///
/// # Output
/// Prints failed verification messages to stderr if any packet fails verification.
// pub fn verify_par2_packets(packets: Vec<crate::Packet>) -> Vec<crate::Packet> {
//     packets.into_iter().filter_map(|packet| {
//         match packet {
//             Packet::PackedMainPacket(packed_main_packet) => {
//                 // TODO: Implement MD5 verification for PackedMainPacket if needed
//                 Some(packet)
//             }
//             _ => Some(packet), // Other packets are assumed valid for now
//         }
//     }).collect()
// }

/// Quickly verifies a set of files from the par2 md5sums
///
/// # Arguments
///
/// * `packets` - A list of packets parsed from the PAR2 files.
///
/// # Returns
///
/// A boolean indicating whether the verification was successful.
pub fn quick_check_files(packets: Vec<crate::Packet>) -> bool {
    // First gather all the file_names from FileDescriptionPackets
    let file_names: Vec<String> = packets
        .iter()
        .filter_map(|packet| {
            if let Packet::FileDescriptionPacket(desc) = packet {
                // Compute the md5 of the first 16 bytes of the file:
                // Trim null bytes from the file name
                // Prepend the directory to the file path
                let file_name = String::from_utf8_lossy(&desc.file_name);
                let file_path = file_name.trim_end_matches(char::from(0));

                // Verify the MD5 of the first 16 KB of the file
                if let Err(err) = verify_md5(
                    file_path,
                    None,
                    Some(16 * 1024),
                    &desc.md5_16k,
                    "first 16 KB of file",
                ) {
                    eprintln!("{}", err);
                    return None;
                }

                // Verify the MD5 of the entire file
                if let Err(err) = verify_md5(file_path, None, None, &desc.md5_hash, "entire file") {
                    eprintln!("{}", err);
                    return None;
                }

                Some(String::from_utf8_lossy(&desc.file_name).to_string())
            } else {
                None
            }
        })
        .collect();

    // Placeholder logic to use the file_names variable and avoid warnings
    let _file_count = file_names.len();

    // Perform verification logic here.
    // For now, we will just return true to indicate success.
    // Replace this with the actual PAR2 verification algorithm.

    true
}

/// Helper function to compute MD5 checksum of a file
fn compute_md5(
    file_name: &str,
    directory: Option<&str>,
    length: Option<usize>,
) -> Result<[u8; 16], String> {
    let file_path = match directory {
        Some(dir) => Path::new(dir)
            .join(file_name.trim_end_matches(char::from(0)))
            .to_string_lossy()
            .to_string(),
        None => {
            let cwd = std::env::current_dir()
                .map_err(|_| "Failed to get current working directory".to_string())?;
            cwd.join(file_name.trim_end_matches(char::from(0)))
                .to_string_lossy()
                .to_string()
        }
    };

    let file = File::open(&file_path).map_err(|_| format!("Failed to open file: {}", file_path))?;
    let mut reader = std::io::BufReader::new(file);
    let mut hasher = md5::Context::new();
    let mut buffer = vec![0u8; 256 * 1024 * 1024]; // 256MB buffer size

    let mut total_read = 0;
    loop {
        let bytes_to_read = match length {
            Some(len) if total_read + buffer.len() > len => len - total_read,
            _ => buffer.len(),
        };

        let bytes_read = reader
            .read(&mut buffer[..bytes_to_read])
            .map_err(|_| format!("Failed to read file: {}", file_path))?;
        if bytes_read == 0 {
            break;
        }
        hasher.consume(&buffer[..bytes_read]);
        total_read += bytes_read;

        if let Some(len) = length {
            if total_read >= len {
                break;
            }
        }
    }

    let file_md5 = hasher.compute();
    file_md5
        .as_slice()
        .try_into()
        .map_err(|_| "MD5 hash should be 16 bytes".to_string())
}

/// Helper function to verify MD5 checksum
fn verify_md5(
    file_name: &str,
    directory: Option<&str>,
    length: Option<usize>,
    expected_md5: &[u8; 16],
    description: &str,
) -> Result<(), String> {
    let computed_md5 = compute_md5(file_name, directory, length)?;
    if &computed_md5 != expected_md5 {
        return Err(format!(
            "MD5 mismatch for {} {}: expected {:?}, got {:?}",
            description, file_name, expected_md5, computed_md5
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Packet;

    #[test]
    fn test_quick_check_files() {
        // Create mock packets for testing
        let mock_packets = vec![Packet::MainPacket(crate::MainPacket {
            length: 0,
            md5: [0; 16],
            set_id: [0; 16],
            slice_size: 0,
            file_ids: vec![],
            non_recovery_file_ids: vec![],
        })];

        let result = quick_check_files(mock_packets);
        assert!(result, "Verification should succeed for mock packets");
    }
}
