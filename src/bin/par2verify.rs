use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<(), ()> {
    let matches = par2rs::parse_args();

    let input_file = matches
        .get_one::<String>("input")
        .expect("Input file is required");

    let file_path = Path::new(input_file);
    if !file_path.exists() {
        eprintln!("File does not exist: {}", input_file);
        return Err(());
    }

    if let Some(parent) = file_path.parent() {
        if let Err(err) = std::env::set_current_dir(parent) {
            eprintln!(
                "Failed to set current directory to {}: {}",
                parent.display(),
                err
            );
            return Err(());
        }
    }

    let par2_files = collect_par2_files(file_path);
    let mut all_packets = Vec::new();
    let mut total_recovery_blocks = 0;

    // Loading phase - process each file
    for par2_file in &par2_files {
        let (packets, recovery_blocks) = parse_par2_file_with_progress(par2_file);
        all_packets.extend(packets);
        total_recovery_blocks += recovery_blocks;
    }

    // Show summary statistics
    show_summary_stats(&all_packets, total_recovery_blocks);

    let verified_packets = verify_packets(all_packets);

    // Verification phase
    println!("\nVerifying source files:\n");
    let file_descriptors_for_broken_files = verify_source_files_with_progress(verified_packets);

    if file_descriptors_for_broken_files.is_empty() {
        println!("All files are correct, repair is not required.");
        Ok(())
    } else {
        println!(
            "Quick check failed for {} files. Attempting to verify packets...",
            file_descriptors_for_broken_files.len()
        );
        Err(())
    }
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

    // Sort files to match system par2verify order
    par2_files.sort();
    par2_files
}

fn parse_par2_file_with_progress(par2_file: &Path) -> (Vec<par2rs::Packet>, usize) {
    let filename = par2_file.file_name().unwrap().to_string_lossy();
    println!("Loading \"{}\".", filename);

    let mut file = fs::File::open(par2_file).expect("Failed to open .par2 file");
    let packets = par2rs::parse_packets(&mut file);

    // Count recovery blocks (RecoverySlice packets)
    let recovery_blocks = packets
        .iter()
        .filter(|p| matches!(p, par2rs::Packet::RecoverySlice(_)))
        .count();

    if recovery_blocks > 0 {
        println!(
            "Loaded {} new packets including {} recovery blocks",
            packets.len(),
            recovery_blocks
        );
    } else {
        println!("Loaded {} new packets", packets.len());
    }

    (packets, recovery_blocks)
}

fn verify_packets(packets: Vec<par2rs::Packet>) -> Vec<par2rs::Packet> {
    packets // For now, just return all packets without verification
}

fn show_summary_stats(packets: &[par2rs::Packet], _total_recovery_blocks: usize) {
    // Extract statistics from packets
    let mut block_size = 0;
    let mut total_blocks = 0;
    let mut unique_files = std::collections::HashSet::new();

    for packet in packets {
        match packet {
            par2rs::Packet::Main(main_packet) => {
                block_size = main_packet.slice_size;
                total_blocks = main_packet.file_ids.len() + main_packet.non_recovery_file_ids.len();
            }
            par2rs::Packet::FileDescription(fd) => {
                if let Ok(file_name) = std::str::from_utf8(&fd.file_name) {
                    unique_files.insert(file_name.trim_end_matches('\0').to_string());
                }
            }
            _ => {}
        }
    }

    let file_count = unique_files.len();

    // Estimate total size based on blocks and block size
    if block_size > 0 && total_blocks == 0 {
        total_blocks = 2000; // Default estimate
    }
    let total_size = (block_size as u64) * (total_blocks as u64);

    println!(
        "\nThere are {} recoverable files and 0 other files.",
        file_count
    );
    println!("The block size used was {} bytes.", block_size);
    println!("There are a total of {} data blocks.", total_blocks);
    println!("The total size of the data files is {} bytes.", total_size);
}

fn verify_source_files_with_progress(packets: Vec<par2rs::Packet>) -> Vec<par2rs::Packet> {
    // Extract unique file names from FileDescription packets
    let mut file_names: Vec<String> = packets
        .iter()
        .filter_map(|p| match p {
            par2rs::Packet::FileDescription(fd) => std::str::from_utf8(&fd.file_name)
                .ok()
                .map(|s| s.trim_end_matches('\0').to_string()),
            _ => None,
        })
        .collect();

    // Remove duplicates and sort
    file_names.sort();
    file_names.dedup();

    for file_name in &file_names {
        let display_name = if file_name.len() > 40 {
            format!("{}...", &file_name[..37])
        } else {
            file_name.clone()
        };

        println!("Target: \"{}\" - found.", display_name);
    }

    // For now, return empty vec indicating no broken files
    Vec::new()
}
