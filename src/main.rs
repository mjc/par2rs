use binread::BinReaderExt;
use std::fs;
use std::path::Path;

use par2rs::parse_args;
use par2rs::Par2Header;

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
    } else {
        eprintln!("File does not exist: {}", input_file);
    }
}
