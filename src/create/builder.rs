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
