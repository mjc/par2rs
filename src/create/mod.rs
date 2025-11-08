//! PAR2 creation functionality
//!
//! This module implements PAR2 file creation compatible with par2cmdline-turbo.
//! Reference: par2cmdline-turbo/src/par2creator.h and par2creator.cpp
//!
//! # Architecture
//!
//! The creation process follows these steps:
//! 1. Scan and validate source files
//! 2. Calculate optimal block size (if not specified)
//! 3. Compute file hashes and block checksums
//! 4. Generate Reed-Solomon recovery blocks
//! 5. Write PAR2 files (index + volume files)
//!
//! # Usage
//!
//! ```no_run
//! use par2rs::create::{CreateContextBuilder, RecoveryFileScheme};
//! use std::path::PathBuf;
//!
//! let mut context = CreateContextBuilder::new()
//!     .output_name("mydata.par2")
//!     .source_files(vec![PathBuf::from("file1.txt"), PathBuf::from("file2.dat")])
//!     .redundancy_percentage(10)
//!     .build()?;
//!
//! context.create()?;
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod builder;
pub mod context;
pub mod error;
pub mod file_naming;
pub mod hashing;
pub mod packet_generator;
pub mod progress;
pub mod source_file;
pub mod types;
pub mod writer;

pub use builder::CreateContextBuilder;
pub use context::CreateContext;
pub use error::{CreateError, CreateResult};
pub use file_naming::{generate_recovery_filenames, RecoveryScheme};
pub use progress::{ConsoleCreateReporter, CreateReporter, SilentCreateReporter};
pub use types::{CreateConfig, RecoveryFileScheme};
pub use writer::{Par2Writer, SourceFileInfo};

// Re-export from reed_solomon for convenience
pub use crate::reed_solomon::RecoveryBlockEncoder;

/// High-level function to create PAR2 files from source files
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp Par2Creator::Process()
///
/// # Arguments
///
/// * `output_name` - Base name for PAR2 files (e.g., "mydata.par2")
/// * `source_files` - List of files to protect
/// * `redundancy_percentage` - Redundancy percentage (5-100, typical is 5-10)
/// * `reporter` - Progress reporter implementation
///
/// # Returns
///
/// Result containing the CreateContext on success
pub fn create_files(
    output_name: &str,
    source_files: Vec<std::path::PathBuf>,
    redundancy_percentage: u32,
    reporter: Box<dyn CreateReporter>,
) -> CreateResult<CreateContext> {
    let mut context = CreateContextBuilder::new()
        .output_name(output_name)
        .source_files(source_files)
        .redundancy_percentage(redundancy_percentage)
        .reporter(reporter)
        .build()?;

    context.create()?;
    Ok(context)
}
