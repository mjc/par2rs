//! Progress reporting for PAR2 creation

/// Trait for reporting creation progress
///
/// Similar to repair::RepairReporter but for creation operations
pub trait CreateReporter: Send + Sync {
    /// Report scanning of source files
    fn report_scanning_files(&self, current: usize, total: usize, filename: &str);

    /// Report file hash computation progress
    fn report_file_hashing(&self, filename: &str, bytes_processed: u64, total_bytes: u64);

    /// Report block checksum computation
    fn report_block_checksums(&self, blocks_processed: u32, total_blocks: u32);

    /// Report recovery block generation
    fn report_recovery_generation(&self, blocks_generated: u32, total_blocks: u32);

    /// Report PAR2 file writing
    fn report_writing_file(&self, filename: &str);

    /// Report completion
    fn report_complete(&self, output_files: &[String]);

    /// Report error
    fn report_error(&self, error: &str);
}

/// Console-based progress reporter
pub struct ConsoleCreateReporter {
    quiet: bool,
}

impl ConsoleCreateReporter {
    pub fn new(quiet: bool) -> Self {
        ConsoleCreateReporter { quiet }
    }
}

impl CreateReporter for ConsoleCreateReporter {
    fn report_scanning_files(&self, current: usize, total: usize, filename: &str) {
        if !self.quiet {
            println!("Scanning files: {}/{} - {}", current, total, filename);
        }
    }

    fn report_file_hashing(&self, filename: &str, bytes_processed: u64, total_bytes: u64) {
        if !self.quiet {
            let percent = (bytes_processed as f64 / total_bytes as f64 * 100.0) as u32;
            println!("Hashing {}: {}%", filename, percent);
        }
    }

    fn report_block_checksums(&self, blocks_processed: u32, total_blocks: u32) {
        if !self.quiet {
            println!(
                "Computing block checksums: {}/{}",
                blocks_processed, total_blocks
            );
        }
    }

    fn report_recovery_generation(&self, blocks_generated: u32, total_blocks: u32) {
        if !self.quiet {
            let percent = (blocks_generated as f64 / total_blocks as f64 * 100.0) as u32;
            println!(
                "Generating recovery blocks: {}/{} ({}%)",
                blocks_generated, total_blocks, percent
            );
        }
    }

    fn report_writing_file(&self, filename: &str) {
        if !self.quiet {
            println!("Writing: {}", filename);
        }
    }

    fn report_complete(&self, output_files: &[String]) {
        if !self.quiet {
            println!("\nCreated {} PAR2 files:", output_files.len());
            for file in output_files {
                println!("  {}", file);
            }
        }
    }

    fn report_error(&self, error: &str) {
        eprintln!("Error: {}", error);
    }
}

/// Silent reporter that produces no output
pub struct SilentCreateReporter;

impl CreateReporter for SilentCreateReporter {
    fn report_scanning_files(&self, _current: usize, _total: usize, _filename: &str) {}
    fn report_file_hashing(&self, _filename: &str, _bytes_processed: u64, _total_bytes: u64) {}
    fn report_block_checksums(&self, _blocks_processed: u32, _total_blocks: u32) {}
    fn report_recovery_generation(&self, _blocks_generated: u32, _total_blocks: u32) {}
    fn report_writing_file(&self, _filename: &str) {}
    fn report_complete(&self, _output_files: &[String]) {}
    fn report_error(&self, _error: &str) {}
}
