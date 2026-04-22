use anyhow::{Context, Result};

use par2rs::args::parse_repair_args;
use par2rs::verify::VerificationConfig;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("-VV")) {
        par2rs::print_long_version();
        return Ok(());
    }

    // Initialize the logger
    env_logger::Builder::from_default_env()
        .format_timestamp(None)
        .format_module_path(false)
        .format_target(false)
        .init();

    let matches = parse_repair_args();

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

    // Create verification config from command line arguments
    let verify_config = VerificationConfig::from_args(&matches);

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
        anyhow::bail!("Repair failed");
    }
}
