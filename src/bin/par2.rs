//! Main par2 binary - drop-in replacement for par2cmdline
//!
//! Supports the same command-line interface as par2cmdline for compatibility

use anyhow::{Context, Result};
use clap::{Arg, ArgAction, Command};

fn main() -> Result<()> {
    env_logger::Builder::from_default_env()
        .format_timestamp(None)
        .format_module_path(false)
        .format_target(false)
        .init();

    let matches = Command::new("par2")
        .version(env!("CARGO_PKG_VERSION"))
        .about("PAR2 file verification and repair utility (Rust implementation)")
        .arg_required_else_help(true)
        .subcommand(
            Command::new("create")
                .visible_alias("c")
                .about("Create PAR2 recovery files")
                .arg(
                    Arg::new("par2_file")
                        .help("Base name for PAR2 files")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::new("files")
                        .help("Files to protect")
                        .required(true)
                        .num_args(1..)
                        .index(2),
                )
                .arg(
                    Arg::new("redundancy")
                        .short('r')
                        .long("redundancy")
                        .help("Redundancy percentage (default: 5)")
                        .value_name("PERCENT")
                        .default_value("5"),
                )
                .arg(
                    Arg::new("block_size")
                        .short('s')
                        .long("block-size")
                        .help("Block size in bytes")
                        .value_name("BYTES"),
                )
                .arg(
                    Arg::new("block_count")
                        .short('b')
                        .long("block-count")
                        .help("Number of recovery blocks")
                        .value_name("COUNT"),
                )
                .arg(
                    Arg::new("recovery_count")
                        .short('n')
                        .long("recovery-count")
                        .help("Number of recovery files")
                        .value_name("COUNT"),
                ),
        )
        .subcommand(
            Command::new("verify")
                .visible_alias("v")
                .about("Verify files using PAR2 data")
                .arg(
                    Arg::new("par2_file")
                        .help("PAR2 file to use for verification")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::new("files")
                        .help("Specific files to verify (optional)")
                        .num_args(0..)
                        .index(2),
                )
                .arg(
                    Arg::new("quiet")
                        .short('q')
                        .long("quiet")
                        .help("Quiet mode - minimal output")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("threads")
                        .short('t')
                        .long("threads")
                        .help("Number of CPU threads for computation (0 = auto-detect)")
                        .value_name("N")
                        .default_value("0"),
                )
                .arg(
                    Arg::new("no-parallel")
                        .long("no-parallel")
                        .help("Disable all parallel processing")
                        .action(ArgAction::SetTrue),
                ),
        )
        .subcommand(
            Command::new("repair")
                .visible_alias("r")
                .about("Repair files using PAR2 recovery data")
                .arg(
                    Arg::new("par2_file")
                        .help("PAR2 file to use for repair")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::new("files")
                        .help("Specific files to repair (optional)")
                        .num_args(0..)
                        .index(2),
                )
                .arg(
                    Arg::new("quiet")
                        .short('q')
                        .long("quiet")
                        .help("Quiet mode - minimal output")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("purge")
                        .short('p')
                        .long("purge")
                        .help("Purge backup files after successful repair")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("threads")
                        .short('t')
                        .long("threads")
                        .help("Number of CPU threads for computation (0 = auto-detect)")
                        .value_name("N")
                        .default_value("0"),
                )
                .arg(
                    Arg::new("no-parallel")
                        .long("no-parallel")
                        .help("Disable all parallel processing")
                        .action(ArgAction::SetTrue),
                ),
        )
        // Backward compatibility: allow command as first positional argument
        .arg(
            Arg::new("command")
                .help("Command (c/create, v/verify, r/repair)")
                .index(1),
        )
        .get_matches();

    // Handle subcommands
    match matches.subcommand() {
        Some(("create", sub_matches)) => handle_create(sub_matches),
        Some(("verify", sub_matches)) => handle_verify(sub_matches),
        Some(("repair", sub_matches)) => handle_repair(sub_matches),
        Some((cmd, _)) => {
            eprintln!("Unknown command: {}", cmd);
            std::process::exit(1);
        }
        None => {
            // No subcommand - show help
            eprintln!("Error: No command specified");
            eprintln!("\nUse 'par2 --help' for usage information");
            std::process::exit(1);
        }
    }
}

