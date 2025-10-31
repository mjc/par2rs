use clap::{Arg, Command};
use std::fs;
use std::path::Path;

pub fn parse_args() -> clap::ArgMatches {
    Command::new("par2rs")
        .version("0.1.0")
        .author("Mika Cohen <mjc@kernel.org>")
        .about("A Rust implementation of par2repair")
        .arg(
            Arg::new("input")
                .help("Input file")
                .required(true)
                .value_parser(|input: &str| {
                    let path =
                        fs::canonicalize(input).map_err(|_| "Failed to resolve input path")?;
                    if path.exists() {
                        Ok(path.to_string_lossy().to_string())
                    } else {
                        Err(String::from("Input file does not exist"))
                    }
                }),
        )
        .arg(
            Arg::new("output")
                .help("Output file")
                .required(false)
                .value_parser(|output: &str| {
                    let path =
                        fs::canonicalize(output).map_err(|_| "Failed to resolve output path")?;
                    if path.parent().is_none_or(|parent| parent.exists()) {
                        Ok(path.to_string_lossy().to_string())
                    } else {
                        Err(String::from("Output directory does not exist"))
                    }
                }),
        )
        .arg(
            Arg::new("threads")
                .help("Number of CPU threads for computation (0 = auto-detect)")
                .short('t')
                .long("threads")
                .value_name("N")
                .default_value("0"),
        )
        .arg(
            Arg::new("no-parallel")
                .help("Disable all parallel processing")
                .long("no-parallel")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches()
}

pub fn parse_repair_args() -> clap::ArgMatches {
    Command::new("par2repair")
        .version("0.1.0")
        .author("Mika Cohen <mjc@kernel.org>")
        .about("A Rust implementation of par2 repair")
        .arg(
            Arg::new("par2_file")
                .help("PAR2 file to use for repair")
                .required(true)
                .index(1)
                .value_parser(|input: &str| {
                    let path = Path::new(input);
                    if path.exists() {
                        Ok(input.to_string())
                    } else {
                        Err(format!("PAR2 file '{}' does not exist", input))
                    }
                }),
        )
        .arg(
            Arg::new("files")
                .help("Target files to repair (optional)")
                .num_args(0..)
                .index(2),
        )
        .arg(
            Arg::new("verbose")
                .help("Verbose output")
                .short('v')
                .long("verbose")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("quiet")
                .help("Quiet output (errors only)")
                .short('q')
                .long("quiet")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("purge")
                .help("Purge backup files and par files on successful recovery")
                .short('p')
                .long("purge")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("threads")
                .help("Number of CPU threads for computation (0 = auto-detect)")
                .short('t')
                .long("threads")
                .value_name("N")
                .default_value("0"),
        )
        .arg(
            Arg::new("no-parallel")
                .help("Disable all parallel processing")
                .long("no-parallel")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches()
}
