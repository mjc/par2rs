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
    let target_files: Vec<String> = matches
        .get_many::<String>("files")
        .unwrap_or_default()
        .map(|s| s.to_string())
        .collect();
    let _verbose = matches.get_flag("verbose");
    let _quiet = matches.get_flag("quiet");

    match repair_files(par2_file, &target_files) {
        Ok(result) => {
            // Exit with success if repair was successful or not needed, error otherwise
            if result.is_success() {
                process::exit(0);
            } else {
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}
