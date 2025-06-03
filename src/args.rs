use clap::{Arg, Command};

pub fn parse_args() -> clap::ArgMatches {
    Command::new("par2rs")
        .version("1.0")
        .author("Your Name <your.email@example.com>")
        .about("A Rust implementation of par2repair")
        .arg(
            Arg::new("input")
                .help("Input file")
                .required(true)
                .value_parser(clap::value_parser!(String)),
        )
        .arg(
            Arg::new("output")
                .help("Output file")
                .required(false)
                .value_parser(clap::value_parser!(String)),
        )
        .get_matches()
}
