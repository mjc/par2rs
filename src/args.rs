use clap::{Arg, ArgAction, Command};

pub fn parse_args() -> clap::ArgMatches {
    reject_detached_verify_repair_short_values();
    let args = crate::cli::compat::normalize_mixed_noise_option_clusters(std::env::args_os());

    Command::new("par2verify")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Mika Cohen <mjc@kernel.org>")
        .about("Verify files using PAR2 data")
        .arg(Arg::new("input").help("Input file").required(true))
        .arg(
            Arg::new("files")
                .help("Extra files to scan (optional)")
                .num_args(0..)
                .index(2),
        )
        .arg(
            Arg::new("basepath")
                .help("Set the basepath to use as reference for the datafiles")
                .short('B')
                .long("basepath")
                .value_name("PATH"),
        )
        .arg(
            Arg::new("archive_name")
                .help("Accepted for par2cmdline compatibility")
                .short('a')
                .value_name("FILE"),
        )
        .arg(
            Arg::new("verbose")
                .help("Be more verbose")
                .short('v')
                .long("verbose")
                .action(ArgAction::Count),
        )
        .arg(
            Arg::new("quiet")
                .help("Be more quiet (-q -q gives silence)")
                .short('q')
                .long("quiet")
                .action(ArgAction::Count),
        )
        .arg(
            Arg::new("purge")
                .help("Purge backup files and par files when no recovery is needed")
                .short('p')
                .long("purge")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("rename_only")
                .help("Rename-only mode")
                .short('O')
                .action(ArgAction::SetTrue),
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
            Arg::new("memory")
                .help("Memory (in MB) to use")
                .short('m')
                .long("memory")
                .value_name("N"),
        )
        .arg(
            Arg::new("file_threads")
                .help("Number of files hashed in parallel")
                .short('T')
                .long("file-threads")
                .value_name("N"),
        )
        .arg(
            Arg::new("no-parallel")
                .help("Disable all parallel processing")
                .long("no-parallel")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("data_skipping")
                .help("Data skipping (find badly mispositioned data blocks)")
                .short('N')
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("skip_leeway")
                .help("Skip leeway (distance +/- from expected block position)")
                .short('S')
                .value_name("N"),
        )
        .get_matches_from(args)
}

pub fn parse_repair_args() -> clap::ArgMatches {
    reject_detached_verify_repair_short_values();
    let args = crate::cli::compat::normalize_mixed_noise_option_clusters(std::env::args_os());

    Command::new("par2repair")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Mika Cohen <mjc@kernel.org>")
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
                .help("Set the basepath to use as reference for the datafiles")
                .short('B')
                .long("basepath")
                .value_name("PATH"),
        )
        .arg(
            Arg::new("archive_name")
                .help("Accepted for par2cmdline compatibility")
                .short('a')
                .value_name("FILE"),
        )
        .arg(
            Arg::new("verbose")
                .help("Be more verbose")
                .short('v')
                .long("verbose")
                .action(ArgAction::Count),
        )
        .arg(
            Arg::new("quiet")
                .help("Be more quiet (-q -q gives silence)")
                .short('q')
                .long("quiet")
                .action(ArgAction::Count),
        )
        .arg(
            Arg::new("purge")
                .help("Purge backup files and par files on successful recovery")
                .short('p')
                .long("purge")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("rename_only")
                .help("Rename-only mode")
                .short('O')
                .action(ArgAction::SetTrue),
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
            Arg::new("memory")
                .help("Memory (in MB) to use")
                .short('m')
                .long("memory")
                .value_name("N"),
        )
        .arg(
            Arg::new("file_threads")
                .help("Number of files hashed in parallel")
                .short('T')
                .long("file-threads")
                .value_name("N"),
        )
        .arg(
            Arg::new("no-parallel")
                .help("Disable all parallel processing")
                .long("no-parallel")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("data_skipping")
                .help("Data skipping (find badly mispositioned data blocks)")
                .short('N')
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("skip_leeway")
                .help("Skip leeway (distance +/- from expected block position)")
                .short('S')
                .value_name("N"),
        )
        .get_matches_from(args)
}

fn reject_detached_verify_repair_short_values() {
    if let Err(message) = crate::cli::compat::reject_short_value_forms(
        std::env::args_os().skip(1),
        &["-a", "-S", "-T", "-m"],
        &["-B", "-S", "-T", "-m"],
    ) {
        eprintln!("{message}");
        std::process::exit(2);
    }
}
