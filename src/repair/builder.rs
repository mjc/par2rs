//! Builder pattern for RepairContext

use super::context::RepairContext;
use super::error::{RepairError, Result};
use super::progress::{ConsoleReporter, ProgressReporter};
use crate::{Packet, RecoverySliceMetadata};
use std::path::PathBuf;

/// Builder for creating RepairContext with flexible configuration
///
/// Provides a more Rustic API for constructing repair contexts with
/// optional parameters and sensible defaults.
///
/// # Example
///
/// ```no_run
/// use par2rs::repair::{RepairContextBuilder, SilentReporter};
/// use std::path::PathBuf;
///
/// let context = RepairContextBuilder::new()
///     .packets(vec![/* packets */])
///     .base_path(PathBuf::from("/path/to/files"))
///     .reporter(Box::new(SilentReporter::new()))
///     .build()
///     .unwrap();
/// ```
pub struct RepairContextBuilder {
    packets: Option<Vec<Packet>>,
    metadata: Option<Vec<RecoverySliceMetadata>>,
    base_path: Option<PathBuf>,
    reporter: Option<Box<dyn ProgressReporter>>,
}

impl RepairContextBuilder {
    /// Create a new builder with default settings
    pub fn new() -> Self {
        Self {
            packets: None,
            metadata: None,
            base_path: None,
            reporter: None,
        }
    }

    /// Set the PAR2 packets
    pub fn packets(mut self, packets: Vec<Packet>) -> Self {
        self.packets = Some(packets);
        self
    }

    /// Set the recovery slice metadata (for memory-efficient loading)
    pub fn metadata(mut self, metadata: Vec<RecoverySliceMetadata>) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Set the base path for file resolution
    pub fn base_path(mut self, path: PathBuf) -> Self {
        self.base_path = Some(path);
        self
    }

    /// Set a custom progress reporter
    pub fn reporter(mut self, reporter: Box<dyn ProgressReporter>) -> Self {
        self.reporter = Some(reporter);
        self
    }

    /// Set quiet mode (uses SilentReporter if true, ConsoleReporter if false)
    pub fn quiet(mut self, quiet: bool) -> Self {
        if quiet {
            self.reporter = Some(Box::new(super::progress::SilentReporter::new()));
        } else {
            self.reporter = Some(Box::new(ConsoleReporter::new(false)));
        }
        self
    }

    /// Build the RepairContext
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No packets were provided
    /// - No base path was provided
    /// - The packets are invalid or incomplete
    pub fn build(self) -> Result<RepairContext> {
        let packets = self
            .packets
            .ok_or_else(|| RepairError::ContextCreation("No packets provided".to_string()))?;

        let base_path = self
            .base_path
            .ok_or_else(|| RepairError::ContextCreation("No base path provided".to_string()))?;

        let reporter = self
            .reporter
            .unwrap_or_else(|| Box::new(ConsoleReporter::new(false)));

        if let Some(metadata) = self.metadata {
            RepairContext::new_with_metadata_and_reporter(packets, metadata, base_path, reporter)
        } else {
            RepairContext::new_with_reporter(packets, base_path, reporter)
        }
    }
}

impl Default for RepairContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repair::SilentReporter;

    #[test]
    fn test_builder_requires_packets() {
        let result = RepairContextBuilder::new()
            .base_path(PathBuf::from("/tmp"))
            .build();

        assert!(result.is_err());
        if let Err(err) = result {
            assert!(matches!(err, RepairError::ContextCreation(_)));
        }
    }

    #[test]
    fn test_builder_requires_base_path() {
        let result = RepairContextBuilder::new().packets(vec![]).build();

        assert!(result.is_err());
        if let Err(err) = result {
            assert!(matches!(err, RepairError::ContextCreation(_)));
        }
    }

    #[test]
    fn test_builder_quiet_mode() {
        let builder = RepairContextBuilder::new().quiet(true);
        assert!(builder.reporter.is_some());

        let builder = RepairContextBuilder::new().quiet(false);
        assert!(builder.reporter.is_some());
    }

    #[test]
    fn test_builder_custom_reporter() {
        let builder = RepairContextBuilder::new().reporter(Box::new(SilentReporter::new()));
        assert!(builder.reporter.is_some());
    }
}
