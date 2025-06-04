use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Write};

const MAGIC_SEQUENCE: &[u8] = b"PAR2\0PKT";

fn main() -> io::Result<()> {
    let input_file = "tests/fixtures/testfile.par2"; // Replace with your PAR2 file path
    println!("Opening input file: {}", input_file);
    let mut file = File::open(input_file)?;

    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    println!("Read {} bytes from input file", buffer.len());

    let mut seen_packet_types = HashSet::new();
    let mut position = 0;

    while position + MAGIC_SEQUENCE.len() <= buffer.len() {
        // Find the start of the next packet
        if &buffer[position..position + MAGIC_SEQUENCE.len()] == MAGIC_SEQUENCE {
            println!("Found packet start at position: {}", position);

            // Ensure the packet has enough data for the header and type field
            if position + 64 <= buffer.len() {
                let packet_data = &buffer[position..];

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
                    },
                };

                if !seen_packet_types.contains(human_readable_name) {
                    println!("Saving packet type: {}", human_readable_name);
                    let output_file = format!("{}.par2", human_readable_name);
                    let mut output = File::create(output_file)?;
                    output.write_all(packet_data)?;
                    seen_packet_types.insert(human_readable_name.to_string());
                }
            } else {
                println!("Incomplete packet at position: {}", position);
            }

            // Move to the next potential packet
            position += MAGIC_SEQUENCE.len();
        } else {
            position += 1;
        }
    }

    println!("Split into {} unique packet types.", seen_packet_types.len());
    Ok(())
}
