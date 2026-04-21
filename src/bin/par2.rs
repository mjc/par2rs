//! Main par2 binary - drop-in replacement for par2cmdline
//!
//! Supports the same command-line interface as par2cmdline for compatibility

use anyhow::{Context, Result};
use clap::{Arg, ArgAction, Command};
use par2rs::cli::compat::{
    init_env_logger, normalize_mixed_noise_option_clusters, parse_memory_mb, parse_noise_level,
    parse_positive_usize, reject_invalid_create_short_clusters, reject_short_value_forms,
};
use par2rs::create::cli::{
    parse_redundancy_option, resolve_create_inputs, validate_recovery_file_count,
    warn_for_high_redundancy, RedundancyOption,
};
use par2rs::reporters::VerificationReporter;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("-VV")) {
        par2rs::print_long_version();
        return Ok(());
    }

    reject_detached_short_values_for_subcommand();
    let args = normalize_mixed_noise_option_clusters(std::env::args_os());

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
                        .num_args(0..)
                        .index(2),
                )
                // Global options (match par2cmdline)
                .arg(
                    Arg::new("basepath")
                        .short('B')
                        .long("basepath")
                        .help("Set the basepath to use as reference for the datafiles")
                        .value_name("PATH"),
                )
                .arg(
                    Arg::new("verbose")
                        .short('v')
                        .long("verbose")
                        .help("Be more verbose")
                        .action(ArgAction::Count),
                )
                .arg(
                    Arg::new("quiet")
                        .short('q')
                        .long("quiet")
                        .help("Be more quiet (-q -q gives silence)")
                        .action(ArgAction::Count),
                )
                .arg(
                    Arg::new("memory")
                        .short('m')
                        .long("memory")
                        .help("Memory (in MB) to use")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("threads")
                        .short('t')
                        .long("threads")
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
                        .long("file-threads")
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
                        .conflicts_with("block_size")
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
                        .conflicts_with("recovery_block_count")
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
                        .conflicts_with("limit_size")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("limit_size")
                        .short('l')
                        .help("Limit size of recovery files (don't use both -u and -l)")
                        .conflicts_with("recovery_file_count")
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
                        .help("Extra files to scan (optional)")
                        .num_args(0..)
                        .index(2),
                )
                .arg(
                    Arg::new("basepath")
                        .short('B')
                        .long("basepath")
                        .help("Set the basepath to use as reference for the datafiles")
                        .value_name("PATH"),
                )
                .arg(
                    Arg::new("archive_name")
                        .short('a')
                        .help("Accepted for par2cmdline compatibility")
                        .value_name("FILE"),
                )
                .arg(
                    Arg::new("verbose")
                        .short('v')
                        .long("verbose")
                        .help("Be more verbose")
                        .action(ArgAction::Count),
                )
                .arg(
                    Arg::new("quiet")
                        .short('q')
                        .long("quiet")
                        .help("Be more quiet (-q -q gives silence)")
                        .action(ArgAction::Count),
                )
                .arg(
                    Arg::new("purge")
                        .short('p')
                        .long("purge")
                        .help("Purge backup files and par files when no recovery is needed")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("rename_only")
                        .short('O')
                        .help("Rename-only mode")
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
                    Arg::new("memory")
                        .short('m')
                        .long("memory")
                        .help("Memory (in MB) to use")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("file_threads")
                        .short('T')
                        .long("file-threads")
                        .help("Number of files hashed in parallel")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("no-parallel")
                        .long("no-parallel")
                        .help("Disable all parallel processing")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("data_skipping")
                        .short('N')
                        .help("Data skipping (find badly mispositioned data blocks)")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("skip_leeway")
                        .short('S')
                        .help("Skip leeway (distance +/- from expected block position)")
                        .value_name("N")
                        .requires("data_skipping"),
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
                        .help("Extra files to scan (optional)")
                        .num_args(0..)
                        .index(2),
                )
                .arg(
                    Arg::new("basepath")
                        .short('B')
                        .long("basepath")
                        .help("Set the basepath to use as reference for the datafiles")
                        .value_name("PATH"),
                )
                .arg(
                    Arg::new("archive_name")
                        .short('a')
                        .help("Accepted for par2cmdline compatibility")
                        .value_name("FILE"),
                )
                .arg(
                    Arg::new("verbose")
                        .short('v')
                        .long("verbose")
                        .help("Be more verbose")
                        .action(ArgAction::Count),
                )
                .arg(
                    Arg::new("quiet")
                        .short('q')
                        .long("quiet")
                        .help("Be more quiet (-q -q gives silence)")
                        .action(ArgAction::Count),
                )
                .arg(
                    Arg::new("purge")
                        .short('p')
                        .long("purge")
                        .help("Purge backup files after successful repair")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("rename_only")
                        .short('O')
                        .help("Rename-only mode")
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
                    Arg::new("memory")
                        .short('m')
                        .long("memory")
                        .help("Memory (in MB) to use")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("file_threads")
                        .short('T')
                        .long("file-threads")
                        .help("Number of files hashed in parallel")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("no-parallel")
                        .long("no-parallel")
                        .help("Disable all parallel processing")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("data_skipping")
                        .short('N')
                        .help("Data skipping (find badly mispositioned data blocks)")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("skip_leeway")
                        .short('S')
                        .help("Skip leeway (distance +/- from expected block position)")
                        .value_name("N")
                        .requires("data_skipping"),
                ),
        )
        .get_matches_from(args);

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

