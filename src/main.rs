use binread::BinRead;
use binread::BinReaderExt;
use clap::{Arg, Command};
use std::fs;
use std::path::Path;

#[derive(Debug, BinRead)]
struct Par2Header {
    magic: [u8; 8], // "PAR2\0PKT"
    length: u64,   // Length of the packet
    md5: [u8; 16], // MD5 hash of the packet
    set_id: [u8; 16], // Unique identifier for the PAR2 set
    type_of_packet: [u8; 16], // Type of the packet
    // Add more fields as per the PAR2 specification
}

fn main() {
    let matches = Command::new("par2rs")
        .version("1.0")
        .author("Your Name <your.email@example.com>")
        .about("A Rust implementation of par2repair")
        .arg(
            Arg::new("input")
                .help("Input file")
                .required(true)
                .value_parser(clap::value_parser!(String)),
        )
        .arg(
            Arg::new("output")
                .help("Output file")
                .required(false)
                .value_parser(clap::value_parser!(String)),
        )
        .get_matches();

    if let Some(input) = matches.get_one::<String>("input") {
        let input_path = Path::new(input);

        let par2_file = if input_path.extension().and_then(|ext| ext.to_str()) != Some("par2") {
            let parent_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
            let par2_files: Vec<_> = fs::read_dir(parent_dir)
                .unwrap()
                .filter_map(|entry| {
                    let entry = entry.unwrap();
                    let path = entry.path();
                    if path.extension().and_then(|ext| ext.to_str()) == Some("par2") {
                        Some(path)
                    } else {
                        None
                    }
                })
                .collect();

            if par2_files.is_empty() {
                eprintln!("No .par2 file found in the same folder as the input file.");
                return;
            }

            par2_files[0].clone()
        } else {
            input_path.to_path_buf()
        };

        println!("Using .par2 file: {}", par2_file.display());

        // Read the .par2 file using binread
        let mut file = fs::File::open(par2_file).unwrap();
        let par2_data: Result<Par2Header, _> = file.read_le();

        match par2_data {
            Ok(data) => println!("Successfully read .par2 file: {:?}", data),
            Err(e) => eprintln!("Failed to read .par2 file: {}", e),
        }
    }

    if let Some(output) = matches.get_one::<String>("output") {
        println!("Output file: {}", output);
    }
}
