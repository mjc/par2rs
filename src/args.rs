use clap::{Arg, Command};
use std::fs;

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
                    if path.parent().map_or(true, |parent| parent.exists()) {
                        Ok(path.to_string_lossy().to_string())
                    } else {
                        Err(String::from("Output directory does not exist"))
                    }
                }),
        )
        .get_matches()
}
