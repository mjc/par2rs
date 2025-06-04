use std::fs;
use std::path::Path;

use par2rs::parse_args;
use rayon::prelude::*;

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
    if !file_path.exists() {
        eprintln!("File does not exist: {}", input_file);
        return;
    }

    // Collect all .par2 files in the folder, including the input file
    let par2_files = collect_par2_files(file_path);

    let all_packets: Vec<_> = par2_files
        .par_iter()
        .map(|par2_file| {
            let mut file = fs::File::open(par2_file).expect("Failed to open .par2 file");
            let packets = par2rs::parse_packets(&mut file);
            println!("Parsed {} packets from {:?}", packets.len(), par2_file);
            packets
        })
        .flatten()
        .collect();

    println!("Total packets collected: {}", all_packets.len());
}

fn collect_par2_files(file_path: &Path) -> Vec<std::path::PathBuf> {
    let mut par2_files = vec![file_path.to_path_buf()];

    if let Some(folder_path) = file_path.parent() {
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
                }),
        );
    }

    println!("Found .par2 files: {:?}", par2_files);
    par2_files
}
