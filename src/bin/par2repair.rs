use std::env;
use std::path::Path;
use std::process;

use par2rs::repair::repair_files;

fn print_usage(program_name: &str) {
    println!("Usage: {} [options] <PAR2 file> [files...]", program_name);
    println!();
    println!("Options:");
    println!("  -h, --help     Show this help message");
    println!("  -v, --verbose  Verbose output");
    println!("  -q, --quiet    Quiet output (errors only)");
    println!();
    println!("Examples:");
    println!("  {} archive.par2", program_name);
    println!("  {} --verbose archive.par2", program_name);
    println!("  {} archive.par2 file1.txt file2.txt", program_name);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program_name = &args[0];

    if args.len() < 2 {
        print_usage(program_name);
        process::exit(1);
    }

    let mut verbose = false;
    let mut quiet = false;
    let mut par2_file = None;
    let mut target_files = Vec::new();
    
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage(program_name);
                process::exit(0);
            }
            "-v" | "--verbose" => {
                verbose = true;
            }
            "-q" | "--quiet" => {
                quiet = true;
            }
            arg if arg.starts_with('-') => {
                eprintln!("Error: Unknown option '{}'", arg);
                print_usage(program_name);
                process::exit(1);
            }
            _ => {
                if par2_file.is_none() {
                    par2_file = Some(args[i].clone());
                } else {
                    target_files.push(args[i].clone());
                }
            }
        }
        i += 1;
    }

    let par2_file = match par2_file {
        Some(file) => file,
        None => {
            eprintln!("Error: No PAR2 file specified");
            print_usage(program_name);
            process::exit(1);
        }
    };

    if !Path::new(&par2_file).exists() {
        eprintln!("Error: PAR2 file '{}' does not exist", par2_file);
        process::exit(1);
    }

    if !quiet {
        println!("Loading PAR2 file: {}", par2_file);
    }

    match repair_files(&par2_file, &target_files, verbose) {
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