fn reject_detached_short_values_for_subcommand() {
    let args: Vec<_> = std::env::args_os().collect();
    let Some(command) = args.get(1).and_then(|arg| arg.to_str()) else {
        return;
    };

    let (detached_rejected, equals_rejected) = match command {
        "create" | "c" => (
            &["-b", "-s", "-r", "-n", "-T", "-t", "-m"][..],
            &["-B", "-b", "-s", "-r", "-c", "-f", "-n", "-T", "-t", "-m"][..],
        ),
        "verify" | "v" | "repair" | "r" => {
            (&["-a", "-S", "-T", "-m"][..], &["-B", "-S", "-T", "-m"][..])
        }
        _ => return,
    };

    if let Err(message) = reject_short_value_forms(
        args.iter().skip(2).cloned(),
        detached_rejected,
        equals_rejected,
    ) {
        eprintln!("{message}");
        std::process::exit(2);
    }

    if matches!(command, "create" | "c") {
        if let Err(message) = reject_invalid_create_short_clusters(args.iter().skip(2).cloned()) {
            eprintln!("{message}");
            std::process::exit(2);
        }
    }
}

fn handle_create(matches: &clap::ArgMatches) -> Result<()> {
    let par2_file = matches
        .get_one::<String>("par2_file")
        .expect("par2_file is required");

    let source_inputs: Vec<PathBuf> = matches
        .get_many::<String>("files")
        .map(|files| files.map(PathBuf::from).collect())
        .unwrap_or_default();

    // Handle verbosity/quiet flags (par2cmdline style)
    let noise_level = parse_noise_level(matches.get_count("verbose"), matches.get_count("quiet"))
        .map_err(anyhow::Error::msg)?;
    init_env_logger(noise_level);
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

    let memory_limit = parse_memory_mb(matches.get_one::<String>("memory").map(String::as_str))
        .map_err(anyhow::Error::msg)?;
    let file_threads = parse_positive_usize(
        matches
            .get_one::<String>("file_threads")
            .map(String::as_str),
        "-T",
    )
    .map_err(anyhow::Error::msg)?;

    let base_path = matches.get_one::<String>("basepath").map(PathBuf::from);

    let threads: Option<u32> = matches
        .get_one::<String>("threads")
        .map(|s| s.parse())
        .transpose()
        .context("Invalid thread count")?;

    let uniform = matches.get_flag("uniform");
    let limit_size = matches.get_flag("limit_size");
    let recurse = matches.get_flag("recurse");

    if let Some(count) = recovery_file_count {
        validate_recovery_file_count(count).map_err(anyhow::Error::msg)?;
    }

    warn_for_high_redundancy(redundancy);

    let (output_name, source_files) = resolve_create_inputs(
        par2_file,
        matches
            .get_one::<String>("archive_name")
            .map(String::as_str),
        source_inputs,
        recurse,
    )
    .map_err(anyhow::Error::msg)?;
    reject_par1_create_target(&output_name)?;

    if !quiet_mode {
        println!(
            "Creating PAR2 files for {} source files...",
            source_files.len()
        );
        println!("Output: {output_name}");
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
        if !uniform && !limit_size {
            context = context.recovery_file_scheme(par2rs::create::RecoveryFileScheme::Uniform);
        }
    }
    if let Some(exponent) = first_recovery_block {
        context = context.first_recovery_block(exponent);
    }
    if let Some(limit) = memory_limit {
        context = context.memory_limit(limit);
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
    if let Some(file_threads) = file_threads {
        context = context.file_thread_count(file_threads);
    }

    // Initialize SIMD policy from CLI flag (disable SIMD if requested)
    let force_scalar = matches.get_flag("force_scalar");
    par2rs::reed_solomon::codec::init_simd_level(force_scalar);

    let mut create_context = context
        .build()
        .context("Failed to initialize PAR2 creation context")?;

    if let Err(error) = create_context.create() {
        if let Some(exit_code) = create_error_exit_code(&error) {
            eprintln!("Error: Failed to create PAR2 files\n\nCaused by:\n    0: {error}");
            std::process::exit(exit_code);
        }
        return Err(error).context("Failed to create PAR2 files");
    }

    if !quiet_mode {
        println!("\nCreated PAR2 files:");
        for file in create_context.output_files() {
            println!("  {}", file);
        }
        println!("\nDone.");
    }

    Ok(())
}

fn create_error_exit_code(error: &par2rs::create::CreateError) -> Option<i32> {
    match error {
        par2rs::create::CreateError::FileCreateError { source, .. }
            if source.kind() == std::io::ErrorKind::AlreadyExists =>
        {
            Some(3)
        }
        _ => None,
    }
}

fn reject_par1_create_target(output_name: &str) -> Result<()> {
    if par2rs::par2_files::detect_recovery_format(Path::new(output_name))
        == Some(par2rs::par2_files::RecoveryFormat::Par1)
    {
        anyhow::bail!("PAR1 create is not supported");
    }
    Ok(())
}

fn handle_verify(matches: &clap::ArgMatches) -> Result<()> {
    use std::path::{Path, PathBuf};

    let noise_level = parse_noise_level(matches.get_count("verbose"), matches.get_count("quiet"))
        .map_err(anyhow::Error::msg)?;
    init_env_logger(noise_level);

    let par2_file = matches
        .get_one::<String>("par2_file")
        .expect("par2_file is required");
    let quiet = matches.get_count("quiet") > 0;
    let purge = matches.get_flag("purge");
    let base_path_override = matches
        .get_one::<String>("basepath")
        .map(|path| std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path)));
    let extra_files: Vec<PathBuf> = matches
        .get_many::<String>("files")
        .map(|files| {
            files
                .map(|path| std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path)))
                .collect()
        })
        .unwrap_or_default();

    if par2rs::par2_files::detect_recovery_format(Path::new(par2_file))
        == Some(par2rs::par2_files::RecoveryFormat::Par1)
    {
        let options = par2rs::par1::verify::Par1VerifyOptions { extra_files, purge };
        let results =
            par2rs::par1::verify::verify_par1_file_with_options(Path::new(par2_file), &options)
                .context("Failed to verify PAR1 file")?;
        let reporter = par2rs::reporters::ConsoleVerificationReporter::new();
        if !quiet {
            reporter.report_verification_results(&results);
        }
        if results.renamed_file_count == 0
            && results.missing_file_count == 0
            && results.corrupted_file_count == 0
        {
            return Ok(());
        }
        anyhow::bail!(
            "Repair required: {} files are missing or damaged",
            results.renamed_file_count + results.missing_file_count + results.corrupted_file_count
        );
    }

    // Create verification config from command line arguments
    let verify_config =
        par2rs::verify::VerificationConfig::try_from_args(matches).map_err(anyhow::Error::msg)?;

    // Initialize Rayon thread pool BEFORE any parallel operations
    // This must be done before any par_iter() calls
    let thread_count = verify_config.effective_threads();
    rayon::ThreadPoolBuilder::new()
        .num_threads(thread_count)
        .build_global()
        .ok(); // Ignore error if already initialized

    let file_path = par2rs::par2_files::resolve_par2_file_argument(Path::new(par2_file))
        .with_context(|| format!("Failed to locate PAR2 file for {}", par2_file))?;

    // Change to parent directory for file resolution (like par2verify does)
    if let Some(parent) = file_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
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
    let mut par2_files = par2_files;
    par2_files.extend(
        verify_config
            .extra_files
            .iter()
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("par2"))
            })
            .cloned(),
    );
    par2_files.sort();
    par2_files.dedup();

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

    let base_dir = base_path_override.unwrap_or_else(|| packet_set.base_dir.clone());
    let reporter = par2rs::reporters::ConsoleVerificationReporter::new();

    // Perform comprehensive verification
    let results = if quiet {
        let silent = par2rs::reporters::SilentVerificationReporter;
        par2rs::verify::comprehensive_verify_files_with_extra_files(
            packet_set,
            &verify_config,
            &silent,
            &base_dir,
            &extra_files,
        )
    } else {
        par2rs::verify::comprehensive_verify_files_with_extra_files(
            packet_set,
            &verify_config,
            &reporter,
            &base_dir,
            &extra_files,
        )
    };

    if !quiet {
        reporter.report_verification_results(&results);
    }

    if results.renamed_file_count > 0 {
        anyhow::bail!(
            "Repair required: {} files are renamed",
            results.renamed_file_count
        );
    }

    if results.missing_block_count == 0 {
        if purge {
            par2rs::repair::RepairContext::purge_par_files_for(&file_name.to_string_lossy())?;
        }
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
    let noise_level = parse_noise_level(matches.get_count("verbose"), matches.get_count("quiet"))
        .map_err(anyhow::Error::msg)?;
    init_env_logger(noise_level);

    let par2_file = matches
        .get_one::<String>("par2_file")
        .expect("par2_file is required");
    let quiet = matches.get_count("quiet") > 0;
    let purge = matches.get_flag("purge");
    let base_path_override = matches.get_one::<String>("basepath").map(PathBuf::from);
    let extra_files: Vec<PathBuf> = matches
        .get_many::<String>("files")
        .map(|files| {
            files
                .map(|path| std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path)))
                .collect()
        })
        .unwrap_or_default();

    if par2rs::par2_files::detect_recovery_format(Path::new(par2_file))
        == Some(par2rs::par2_files::RecoveryFormat::Par1)
    {
        let memory_limit = parse_memory_mb(matches.get_one::<String>("memory").map(String::as_str))
            .map_err(anyhow::Error::msg)?;
        let options = par2rs::par1::repair::Par1RepairOptions {
            memory_limit,
            extra_files,
            purge,
        };
        let results =
            par2rs::par1::repair::repair_par1_file_with_options(Path::new(par2_file), &options)
                .context("Failed to repair PAR1 files")?;
        if !quiet {
            let reporter = par2rs::reporters::ConsoleVerificationReporter::new();
            reporter.report_verification_results(&results);
        }
        anyhow::ensure!(
            results.renamed_file_count == 0
                && results.missing_file_count == 0
                && results.corrupted_file_count == 0,
            "PAR1 repair failed"
        );
        return Ok(());
    }

    // Create verification config from command line arguments (like par2repair does)
    let verify_config =
        par2rs::verify::VerificationConfig::try_from_args(matches).map_err(anyhow::Error::msg)?;

    let resolved_par2_file =
        par2rs::par2_files::resolve_par2_file_argument(Path::new(par2_file))
            .with_context(|| format!("Failed to locate PAR2 file for {}", par2_file))?;
    let resolved_par2_file = resolved_par2_file.to_string_lossy().into_owned();

    let (context, result) = par2rs::repair::repair_files_with_base_path_and_extra_files(
        &resolved_par2_file,
        Box::new(par2rs::repair::ConsoleReporter::new(quiet)),
        &verify_config,
        base_path_override.as_deref(),
        &extra_files,
    )
    .context("Failed to repair files")?;

    if !quiet {
        context.recovery_set.print_statistics();
        result.print_result();
    }

    if purge {
        match &result {
            par2rs::repair::RepairResult::Success { .. } => {
                context.purge_files(&resolved_par2_file)?
            }
            par2rs::repair::RepairResult::NoRepairNeeded { .. } => {
                context.purge_par_files(&resolved_par2_file)?
            }
            par2rs::repair::RepairResult::Failed { .. } => {}
        }
    }

    if result.is_success() {
        Ok(())
    } else {
        std::process::exit(2);
    }
}
