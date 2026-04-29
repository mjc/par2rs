//! Configuration for verification operations

use crate::cli::compat::{parse_memory_mb, parse_positive_usize, parse_skip_options};

/// Configuration for file verification operations
#[derive(Debug, Clone)]
pub struct VerificationConfig {
    /// Number of threads for computation (0 = auto-detect)
    pub threads: usize,
    /// Whether to use parallel verification (false = single-threaded everything)
    pub parallel: bool,
    /// Skip full file MD5 computation (for pre-repair verification where only block-level validation is needed)
    pub skip_full_file_md5: bool,
    /// Memory limit for repair/verification work, in bytes.
    pub memory_limit: Option<usize>,
    /// Number of file-level worker threads.
    pub file_threads: Option<usize>,
    /// Enable turbo-style skip-ahead scanning.
    pub data_skipping: bool,
    /// Skip-ahead scan leeway in bytes when data skipping is enabled.
    pub skip_leeway: usize,
    /// Turbo-compatible rename-only mode for verify/repair.
    pub rename_only: bool,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            threads: 0, // Auto-detect CPU cores
            parallel: true,
            skip_full_file_md5: false, // Default: compute full file MD5 for thorough verification
            memory_limit: None,
            file_threads: None,
            data_skipping: false,
            skip_leeway: 0,
            rename_only: false,
        }
    }
}

impl VerificationConfig {
    pub fn new(threads: usize, parallel: bool) -> Self {
        Self {
            threads,
            parallel,
            skip_full_file_md5: false,
            memory_limit: None,
            file_threads: None,
            data_skipping: false,
            skip_leeway: 0,
            rename_only: false,
        }
    }

    /// Create config optimized for pre-repair verification (skips full file MD5)
    pub fn for_repair(threads: usize, parallel: bool) -> Self {
        Self {
            threads,
            parallel,
            skip_full_file_md5: true, // Skip expensive full-file MD5 before repair
            memory_limit: None,
            file_threads: None,
            data_skipping: false,
            skip_leeway: 0,
            rename_only: false,
        }
    }

    pub fn from_args(matches: &clap::ArgMatches) -> Self {
        Self::try_from_args(matches).unwrap_or_else(|_| {
            let threads = matches
                .try_get_one::<String>("threads")
                .ok()
                .flatten()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            let parallel = matches
                .try_get_one::<bool>("no-parallel")
                .ok()
                .flatten()
                .copied()
                .map(|no_parallel| !no_parallel)
                .unwrap_or(true);

            Self::new(threads, parallel)
        })
    }

    pub fn try_from_args(matches: &clap::ArgMatches) -> Result<Self, String> {
        let threads = matches
            .try_get_one::<String>("threads")
            .map_err(|e| e.to_string())?
            .map(|s| {
                s.parse::<usize>()
                    .map_err(|_| format!("Invalid thread count: {s}"))
            })
            .transpose()?
            .unwrap_or(0);

        let parallel = !matches
            .try_get_one::<bool>("no-parallel")
            .map_err(|e| e.to_string())?
            .copied()
            .unwrap_or(false);

        let memory_limit = parse_memory_mb(
            matches
                .try_get_one::<String>("memory")
                .map_err(|e| e.to_string())?
                .map(String::as_str),
        )?;
        let file_threads = parse_positive_usize(
            matches
                .try_get_one::<String>("file_threads")
                .map_err(|e| e.to_string())?
                .map(String::as_str),
            "-T",
        )?;
        let skip = parse_skip_options(
            matches
                .try_get_one::<bool>("data_skipping")
                .map_err(|e| e.to_string())?
                .copied()
                .unwrap_or(false),
            matches
                .try_get_one::<String>("skip_leeway")
                .map_err(|e| e.to_string())?
                .map(String::as_str),
        )?;

        Ok(Self {
            threads,
            parallel,
            skip_full_file_md5: false,
            memory_limit,
            file_threads,
            data_skipping: skip.data_skipping,
            skip_leeway: skip.skip_leeway,
            rename_only: matches
                .try_get_one::<bool>("rename_only")
                .ok()
                .flatten()
                .copied()
                .unwrap_or(false),
        })
    }

