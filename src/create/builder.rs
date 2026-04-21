//! Builder pattern for CreateContext

use super::context::CreateContext;
use super::error::CreateResult;
use super::progress::{ConsoleCreateReporter, CreateReporter};
use super::types::{CreateConfig, RecoveryFileScheme};
use crate::domain::SourceBlockCount;
use std::path::PathBuf;

/// Builder for CreateContext
///
/// Provides a fluent API for configuring PAR2 creation
///
/// # Example
///
/// ```no_run
/// use par2rs::create::CreateContextBuilder;
/// use std::path::PathBuf;
///
/// let context = CreateContextBuilder::new()
///     .output_name("mydata.par2")
///     .source_files(vec![PathBuf::from("file1.txt")])
///     .redundancy_percentage(10)
///     .build()?;
/// # Ok::<(), par2rs::create::CreateError>(())
/// ```
pub struct CreateContextBuilder {
    config: CreateConfig,
    reporter: Option<Box<dyn CreateReporter>>,
}

impl CreateContextBuilder {
    /// Create a new builder with default settings
    pub fn new() -> Self {
        CreateContextBuilder {
            config: CreateConfig::default(),
            reporter: None,
        }
    }

    /// Set the output PAR2 file base name
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use par2rs::create::CreateContextBuilder;
    /// let builder = CreateContextBuilder::new()
    ///     .output_name("mydata.par2");
    /// ```
    pub fn output_name(mut self, name: impl Into<String>) -> Self {
        self.config.output_name = name.into();
        self
    }

    /// Set the source files to protect
    pub fn source_files(mut self, files: Vec<PathBuf>) -> Self {
        self.config.source_files = files;
        self
    }

    /// Add a single source file
    pub fn add_source_file(mut self, file: PathBuf) -> Self {
        self.config.source_files.push(file);
        self
    }

    /// Set base path used to derive source packet names.
    pub fn base_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.base_path = Some(path.into());
        self
    }

    /// Set explicit block size in bytes
    ///
    /// If not set, block size will be auto-calculated from source_block_count
    /// Reference: par2cmdline -s option
    pub fn block_size(mut self, size: u64) -> Self {
        self.config.block_size = Some(size);
        self
    }

    /// Set target number of source blocks
    ///
    /// If block_size is not set, block size will be calculated to achieve this target
    /// Reference: par2cmdline -b option
    pub fn source_block_count(mut self, count: u32) -> Self {
        self.config.source_block_count = Some(SourceBlockCount::new(count));
        self
    }

    /// Set redundancy percentage (1-100)
    ///
    /// Typical values: 5-10%
    /// This determines how many recovery blocks to create
    pub fn redundancy_percentage(mut self, percent: u32) -> Self {
        self.config.redundancy_percentage = Some(percent);
        self
    }

    /// Set explicit recovery block count
    ///
    /// If set, overrides redundancy_percentage
    pub fn recovery_block_count(mut self, count: u32) -> Self {
        self.config.recovery_block_count = Some(count);
        self
    }

    /// Set recovery file distribution scheme
    pub fn recovery_file_scheme(mut self, scheme: RecoveryFileScheme) -> Self {
        self.config.recovery_file_scheme = scheme;
        self
    }

    /// Set number of recovery files (for Limited scheme)
    pub fn recovery_file_count(mut self, count: u32) -> Self {
        self.config.recovery_file_count = Some(count);
        self
    }

    /// Set first recovery block exponent (default 0)
    ///
    /// Advanced option: sets the starting exponent for recovery blocks.
    /// Normally left at 0; useful when splitting recovery data across separate par2 sets.
    /// Reference: par2cmdline -f option
    pub fn first_recovery_block(mut self, exponent: u32) -> Self {
        self.config.first_recovery_block = exponent;
        self
    }

    /// Set memory limit for processing
    pub fn memory_limit(mut self, limit: usize) -> Self {
        self.config.memory_limit = Some(limit);
        self
    }

    /// Set number of threads (0 = auto-detect)
    pub fn thread_count(mut self, count: u32) -> Self {
        self.config.thread_count = count;
        self
    }

    /// Set custom progress reporter
    pub fn reporter(mut self, reporter: Box<dyn CreateReporter>) -> Self {
        self.reporter = Some(reporter);
        self
    }

    /// Set quiet mode (minimal output)
    pub fn quiet(mut self, quiet: bool) -> Self {
        self.reporter = Some(Box::new(ConsoleCreateReporter::new(quiet)));
        self
    }

    /// Build the CreateContext
    ///
    /// Validates configuration and initializes the context
    pub fn build(self) -> CreateResult<CreateContext> {
        // Validate configuration
        self.config.validate()?;

        // Use default reporter if none specified
        let reporter = self
            .reporter
            .unwrap_or_else(|| Box::new(ConsoleCreateReporter::new(false)));

        CreateContext::new(self.config, reporter)
    }
}

