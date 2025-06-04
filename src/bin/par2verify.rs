use std::fs;
use std::path::{Path, PathBuf};

use par2rs::verify::quick_check_files;
use rayon::prelude::*;

fn main() {
    let matches = par2rs::parse_args();

    let input_file = matches
        .get_one::<String>("input")
        .expect("Input file is required");

    println!("Input file: {}", input_file);

    let file_path = Path::new(input_file);
    if !file_path.exists() {
        eprintln!("File does not exist: {}", input_file);
        return;
    }

    if let Some(parent) = file_path.parent() {
        if let Err(err) = std::env::set_current_dir(parent) {
            eprintln!(
                "Failed to set current directory to {}: {}",
                parent.display(),
                err
            );
            return;
        }
    }

    let all_packets = collect_par2_files(file_path)
        .par_iter()
        .flat_map(|par2_file| parse_par2_file(par2_file))
        .collect::<Vec<_>>();

    println!("Total packets collected: {}", all_packets.len());

    let verified_packets = verify_packets(all_packets);

    quick_check_files(verified_packets);
}

fn verify_packets(packets: Vec<par2rs::Packet>) -> Vec<par2rs::Packet> {
    let mut verified_packets = vec![];

    for packet in packets {
        if packet.verify() {
            verified_packets.push(packet);
        } else {
            eprintln!("Packet verification failed: {:?}", packet);
        }
    }

    println!("Total verified packets: {}", verified_packets.len());
    verified_packets
}

fn collect_par2_files(file_path: &Path) -> Vec<PathBuf> {
    let mut par2_files = vec![file_path.to_path_buf()];

    if let Some(folder_path) = file_path.parent() {
        par2_files.extend(
            fs::read_dir(folder_path)
                .expect("Failed to read directory")
                .filter_map(|entry| {
                    let path = entry.ok()?.path();
                    (path.extension().map_or(false, |ext| ext == "par2") && path != file_path)
                        .then_some(path)
                }),
        );
    }

    println!("Found .par2 files: {:?}", par2_files);
    par2_files
}

fn parse_par2_file(par2_file: &Path) -> Vec<par2rs::Packet> {
    let mut file = fs::File::open(par2_file).expect("Failed to open .par2 file");
    let packets = par2rs::parse_packets(&mut file);
    println!("Parsed {} packets from {:?}", packets.len(), par2_file);
    packets
}
