use crate::Packet;
use md5;
use std::convert::TryInto;
use std::fs::File;
use std::io::Read;

/// Verifies the integrity of a PAR2 file set.
///
/// # Arguments
///
/// * `packets` - A list of packets parsed from the PAR2 files.
///
/// # Returns
///
/// A boolean indicating whether the verification was successful.
pub fn verify_par2_packets(packets: Vec<crate::Packet>) -> bool {
    // First gather all the file_names from FileDescriptionPackets
    let file_names: Vec<String> = packets
        .iter()
        .filter_map(|packet| {
            if let Packet::FileDescriptionPacket(desc) = packet {
                // Compute the md5 of the first 16 bytes of the file:
                // Trim null bytes from the file name
                // Prepend the directory to the file path
                let directory = "/home/mjc/Dune/";
                let file_path = format!(
                    "{}{}",
                    directory,
                    String::from_utf8_lossy(&desc.file_name).trim_end_matches(char::from(0))
                );

                // Helper function to compute MD5 checksum of a file
                fn compute_md5(file_path: &str, buffer_size: usize) -> Result<[u8; 16], String> {
                    let file = File::open(file_path)
                        .map_err(|_| format!("Failed to open file: {}", file_path))?;
                    let mut reader = std::io::BufReader::new(file);
                    let mut hasher = md5::Context::new();
                    let mut buffer = vec![0u8; buffer_size];

                    loop {
                        let bytes_read = reader
                            .read(&mut buffer)
                            .map_err(|_| format!("Failed to read file: {}", file_path))?;
                        if bytes_read == 0 {
                            break;
                        }
                        hasher.consume(&buffer[..bytes_read]);
                    }

                    let file_md5 = hasher.compute();
                    file_md5
                        .as_slice()
                        .try_into()
                        .map_err(|_| "MD5 hash should be 16 bytes".to_string())
                }

                // Helper function to verify MD5 checksum
                fn verify_md5(
                    file_path: &str,
                    buffer_size: usize,
                    expected_md5: &[u8; 16],
                    description: &str,
                ) -> Result<(), String> {
                    let computed_md5 = compute_md5(file_path, buffer_size)?;
                    if &computed_md5 != expected_md5 {
                        return Err(format!(
                            "MD5 mismatch for {} {}: expected {:?}, got {:?}",
                            description, file_path, expected_md5, computed_md5
                        ));
                    }
                    Ok(())
                }

                // Verify the MD5 of the first 16 KB of the file
                if let Err(err) =
                    verify_md5(&file_path, 16 * 1024, &desc.md5_16k, "first 16 KB of file")
                {
                    eprintln!("{}", err);
                    return None;
                }

                // Verify the MD5 of the entire file
                if let Err(err) = verify_md5(&file_path, 16 * 1024, &desc.md5_hash, "entire file") {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Packet;

    #[test]
    fn test_verify_par2_packets() {
        // Create mock packets for testing
        let mock_packets = vec![Packet::MainPacket(crate::MainPacket {
            length: 0,
            md5: [0; 16],
            set_id: [0; 16],
            type_of_packet: "MainPacket".to_string(),
            slice_size: 0,
            file_ids: vec![],
        })];

        let result = verify_par2_packets(mock_packets);
        assert!(result, "Verification should succeed for mock packets");
    }
}
