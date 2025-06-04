use std::fs;
use std::path::Path;

use par2rs::parse_args;

fn main() {
    let matches = parse_args();

    let input_file = matches
        .get_one::<String>("input")
        .expect("Input file is required");
    let output_file = matches.get_one::<String>("output");

    println!("Input file: {}", input_file);
    if let Some(output) = output_file {
        println!("Output file: {}", output);
    }

    let file_path = Path::new(input_file);
    if file_path.exists() {
        let mut file = fs::File::open(file_path).expect("Failed to open file");

        let packets = par2rs::parse_packets(&mut file);

        // Here you can do something with the packets, like processing or saving them
        println!("Parsed {} packets", packets.len());
    } else {
        eprintln!("File does not exist: {}", input_file);
    }
}
