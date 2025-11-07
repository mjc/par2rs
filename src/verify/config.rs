//! Configuration for verification operations

/// Configuration for file verification operations
#[derive(Debug, Clone)]
pub struct VerificationConfig {
    /// Number of threads for computation (0 = auto-detect)
    pub threads: usize,
    /// Whether to use parallel verification (false = single-threaded everything)
    pub parallel: bool,
    /// Skip full file MD5 computation (for pre-repair verification where only block-level validation is needed)
    pub skip_full_file_md5: bool,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            threads: 0, // Auto-detect CPU cores
            parallel: true,
            skip_full_file_md5: false, // Default: compute full file MD5 for thorough verification
        }
    }
}

impl VerificationConfig {
    pub fn new(threads: usize, parallel: bool) -> Self {
        Self {
            threads,
            parallel,
            skip_full_file_md5: false,
        }
    }

    /// Create config optimized for pre-repair verification (skips full file MD5)
    pub fn for_repair(threads: usize, parallel: bool) -> Self {
        Self {
            threads,
            parallel,
            skip_full_file_md5: true, // Skip expensive full-file MD5 before repair
        }
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

    /// Whether to actually use parallel processing (false if threads=1)
    pub fn should_parallelize(&self) -> bool {
        self.parallel && self.effective_threads() > 1
    }
}
