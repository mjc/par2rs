use binread::BinReaderExt;
use std::fs;
use std::path::Path;

use par2rs::parse_args;
use par2rs::{Par2Header, MainPacket, FileDescriptionPacket, InputFileSliceChecksumPacket, RecoverySlicePacket, CreatorPacket};

fn main() {
    let matches = parse_args();

    let input_file = matches.get_one::<String>("input").expect("Input file is required");
    let output_file = matches.get_one::<String>("output");

    println!("Input file: {}", input_file);
    if let Some(output) = output_file {
        println!("Output file: {}", output);
    }

    // Example usage of Par2Header (to avoid unused field warnings)
    let file_path = Path::new(input_file);
    if file_path.exists() {
        let mut file = fs::File::open(file_path).expect("Failed to open file");
        let header: Par2Header = file.read_le().expect("Failed to read Par2Header");
        println!("Parsed Par2Header: {:?}", header);
        println!("Magic: {:?}", header.magic);
        println!("Length: {}", header.length);
        println!("MD5: {:x?}", header.md5);
        println!("Set ID: {:x?}", header.set_id);
        println!("Type of Packet: {:x?}", header.type_of_packet);

        // Parse the rest of the PAR2 file
        let mut file = fs::File::open(file_path).expect("Failed to open file");

        // Read the main packet
        let main_packet: MainPacket = file.read_le().expect("Failed to read MainPacket");
        println!("Parsed MainPacket: {:?}", main_packet);

        // Read file description packets
        for _ in 0..main_packet.file_count {
            let file_description: FileDescriptionPacket = file.read_le().expect("Failed to read FileDescriptionPacket");
            println!("Parsed FileDescriptionPacket: {:?}", file_description);
        }

        // Read input file slice checksum packets
        for _ in 0..main_packet.file_count {
            let input_file_slice_checksum: InputFileSliceChecksumPacket = file.read_le().expect("Failed to read InputFileSliceChecksumPacket");
            println!("Parsed InputFileSliceChecksumPacket: {:?}", input_file_slice_checksum);
        }

        // Read recovery slice packets
        for _ in 0..header.recovery_block_count {
            let recovery_slice: RecoverySlicePacket = file.read_le().expect("Failed to read RecoverySlicePacket");
            println!("Parsed RecoverySlicePacket: {:?}", recovery_slice);
        }

        // Read creator packet
        let creator_packet: CreatorPacket = file.read_le().expect("Failed to read CreatorPacket");
        println!("Parsed CreatorPacket: {:?}", creator_packet);
    } else {
        eprintln!("File does not exist: {}", input_file);
    }
}