impl Default for CreateContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Default / new() ---

    #[test]
    fn default_produces_same_as_new() {
        // Both should produce a builder with the same default config
        let b1 = CreateContextBuilder::new();
        let b2 = CreateContextBuilder::default();
        // Verify both have empty output_name (proxy for default config)
        assert!(b1.config.output_name.is_empty());
        assert!(b2.config.output_name.is_empty());
    }

    // --- Fluent API ---

    #[test]
    fn setter_chain_sets_fields() {
        let builder = CreateContextBuilder::new()
            .output_name("mydata.par2")
            .redundancy_percentage(10)
            .thread_count(2)
            .first_recovery_block(5)
            .memory_limit(1024 * 1024)
            .base_path(PathBuf::from("/tmp/base"))
            .source_block_count(1000)
            .recovery_block_count(50)
            .recovery_file_count(4)
            .recovery_file_scheme(RecoveryFileScheme::Uniform);

        assert_eq!(builder.config.output_name, "mydata.par2");
        assert_eq!(builder.config.redundancy_percentage, Some(10));
        assert_eq!(builder.config.thread_count, 2);
        assert_eq!(builder.config.first_recovery_block, 5);
        assert_eq!(builder.config.memory_limit, Some(1024 * 1024));
        assert_eq!(builder.config.base_path, Some(PathBuf::from("/tmp/base")));
        assert_eq!(builder.config.recovery_block_count, Some(50));
        assert_eq!(builder.config.recovery_file_count, Some(4));
        assert_eq!(
            builder.config.recovery_file_scheme,
            RecoveryFileScheme::Uniform
        );
    }

    #[test]
    fn add_source_file_appends() {
        let builder = CreateContextBuilder::new()
            .add_source_file(PathBuf::from("a.dat"))
            .add_source_file(PathBuf::from("b.dat"));
        assert_eq!(builder.config.source_files.len(), 2);
    }

    #[test]
    fn source_files_replaces() {
        let builder = CreateContextBuilder::new()
            .add_source_file(PathBuf::from("a.dat"))
            .source_files(vec![PathBuf::from("x.dat")]);
        assert_eq!(builder.config.source_files.len(), 1);
        assert_eq!(builder.config.source_files[0], PathBuf::from("x.dat"));
    }

    #[test]
    fn quiet_true_sets_reporter() {
        let builder = CreateContextBuilder::new().quiet(true);
        assert!(builder.reporter.is_some());
    }

    #[test]
    fn quiet_false_sets_reporter() {
        let builder = CreateContextBuilder::new().quiet(false);
        assert!(builder.reporter.is_some());
    }

    // --- build() validation failures ---

    #[test]
    fn build_fails_with_no_output_name() {
        let result = CreateContextBuilder::new()
            .source_files(vec![PathBuf::from("a.dat")])
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn build_fails_with_no_source_files() {
        let result = CreateContextBuilder::new().output_name("out.par2").build();
        assert!(result.is_err());
    }

    #[test]
    fn build_fails_with_nonexistent_source_file() {
        let result = CreateContextBuilder::new()
            .output_name("out.par2")
            .source_files(vec![PathBuf::from("/nonexistent/file.dat")])
            .build();
        assert!(result.is_err());
    }

    // --- build() success with real temp file ---

    #[test]
    fn build_succeeds_with_valid_config() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test.dat");
        std::fs::write(&file_path, b"hello par2rs").unwrap();

        let ctx = CreateContextBuilder::new()
            .output_name("out.par2")
            .source_files(vec![file_path])
            .redundancy_percentage(5)
            .build();

        assert!(ctx.is_ok());
    }

    #[test]
    fn build_with_explicit_block_size() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test.dat");
        std::fs::write(&file_path, b"hello par2rs").unwrap();

        let ctx = CreateContextBuilder::new()
            .output_name("out.par2")
            .source_files(vec![file_path])
            .block_size(512)
            .recovery_block_count(2)
            .build();

        assert!(ctx.is_ok());
        assert_eq!(ctx.unwrap().block_size(), 512);
    }
}
