//! PAR2 verification tool
//!
//! This tool verifies the integrity of files using PAR2 (Parity Archive) files.
//! It loads PAR2 packets from the main and volume files, displays statistics,
//! and verifies that the protected files are intact.
//!
//! This implementation follows the par2cmdline approach:
//! - Performs whole-file verification using MD5 hashes
//! - For damaged files, performs block-level verification
//! - Reports which blocks are broken and calculates repair requirements
//! - Determines if repair is possible with available recovery blocks

use anyhow::{Context, Result};
use par2rs::cli::compat::{init_env_logger, parse_noise_level};
use par2rs::{analysis, par2_files, reporters::VerificationReporter, verify};
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("-VV")) {
        par2rs::print_long_version();
        return Ok(());
    }

    let matches = par2rs::parse_args();
    let noise_level = parse_noise_level(matches.get_count("verbose"), matches.get_count("quiet"))
        .map_err(anyhow::Error::msg)?;
    init_env_logger(noise_level);

    let input_file = matches
        .get_one::<String>("input")
        .expect("input is required by clap");
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

    if par2_files::detect_recovery_format(Path::new(input_file))
        == Some(par2_files::RecoveryFormat::Par1)
    {
        let options = par2rs::par1::verify::Par1VerifyOptions { extra_files, purge };
        let verification_results =
            par2rs::par1::verify::verify_par1_file_with_options(Path::new(input_file), &options)
                .context("Failed to verify PAR1 file")?;
        let reporter = par2rs::reporters::ConsoleVerificationReporter::new();
        if !quiet {
            reporter.report_verification_results(&verification_results);
        }
        anyhow::ensure!(
            verification_results.renamed_file_count == 0
                && verification_results.missing_file_count == 0
                && verification_results.corrupted_file_count == 0,
            "Repair required: {} files are missing or damaged",
            verification_results.renamed_file_count
                + verification_results.missing_file_count
                + verification_results.corrupted_file_count
        );
        return Ok(());
    }

    // Create verification config from command line arguments
    let verify_config =
        verify::VerificationConfig::try_from_args(&matches).map_err(anyhow::Error::msg)?;

    let file_path = par2_files::resolve_par2_file_argument(Path::new(input_file))
        .with_context(|| format!("Failed to locate PAR2 file for {}", input_file))?;

    // Change to parent directory for file resolution
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
        .and_then(|name| name.to_str())
        .map(Path::new)
        .unwrap_or(&file_path);
    let par2_files = par2_files::collect_par2_files(file_name);

    // Parse packets excluding recovery slices but validate and count them
    if !quiet {
        println!("Loading PAR2 files...\n");
    }
    let packet_set = par2_files::load_par2_packets(&par2_files, false, !quiet);

    if !quiet {
        println!(); // Blank line after loading

        // Show summary statistics
        let stats =
            analysis::calculate_par2_stats(&packet_set.packets, packet_set.recovery_block_count);
        analysis::print_summary_stats(&stats);
    }

    let base_dir = base_path_override.unwrap_or_else(|| packet_set.base_dir.clone());

    // Perform comprehensive verification with configuration
    let reporter = par2rs::reporters::ConsoleVerificationReporter::new();
    let verification_results = if quiet {
        let silent = par2rs::reporters::SilentVerificationReporter;
        verify::comprehensive_verify_files_with_extra_files(
            packet_set,
            &verify_config,
            &silent,
            &base_dir,
            &extra_files,
        )
    } else {
        println!("\nVerifying source files:\n");
        verify::comprehensive_verify_files_with_extra_files(
            packet_set,
            &verify_config,
            &reporter,
            &base_dir,
            &extra_files,
        )
    };

    // Print detailed results
    if !quiet {
        reporter.report_verification_results(&verification_results);
    }

    // Return success if no repair is needed, error if repair is required
    anyhow::ensure!(
        verification_results.missing_block_count == 0,
        "Repair required: {} blocks are missing or damaged",
        verification_results.missing_block_count
    );

    if purge {
        let packet_set = par2_files::load_par2_packets(&par2_files, false, false);
        let context = par2rs::repair::RepairContextBuilder::new()
            .packets(packet_set.packets)
            .base_path(base_dir)
            .reporter(Box::new(par2rs::repair::ConsoleReporter::new(quiet)))
            .build()
            .context("Failed to initialize purge context")?;
        context.purge_files(&file_name.to_string_lossy())?;
    }

    Ok(())
}
