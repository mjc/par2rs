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

        // Add the input file to the list of .par2 files to parse
        let mut par2_files: Vec<_> = vec![file_path.to_path_buf()];

        // Find additional .par2 files in the folder
        let folder_path = file_path.parent().expect("Failed to get parent folder");
        par2_files.extend(
            fs::read_dir(folder_path)
                .expect("Failed to read directory")
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    let path = entry.path();
                    if path.extension().map_or(false, |ext| ext == "par2") && path != file_path {
                        Some(path)
                    } else {
                        None
                    }
                })
        );

        println!("Found .par2 files: {:?}", par2_files);

        let mut all_packets = Vec::new();

        for par2_file in par2_files {
            let mut file = fs::File::open(&par2_file).expect("Failed to open .par2 file");
            let packets = par2rs::parse_packets(&mut file);
            println!("Parsed {} packets from {:?}", packets.len(), par2_file);
            all_packets.extend(packets);
        }

        println!("Total packets collected: {}", all_packets.len());
    } else {
        eprintln!("File does not exist: {}", input_file);
    }
}