fn handle_create(_matches: &clap::ArgMatches) -> Result<()> {
    eprintln!("PAR2 create functionality not yet implemented");
    eprintln!("Use 'par2create' binary directly for now");
    std::process::exit(1);
}

fn handle_verify(matches: &clap::ArgMatches) -> Result<()> {
    use std::path::{Path, PathBuf};

    let par2_file = matches
        .get_one::<String>("par2_file")
        .expect("par2_file is required");
    let quiet = matches.get_flag("quiet");

    // Create verification config from command line arguments
    let verify_config = par2rs::verify::VerificationConfig::from_args(matches);

    // Initialize Rayon thread pool BEFORE any parallel operations
    // This must be done before any par_iter() calls
    let thread_count = verify_config.effective_threads();
    rayon::ThreadPoolBuilder::new()
        .num_threads(thread_count)
        .build_global()
        .ok(); // Ignore error if already initialized

    let file_path = PathBuf::from(par2_file);
    anyhow::ensure!(file_path.exists(), "File does not exist: {}", par2_file);

    // Change to parent directory for file resolution (like par2verify does)
    if let Some(parent) = file_path.parent() {
        std::env::set_current_dir(parent)
            .with_context(|| format!("Failed to set current directory to {}", parent.display()))?;
    }

    // Collect all PAR2 files in the set (use just filename after cd)
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(Path::new)
        .unwrap_or(&file_path);
    let par2_files = par2rs::par2_files::collect_par2_files(file_name);

    if !quiet {
        println!("Loading PAR2 files...\n");
    }

    // Parse packets excluding recovery slices but validate and count them
    // Recovery slice data is NOT loaded into memory (saves gigabytes for large PAR2 sets)
    // but they are validated and counted for repair possibility checking
    let packet_set = par2rs::par2_files::load_par2_packets(&par2_files, false, !quiet);

    if !quiet {
        println!(); // Blank line after loading

        // Show rsummary statistics
        let stats = par2rs::analysis::calculate_par2_stats(
            &packet_set.packets,
            packet_set.recovery_block_count,
        );
        par2rs::analysis::print_summary_stats(&stats);

        println!("\nVerifying source files:\n");
    }

    // Perform comprehensive verification
    let results = if quiet {
        par2rs::verify::comprehensive_verify_files_with_config_and_reporter(
            packet_set,
            &verify_config,
            &par2rs::reporters::SilentVerificationReporter,
        )
    } else {
        par2rs::verify::comprehensive_verify_files_with_config(packet_set, &verify_config)
    };

    if !quiet {
        par2rs::verify::print_verification_results(&results);
    }

    if results.missing_block_count == 0 {
        Ok(())
    } else if results.repair_possible {
        if !quiet {
            eprintln!("\nRepair is required.");
        }
        std::process::exit(1);
    } else {
        if !quiet {
            eprintln!("\nRepair is not possible.");
        }
        std::process::exit(2);
    }
}

fn handle_repair(matches: &clap::ArgMatches) -> Result<()> {
    let par2_file = matches
        .get_one::<String>("par2_file")
        .expect("par2_file is required");
    let quiet = matches.get_flag("quiet");
    let purge = matches.get_flag("purge");

    // Create verification config from command line arguments (like par2repair does)
    let verify_config = par2rs::verify::VerificationConfig::from_args(matches);

    let (context, result) = par2rs::repair::repair_files_with_config(
        par2_file,
        Box::new(par2rs::repair::ConsoleReporter::new(quiet)),
        &verify_config,
    )
    .context("Failed to repair files")?;

    if !quiet {
        context.recovery_set.print_statistics();
        result.print_result();
    }

    if purge && result.is_success() {
        context.purge_files(par2_file)?;
    }

    if result.is_success() {
        Ok(())
    } else {
        anyhow::bail!("Repair failed");
    }
}
