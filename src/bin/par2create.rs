//! Standalone par2create-compatible entry point.

use anyhow::{Context, Result};
use clap::{Arg, ArgAction, Command};
use par2rs::cli::compat::{
    init_env_logger, parse_memory_mb, parse_noise_level, parse_positive_usize,
};
use par2rs::create::cli::{
    parse_redundancy_option, resolve_create_inputs, validate_recovery_file_count,
    warn_for_high_redundancy, RedundancyOption,
};
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("-VV")) {
        par2rs::print_long_version();
        return Ok(());
    }

    let matches = Command::new("par2create")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Create PAR2 recovery files")
        .arg_required_else_help(true)
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
        )
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
        .get_matches();

    let noise_level = parse_noise_level(matches.get_count("verbose"), matches.get_count("quiet"))
        .map_err(anyhow::Error::msg)?;
    init_env_logger(noise_level);

    let par2_file = matches
        .get_one::<String>("par2_file")
        .expect("par2_file is required");
    let source_inputs: Vec<PathBuf> = matches
        .get_many::<String>("files")
        .map(|files| files.map(PathBuf::from).collect())
        .unwrap_or_default();

    let quiet_mode = matches.get_count("quiet") > 0;
    let redundancy = matches
        .get_one::<String>("redundancy")
        .map(|value| parse_redundancy_option(value).map_err(anyhow::Error::msg))
        .transpose()?;

    let block_size: Option<u64> = parse_optional_u64(&matches, "block_size")?;
    let block_count: Option<u32> = parse_optional_u32(&matches, "block_count")?;
    let recovery_block_count: Option<u32> = parse_optional_u32(&matches, "recovery_block_count")?;
    let recovery_file_count: Option<u32> = parse_optional_u32(&matches, "recovery_file_count")?;
    let first_recovery_block: Option<u32> = parse_optional_u32(&matches, "first_recovery_block")?;
    let memory_limit = parse_memory_mb(matches.get_one::<String>("memory").map(String::as_str))
        .map_err(anyhow::Error::msg)?;
    let file_threads = parse_positive_usize(
        matches
            .get_one::<String>("file_threads")
            .map(String::as_str),
        "-T",
    )
    .map_err(anyhow::Error::msg)?;
    let threads: Option<u32> = parse_optional_u32(&matches, "threads")?;

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
        matches.get_flag("recurse"),
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
    if let Some(size) = block_size {
        context = context.block_size(size);
    }
    if let Some(count) = block_count {
        context = context.source_block_count(count);
    }
    if let Some(count) = recovery_block_count {
        context = context.recovery_block_count(count);
    }
    if let Some(count) = recovery_file_count {
        context = context.recovery_file_count(count);
        if !matches.get_flag("uniform") && !matches.get_flag("limit_size") {
            context = context.recovery_file_scheme(par2rs::create::RecoveryFileScheme::Uniform);
        }
    }
    if let Some(exponent) = first_recovery_block {
        context = context.first_recovery_block(exponent);
    }
    if let Some(limit) = memory_limit {
        context = context.memory_limit(limit);
    }
    if let Some(thread_count) = threads {
        context = context.thread_count(thread_count);
    }
    if let Some(file_threads) = file_threads {
        context = context.file_thread_count(file_threads);
    }
    if matches.get_flag("uniform") {
        context = context.recovery_file_scheme(par2rs::create::RecoveryFileScheme::Uniform);
    }
    if matches.get_flag("limit_size") {
        context = context.recovery_file_scheme(par2rs::create::RecoveryFileScheme::Limited);
    }
    if let Some(base_path) = matches.get_one::<String>("basepath") {
        context = context.base_path(PathBuf::from(base_path));
    }

    par2rs::reed_solomon::codec::init_simd_level(matches.get_flag("force_scalar"));

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

fn parse_optional_u32(matches: &clap::ArgMatches, id: &str) -> Result<Option<u32>> {
    matches
        .get_one::<String>(id)
        .map(|s| s.parse())
        .transpose()
        .with_context(|| format!("Invalid {id} value"))
}

fn parse_optional_u64(matches: &clap::ArgMatches, id: &str) -> Result<Option<u64>> {
    matches
        .get_one::<String>(id)
        .map(|s| s.parse())
        .transpose()
        .with_context(|| format!("Invalid {id} value"))
}
