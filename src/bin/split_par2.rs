use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

const MAGIC_SEQUENCE: &[u8] = b"PAR2\0PKT";

fn main() -> io::Result<()> {
    let input_dir = "tests/fixtures"; // Replace with your PAR2 files directory
    println!("Opening input directory: {}", input_dir);

    let par2_files: Vec<_> = fs::read_dir(input_dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "par2") {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    println!("Found {} PAR2 files.", par2_files.len());

    for input_file in par2_files {
        println!("Processing file: {:?}", input_file);
        let mut file = File::open(&input_file)?;

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        println!("Read {} bytes from input file", buffer.len());

        // Split the buffer into packets based on MAGIC_SEQUENCE
        let mut packets = Vec::new();
        let mut position = 0;

        while position + MAGIC_SEQUENCE.len() <= buffer.len() {
            if &buffer[position..position + MAGIC_SEQUENCE.len()] == MAGIC_SEQUENCE {
                if let Some(next_position) = buffer[position + MAGIC_SEQUENCE.len()..]
                    .windows(MAGIC_SEQUENCE.len())
                    .position(|window| window == MAGIC_SEQUENCE)
                {
                    packets
                        .push(&buffer[position..position + MAGIC_SEQUENCE.len() + next_position]);
                    position += MAGIC_SEQUENCE.len() + next_position;
                } else {
                    packets.push(&buffer[position..]);
                    break;
                }
            } else {
                position += 1;
            }
        }

        println!("Found {} packets.", packets.len());

        let mut seen_packet_types = HashSet::new();

        // Ensure the output directory exists
        let output_dir = Path::new("tests/fixtures/packets");
        if let Err(e) = fs::create_dir_all(output_dir) {
            println!("Failed to create output directory {:?}: {}", output_dir, e);
            return Err(e);
        }

        for packet_data in packets {
            if packet_data.len() >= 64 {
                // Extract the packet type field (8 + 8 + 16 + 16 = 48 bytes offset)
                let packet_type_bytes = &packet_data[48..64];
                let human_readable_name = match packet_type_bytes {
                    b"PAR 2.0\0Main\0\0\0\0" => "MainPacket",
                    b"PAR 2.0\0PkdMain\0" => "PackedMainPacket",
                    b"PAR 2.0\0FileDesc" => "FileDescriptionPacket",
                    b"PAR 2.0\0RecvSlic" => "RecoverySlicePacket",
                    b"PAR 2.0\0Creator\0" => "CreatorPacket",
                    b"PAR 2.0\0IFSC\0\0\0\0" => "InputFileSliceChecksumPacket",
                    _ => {
                        println!("Unknown packet type: {:02X?}", packet_type_bytes);
                        "UnknownPacket"
                    }
                };

                // Debug: Print the length of the packet
                println!("Packet length: {}", packet_data.len());

                // Debug: Correctly interpret the length field's value (first 8 bytes of the packet) as a little-endian u64
                // Check if the file size matches the length field value
                if packet_data.len() >= 16 {
                    match packet_data[8..16].try_into() {
                        Ok(bytes) => {
                            let length_field = u64::from_le_bytes(bytes);
                            if length_field != packet_data.len() as u64 {
                                println!("Error: Packet length field value ({}) does not match actual packet size ({}).", length_field, packet_data.len());
                            }
                        }
                        Err(_) => {
                            println!("Error: Failed to extract length field from packet.");
                        }
                    }
                } else {
                    println!("Packet too short to extract length field as u64.");
                }

                if !seen_packet_types.contains(human_readable_name) {
                    // Update the output file path
                    let output_file = output_dir.join(format!("{}.par2", human_readable_name));
                    println!(
                        "Attempting to save packet type: {} to file: {:?}",
                        human_readable_name, output_file
                    );
                    match File::create(&output_file) {
                        Ok(mut output) => {
                            if let Err(e) = output.write_all(packet_data) {
                                println!("Failed to write to file {:?}: {}", output_file, e);
                            } else {
                                println!("Successfully wrote to file: {:?}", output_file);
                                seen_packet_types.insert(human_readable_name.to_string());

                                // Verify the length of the newly written file
                                match output.metadata() {
                                    Ok(metadata) => {
                                        let written_file_size = metadata.len();
                                        if written_file_size != packet_data.len() as u64 {
                                            println!("Error: Written file size ({}) does not match packet size ({}).", written_file_size, packet_data.len());
                                        } else {
                                            println!(
                                                "File size verification successful: {} bytes.",
                                                written_file_size
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        println!(
                                            "Failed to retrieve metadata for file {:?}: {}",
                                            output_file, e
                                        );
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            println!("Failed to create file {:?}: {}", output_file, e);
                        }
                    }
                }
            } else {
                println!("Incomplete packet detected.");
            }
        }

        println!(
            "Split into {} unique packet types.",
            seen_packet_types.len()
        );
    }

    Ok(())
}
