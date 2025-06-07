use std::process;

use par2rs::args::parse_repair_args;
use par2rs::repair::repair_files;

fn main() {
    let matches = parse_repair_args();

    let par2_file = matches.get_one::<String>("par2_file").unwrap();
    let target_files: Vec<String> = matches
        .get_many::<String>("files")
        .unwrap_or_default()
        .map(|s| s.to_string())
        .collect();
    let verbose = matches.get_flag("verbose");
    let quiet = matches.get_flag("quiet");

    if !quiet {
        println!("Loading PAR2 file: {}", par2_file);
    }

    match repair_files(par2_file, &target_files, verbose) {
        Ok(result) => {
            if !quiet {
                println!("Repair operation completed successfully");
                println!("Files repaired: {}", result.files_repaired);
                println!("Files verified: {}", result.files_verified);

                if !result.repaired_files.is_empty() {
                    println!("Repaired files:");
                    for file in &result.repaired_files {
                        println!("  - {}", file);
                    }
                }

                if !result.verified_files.is_empty() {
                    println!("Verified files:");
                    for file in &result.verified_files {
                        println!("  - {}", file);
                    }
                }
            }

            if result.files_repaired > 0 {
                process::exit(0);
            } else if result.files_verified > 0 {
                if !quiet {
                    println!("All files are already intact - no repair needed");
                }
                process::exit(0);
            } else {
                eprintln!("No files could be repaired or verified");
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error during repair: {}", e);
            process::exit(1);
        }
    }
}
