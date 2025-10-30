use anyhow::{Context, Result};

use par2rs::args::parse_repair_args;
use par2rs::verify::VerificationConfig;

fn main() -> Result<()> {
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
    let quiet = matches.get_flag("quiet");
    let purge = matches.get_flag("purge");

    // Create verification config from command line arguments
    let verify_config = VerificationConfig::from_args(&matches);

    let (context, result) = par2rs::repair::repair_files_with_config(
        par2_file,
        Box::new(par2rs::repair::ConsoleReporter::new(quiet)),
        &verify_config,
    )
    .context("Failed to repair files")?;

    // Print output unless quiet mode is enabled
    if !quiet {
        context.recovery_set.print_statistics();
        result.print_result();
    }

    // Purge backup and PAR2 files on successful repair if -p flag is set
    if purge && result.is_success() {
        context.purge_files(par2_file)?;
    }

    // Exit with success if repair was successful or not needed, error otherwise
    if result.is_success() {
        Ok(())
    } else {
        anyhow::bail!("Repair failed");
    }
}
