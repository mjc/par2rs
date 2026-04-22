use anyhow::{Context, Result};

use par2rs::args::parse_repair_args;
use par2rs::cli::compat::{init_env_logger, parse_memory_mb, parse_noise_level};
use par2rs::reporters::VerificationReporter;
use par2rs::verify::VerificationConfig;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("-VV")) {
        par2rs::print_long_version();
        return Ok(());
    }

    let matches = parse_repair_args();
    let noise_level = parse_noise_level(matches.get_count("verbose"), matches.get_count("quiet"))
        .map_err(anyhow::Error::msg)?;
    init_env_logger(noise_level);

    let par2_file = matches
        .get_one::<String>("par2_file")
        .expect("par2_file is required by clap");
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
        anyhow::ensure!(!purge, "PAR1 purge is not supported");
        let memory_limit = parse_memory_mb(matches.get_one::<String>("memory").map(String::as_str))
            .map_err(anyhow::Error::msg)?;
        let results = par2rs::par1::repair::repair_par1_file_with_memory_limit(
            Path::new(par2_file),
            memory_limit,
        )
        .context("Failed to repair PAR1 files")?;
        if !quiet {
            let reporter = par2rs::reporters::ConsoleVerificationReporter::new();
            reporter.report_verification_results(&results);
        }
        anyhow::ensure!(
            results.missing_file_count == 0 && results.corrupted_file_count == 0,
            "PAR1 repair failed"
        );
        return Ok(());
    }

    // Create verification config from command line arguments
    let verify_config = VerificationConfig::try_from_args(&matches).map_err(anyhow::Error::msg)?;

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

    // Print output unless quiet mode is enabled
    if !quiet {
        context.recovery_set.print_statistics();
        result.print_result();
    }

    // Purge backup and PAR2 files on successful repair if -p flag is set
    if purge && result.is_success() {
        context.purge_files(&resolved_par2_file)?;
    }

    // Exit with success if repair was successful or not needed, error otherwise
    if result.is_success() {
        Ok(())
    } else {
        std::process::exit(2);
    }
}
