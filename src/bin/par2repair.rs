use std::process;

use par2rs::args::parse_repair_args;
use par2rs::repair::repair_files;

fn main() {
    // Initialize the logger
    env_logger::Builder::from_default_env()
        .format_timestamp(None)
        .format_module_path(false)
        .format_target(false)
        .init();

    let matches = parse_repair_args();

    let par2_file = matches.get_one::<String>("par2_file").unwrap();
    let quiet = matches.get_flag("quiet");

    match repair_files(par2_file) {
        Ok((context, result)) => {
            // Print output unless quiet mode is enabled
            if !quiet {
                context.recovery_set.print_statistics();
                result.print_result();
            }

            // Exit with success if repair was successful or not needed, error otherwise
            if result.is_success() {
                process::exit(0);
            } else {
                process::exit(1);
            }
        }
        Err(e) => {
            if !quiet {
                eprintln!("Error: {}", e);
            }
            process::exit(1);
        }
    }
}