    /// Get effective thread count (auto-detect if 0)
    pub fn effective_threads(&self) -> usize {
        match (self.parallel, self.threads) {
            (false, _) => 1, // Sequential mode always uses single thread
            (true, 0) => std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4), // Auto-detect CPU cores
            (true, n) => n,  // Use specified thread count
        }
    }

    /// Whether to actually use parallel processing (false if threads=1)
    pub fn should_parallelize(&self) -> bool {
        self.parallel && self.effective_threads() > 1
    }

    pub(crate) fn should_parallelize_file_scans(&self) -> bool {
        self.parallel
            && self
                .file_threads
                .map_or_else(|| self.effective_threads() > 1, |threads| threads > 1)
    }
}

#[cfg(test)]
mod tests {
    use super::VerificationConfig;
    use clap::{Arg, ArgAction, Command};

    fn verify_app() -> Command {
        Command::new("test")
            .arg(Arg::new("threads").long("threads"))
            .arg(
                Arg::new("no-parallel")
                    .long("no-parallel")
                    .action(ArgAction::SetTrue),
            )
            .arg(Arg::new("memory").short('m'))
            .arg(Arg::new("file_threads").short('T'))
            .arg(
                Arg::new("data_skipping")
                    .short('N')
                    .action(ArgAction::SetTrue),
            )
            .arg(Arg::new("skip_leeway").short('S'))
            .arg(
                Arg::new("rename_only")
                    .short('O')
                    .action(ArgAction::SetTrue),
            )
    }

    #[test]
    fn file_scans_can_parallelize_from_file_threads_alone() {
        let mut config = VerificationConfig::new(1, true);
        config.file_threads = Some(8);
        assert!(config.should_parallelize_file_scans());
    }

    #[test]
    fn file_scans_respect_no_parallel_even_with_file_threads() {
        let mut config = VerificationConfig::new(1, false);
        config.file_threads = Some(8);
        assert!(!config.should_parallelize_file_scans());
    }

    #[test]
    fn explicit_single_file_thread_disables_file_scan_parallelism() {
        let mut config = VerificationConfig::new(8, true);
        config.file_threads = Some(1);
        assert!(!config.should_parallelize_file_scans());
    }

    #[test]
    fn for_repair_enables_skip_full_file_md5_without_changing_parallel_policy() {
        let config = VerificationConfig::for_repair(6, true);

        assert_eq!(config.threads, 6);
        assert!(config.parallel);
        assert!(config.skip_full_file_md5);
        assert_eq!(config.memory_limit, None);
        assert_eq!(config.file_threads, None);
        assert!(!config.data_skipping);
        assert_eq!(config.skip_leeway, 0);
        assert!(!config.rename_only);
    }

    #[test]
    fn should_parallelize_depends_on_effective_thread_count() {
        assert!(!VerificationConfig::new(1, true).should_parallelize());
        assert!(VerificationConfig::new(2, true).should_parallelize());
        assert!(!VerificationConfig::new(8, false).should_parallelize());
    }

    #[test]
    fn try_from_args_parses_rename_only_and_resource_flags() {
        let matches = verify_app().get_matches_from(vec![
            "test",
            "--threads",
            "4",
            "-m16",
            "-T2",
            "-N",
            "-S10",
            "-O",
        ]);

        let config = VerificationConfig::try_from_args(&matches).unwrap();

        assert_eq!(config.threads, 4);
        assert!(config.parallel);
        assert_eq!(config.memory_limit, Some(16 * 1024 * 1024));
        assert_eq!(config.file_threads, Some(2));
        assert!(config.data_skipping);
        assert_eq!(config.skip_leeway, 10);
        assert!(config.rename_only);
    }

    #[test]
    fn try_from_args_rejects_invalid_thread_counts() {
        let matches = verify_app().get_matches_from(vec!["test", "--threads", "invalid"]);

        let error = VerificationConfig::try_from_args(&matches).unwrap_err();
        assert!(error.contains("Invalid thread count: invalid"));
    }
}
