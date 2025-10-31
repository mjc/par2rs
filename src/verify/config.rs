//! Configuration for verification operations

/// Configuration for file verification operations
#[derive(Debug, Clone)]
pub struct VerificationConfig {
    /// Number of threads for computation (0 = auto-detect)
    pub threads: usize,
    /// Whether to use parallel verification (false = single-threaded everything)
    pub parallel: bool,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            threads: 0, // Auto-detect CPU cores
            parallel: true,
        }
    }
}

impl VerificationConfig {
    pub fn new(threads: usize, parallel: bool) -> Self {
        Self { threads, parallel }
    }

    pub fn from_args(matches: &clap::ArgMatches) -> Self {
        let threads = matches
            .get_one::<String>("threads")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let parallel = !matches.get_flag("no-parallel");

        Self::new(threads, parallel)
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
}
