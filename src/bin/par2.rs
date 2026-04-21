//! Main par2 binary - drop-in replacement for par2cmdline
//!
//! Supports the same command-line interface as par2cmdline for compatibility

use anyhow::{Context, Result};
use clap::{Arg, ArgAction, Command};
use par2rs::create::cli::{expand_source_files, parse_redundancy_option, RedundancyOption};
use par2rs::reporters::VerificationReporter;
use std::path::PathBuf;

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
                // Global options (match par2cmdline)
                .arg(
                    Arg::new("basepath")
                        .short('B')
                        .help("Set the basepath to use as reference for the datafiles")
                        .value_name("PATH"),
                )
                .arg(
                    Arg::new("verbose")
                        .short('v')
                        .help("Be more verbose")
                        .action(ArgAction::Count),
                )
                .arg(
                    Arg::new("quiet")
                        .short('q')
                        .help("Be more quiet (-q -q gives silence)")
                        .action(ArgAction::Count),
                )
                .arg(
                    Arg::new("memory")
                        .short('m')
                        .help("Memory (in MB) to use")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("threads")
                        .short('t')
                        .help("Number of threads used for main processing")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("force_scalar")
                        .long("force-scalar")
                        .help("Force scalar code paths (disable SIMD optimizations)")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("file_threads")
                        .short('T')
                        .help("Number of files hashed in parallel")
                        .value_name("N"),
                )
                // Create-specific options (match par2cmdline exactly)
                .arg(
                    Arg::new("archive_name")
                        .short('a')
                        .help("Set the main PAR2 archive name")
                        .value_name("FILE"),
                )
                .arg(
                    Arg::new("block_count")
                        .short('b')
                        .help("Set the Block-Count")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("block_size")
                        .short('s')
                        .help("Set the Block-Size (don't use both -b and -s)")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("redundancy")
                        .short('r')
                        .help("Level of redundancy (%) or target size with g/m/k suffix")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("recovery_block_count")
                        .short('c')
                        .help("Recovery Block-Count (don't use both -r and -c)")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("first_recovery_block")
                        .short('f')
                        .help("First Recovery-Block-Number")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("uniform")
                        .short('u')
                        .help("Uniform recovery file sizes")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("limit_size")
                        .short('l')
                        .help("Limit size of recovery files (don't use both -u and -l)")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("recovery_file_count")
                        .short('n')
                        .help("Number of recovery files (max 31) (don't use both -n and -l)")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("recurse")
                        .short('R')
                        .help("Recurse into subdirectories")
                        .action(ArgAction::SetTrue),
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

fn handle_create(matches: &clap::ArgMatches) -> Result<()> {
    let par2_file = matches
        .get_one::<String>("par2_file")
        .expect("par2_file is required");

    let source_inputs: Vec<PathBuf> = matches
        .get_many::<String>("files")
        .expect("files are required")
        .map(PathBuf::from)
        .collect();

    // Handle verbosity/quiet flags (par2cmdline style)
    let _verbose_count = matches.get_count("verbose"); // TODO: Use for logging level
    let quiet_count = matches.get_count("quiet");
    let quiet_mode = quiet_count > 0;

    // Parse redundancy - handle percentage or size suffix (g/m/k)
    let redundancy = matches
        .get_one::<String>("redundancy")
        .map(|value| parse_redundancy_option(value).map_err(anyhow::Error::msg))
        .transpose()?;

    // Parse optional arguments (matching par2cmdline exactly)
    let block_size: Option<u64> = matches
        .get_one::<String>("block_size")
        .map(|s| s.parse())
        .transpose()
        .context("Invalid block size")?;

    let block_count: Option<u32> = matches
        .get_one::<String>("block_count")
        .map(|s| s.parse())
        .transpose()
        .context("Invalid block count")?;

    let recovery_block_count: Option<u32> = matches
        .get_one::<String>("recovery_block_count")
        .map(|s| s.parse())
        .transpose()
        .context("Invalid recovery block count")?;

    let recovery_file_count: Option<u32> = matches
        .get_one::<String>("recovery_file_count")
        .map(|s| s.parse())
        .transpose()
        .context("Invalid recovery file count")?;

    let first_recovery_block: Option<u32> = matches
        .get_one::<String>("first_recovery_block")
        .map(|s| s.parse())
        .transpose()
        .context("Invalid first recovery block number")?;

    let memory_mb: Option<usize> = matches
        .get_one::<String>("memory")
        .map(|s| s.parse())
        .transpose()
        .context("Invalid memory value")?;

    let base_path = matches.get_one::<String>("basepath").map(PathBuf::from);

    let threads: Option<u32> = matches
        .get_one::<String>("threads")
        .map(|s| s.parse())
        .transpose()
        .context("Invalid thread count")?;

    let uniform = matches.get_flag("uniform");
    let limit_size = matches.get_flag("limit_size");
    let recurse = matches.get_flag("recurse");
    let source_files =
        expand_source_files(source_inputs, recurse).context("Failed to expand source file list")?;

    // Use archive name if specified, otherwise use par2_file
    let output_name = matches
        .get_one::<String>("archive_name")
        .unwrap_or(par2_file);

    if !quiet_mode {
        println!(
            "Creating PAR2 files for {} source files...",
            source_files.len()
        );
        println!("Output: {}", output_name);
        if let Some(RedundancyOption::Percent(redundancy)) = redundancy {
            println!("Redundancy: {}%", redundancy);
        }
    }

    // Create PAR2 files using our implementation
    let reporter = Box::new(par2rs::create::ConsoleCreateReporter::new(quiet_mode));

    let mut context = par2rs::create::CreateContextBuilder::new()
        .output_name(output_name)
        .source_files(source_files)
        .reporter(reporter);

    if let Some(redundancy) = redundancy {
        context = match redundancy {
            RedundancyOption::Percent(percent) => context.redundancy_percentage(percent),
            RedundancyOption::TargetSize(bytes) => context.recovery_target_size(bytes),
        };
    }

    // Apply optional parameters
    if let Some(size) = block_size {
        context = context.block_size(size);
    }
    if let Some(count) = block_count {
        // Par2cmdline uses -b for source block count (target number of blocks)
        // This is used to calculate block_size if block_size is not specified
        context = context.source_block_count(count);
    }
    if let Some(count) = recovery_block_count {
        context = context.recovery_block_count(count);
    }
    if let Some(count) = recovery_file_count {
        context = context.recovery_file_count(count);
    }
    if let Some(exponent) = first_recovery_block {
        context = context.first_recovery_block(exponent);
    }
    if let Some(limit_mb) = memory_mb {
        context = context.memory_limit(limit_mb * 1024 * 1024);
    }
    if let Some(path) = base_path {
        context = context.base_path(path);
    }
    if uniform {
        context = context.recovery_file_scheme(par2rs::create::RecoveryFileScheme::Uniform);
    }
    if limit_size {
        context = context.recovery_file_scheme(par2rs::create::RecoveryFileScheme::Limited);
    }
    if let Some(thread_count) = threads {
        context = context.thread_count(thread_count);
    }

    // Initialize SIMD policy from CLI flag (disable SIMD if requested)
    let force_scalar = matches.get_flag("force_scalar");
    par2rs::reed_solomon::codec::init_simd_level(force_scalar);

    let mut create_context = context
        .build()
        .context("Failed to initialize PAR2 creation context")?;

    create_context
        .create()
        .context("Failed to create PAR2 files")?;

    if !quiet_mode {
        println!("\nCreated PAR2 files:");
        for file in create_context.output_files() {
            println!("  {}", file);
        }
        println!("\nDone.");
    }

    Ok(())
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

    // Parse packets excluding recovery slices but validate and count them
    // Recovery slice data is NOT loaded into memory (saves gigabytes for large PAR2 sets)
    // but they are validated and counted for repair possibility checking
    let packet_set = par2rs::par2_files::load_par2_packets(&par2_files, false, !quiet);

    if !quiet {
        println!(); // Blank line after loading

        // Show summary statistics
        let stats = par2rs::analysis::calculate_par2_stats(
            &packet_set.packets,
            packet_set.recovery_block_count,
        );
        par2rs::analysis::print_summary_stats(&stats);

        println!("\nVerifying source files:\n");
    }

    let base_dir = packet_set.base_dir.clone();
    let reporter = par2rs::reporters::ConsoleVerificationReporter::new();

    // Perform comprehensive verification
    let results = if quiet {
        let silent = par2rs::reporters::SilentVerificationReporter;
        par2rs::verify::comprehensive_verify_files(packet_set, &verify_config, &silent, base_dir)
    } else {
        par2rs::verify::comprehensive_verify_files(packet_set, &verify_config, &reporter, base_dir)
    };

    if !quiet {
        reporter.report_verification_results(&results);
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

    let (context, result) = par2rs::repair::repair_files(
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
