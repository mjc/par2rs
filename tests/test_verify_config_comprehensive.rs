use par2rs::verify::VerificationConfig;

#[test]
fn test_default_config() {
    let config = VerificationConfig::default();
    assert_eq!(config.threads, 0);
    assert!(config.parallel);
}

#[test]
fn test_new_config() {
    let config = VerificationConfig::new(4, true);
    assert_eq!(config.threads, 4);
    assert!(config.parallel);
}

#[test]
fn test_new_config_sequential() {
    let config = VerificationConfig::new(8, false);
    assert_eq!(config.threads, 8);
    assert!(!config.parallel);
}

#[test]
fn test_effective_threads_auto_parallel() {
    let config = VerificationConfig::new(0, true);
    let threads = config.effective_threads();
    // Should auto-detect, minimum 1
    assert!(threads >= 1);
}

#[test]
fn test_effective_threads_explicit_parallel() {
    let config = VerificationConfig::new(6, true);
    assert_eq!(config.effective_threads(), 6);
}

#[test]
fn test_effective_threads_sequential_always_one() {
    let config = VerificationConfig::new(0, false);
    assert_eq!(config.effective_threads(), 1);
}

#[test]
fn test_effective_threads_sequential_ignores_threads() {
    let config = VerificationConfig::new(99, false);
    assert_eq!(config.effective_threads(), 1);
}

#[test]
fn test_config_clone() {
    let config1 = VerificationConfig::new(4, true);
    let config2 = config1.clone();
    assert_eq!(config1.threads, config2.threads);
    assert_eq!(config1.parallel, config2.parallel);
}

#[test]
fn test_config_debug() {
    let config = VerificationConfig::new(2, true);
    let debug_str = format!("{:?}", config);
    assert!(debug_str.contains("threads"));
    assert!(debug_str.contains("parallel"));
}

#[test]
fn test_from_args_defaults() {
    use clap::{Arg, Command};

    let app = Command::new("test")
        .arg(Arg::new("threads").long("threads"))
        .arg(
            Arg::new("no-parallel")
                .long("no-parallel")
                .action(clap::ArgAction::SetTrue),
        );

    let matches = app.get_matches_from(vec!["test"]);
    let config = VerificationConfig::from_args(&matches);

    assert_eq!(config.threads, 0);
    assert!(config.parallel);
}

#[test]
fn test_from_args_with_threads() {
    use clap::{Arg, Command};

    let app = Command::new("test")
        .arg(Arg::new("threads").long("threads"))
        .arg(
            Arg::new("no-parallel")
                .long("no-parallel")
                .action(clap::ArgAction::SetTrue),
        );

    let matches = app.get_matches_from(vec!["test", "--threads", "8"]);
    let config = VerificationConfig::from_args(&matches);

    assert_eq!(config.threads, 8);
    assert!(config.parallel);
}

#[test]
fn test_from_args_with_no_parallel() {
    use clap::{Arg, Command};

    let app = Command::new("test")
        .arg(Arg::new("threads").long("threads"))
        .arg(
            Arg::new("no-parallel")
                .long("no-parallel")
                .action(clap::ArgAction::SetTrue),
        );

    let matches = app.get_matches_from(vec!["test", "--no-parallel"]);
    let config = VerificationConfig::from_args(&matches);

    assert_eq!(config.threads, 0);
    assert!(!config.parallel);
}

#[test]
fn test_from_args_both_flags() {
    use clap::{Arg, Command};

    let app = Command::new("test")
        .arg(Arg::new("threads").long("threads"))
        .arg(
            Arg::new("no-parallel")
                .long("no-parallel")
                .action(clap::ArgAction::SetTrue),
        );

    let matches = app.get_matches_from(vec!["test", "--threads", "4", "--no-parallel"]);
    let config = VerificationConfig::from_args(&matches);

    assert_eq!(config.threads, 4);
    assert!(!config.parallel);
    // Should still be 1 thread due to sequential mode
    assert_eq!(config.effective_threads(), 1);
}

#[test]
fn test_from_args_invalid_threads_uses_default() {
    use clap::{Arg, Command};

    let app = Command::new("test")
        .arg(Arg::new("threads").long("threads"))
        .arg(
            Arg::new("no-parallel")
                .long("no-parallel")
                .action(clap::ArgAction::SetTrue),
        );

    let matches = app.get_matches_from(vec!["test", "--threads", "invalid"]);
    let config = VerificationConfig::from_args(&matches);

    assert_eq!(config.threads, 0); // Falls back to auto-detect
}

#[test]
fn test_effective_threads_single_thread() {
    let config = VerificationConfig::new(1, true);
    assert_eq!(config.effective_threads(), 1);
}

#[test]
fn test_effective_threads_large_number() {
    let config = VerificationConfig::new(128, true);
    assert_eq!(config.effective_threads(), 128);
}
