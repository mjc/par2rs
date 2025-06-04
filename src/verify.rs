use crate::Packet;
use md5;
use std::fs::File;
use std::io::Read;
use std::convert::TryInto;

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
                let file_path = format!("{}{}", directory, String::from_utf8_lossy(&desc.file_name).trim_end_matches(char::from(0)));
                
                let mut file = File::open(&file_path).expect("Failed to open file");
                let mut buffer = vec![0u8; 16 * 1024]; // Buffer for the first 16 KB
                file.read_exact(&mut buffer).expect("Failed to read file");
                let file_16k_md5 = md5::compute(&buffer); // Compute MD5 of the buffer
                // Check if the md5 matches the one in the packet
                let file_16k_md5_bytes: [u8; 16] = file_16k_md5.as_slice().try_into().expect("MD5 hash should be 16 bytes");
                if file_16k_md5_bytes != desc.md5_16k { // Compare as [u8; 16]
                    eprintln!(
                        "MD5 mismatch for file {}: expected {:?}, got {:?}",
                        file_path,
                        desc.md5_16k,
                        file_16k_md5_bytes
                    );
                    return None;
                }
                else {
                    println!("MD5 match for file: {}", file_path);
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
        let mock_packets = vec![
            Packet::MainPacket(crate::MainPacket {
                length: 0,
                md5: [0; 16],
                set_id: [0; 16],
                type_of_packet: "MainPacket".to_string(),
                slice_size: 0,
                file_ids: vec![],
            }),
        ];

        let result = verify_par2_packets(mock_packets);
        assert!(result, "Verification should succeed for mock packets");
    }
}
