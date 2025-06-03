use binread::BinReaderExt;
use std::fs;
use std::io::{Read, Seek};
use std::path::Path;

use par2rs::parse_args;
use par2rs::{MainPacket, PackedMainPacket, FileDescriptionPacket, RecoverySlicePacket, CreatorPacket};

fn parse_packets(file: &mut fs::File) {
    loop {
        let mut magic = [0u8; 8];
        if let Err(_) = file.read_exact(&mut magic) {
            break; // End of file or error
        }

        if &magic != b"PAR2\0PKT" {
            eprintln!("Invalid magic sequence, stopping parsing.");
            break;
        }

        file.seek(std::io::SeekFrom::Current(-8)).expect("Failed to rewind to the beginning of the packet");
        let mut header = [0u8; 64]; // Full 64-byte header including type_of_packet
        
        file.read_exact(&mut header).expect("Failed to read header");

        let type_of_packet = &header[48..64]; // Adjusted to correctly extract the last 16 bytes of the common header

        match type_of_packet {
            b"PAR 2.0\0Main\0\0\0\0" => {
                file.seek(std::io::SeekFrom::Current(-64)).expect("Failed to rewind to the beginning of the packet");
                let _: MainPacket = file.read_le().expect("Failed to read MainPacket");
                println!("Parsed MainPacket successfully");
            }
            b"PAR 2.0\0PkdMain\0" => {
                let _: PackedMainPacket = file.read_le().expect("Failed to read PackedMainPacket");
                println!("Parsed PackedMainPacket successfully");
            }
            b"PAR 2.0\0FileDesc" => {
                let _: FileDescriptionPacket = file.read_le().expect("Failed to read FileDescriptionPacket");
                println!("Parsed FileDescriptionPacket successfully");
            }
            b"PAR 2.0\0RecvSlic" => {
                let _: RecoverySlicePacket = file.read_le().expect("Failed to read RecoverySlicePacket");
                println!("Parsed RecoverySlicePacket successfully");
            }
            b"PAR 2.0\0Creator\0" => {
                let _: CreatorPacket = file.read_le().expect("Failed to read CreatorPacket");
                println!("Parsed CreatorPacket successfully");
            }
            _ => {
                eprintln!("Unknown packet type: {:?}", String::from_utf8_lossy(&type_of_packet));
                break;
            }
        }
    }
}

fn main() {
    let matches = parse_args();

    let input_file = matches.get_one::<String>("input").expect("Input file is required");
    let output_file = matches.get_one::<String>("output");

    println!("Input file: {}", input_file);
    if let Some(output) = output_file {
        println!("Output file: {}", output);
    }

    let file_path = Path::new(input_file);
    if file_path.exists() {
        let mut file = fs::File::open(file_path).expect("Failed to open file");
        parse_packets(&mut file);
    } else {
        eprintln!("File does not exist: {}", input_file);
    }
}
