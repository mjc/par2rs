// File naming scheme for PAR2 recovery files
//
// Reference: par2cmdline-turbo/src/par2creator.cpp InitialiseOutputFiles() lines 484-630
// Reference: par2cmdline-turbo/src/libpar2.h Scheme enum lines 91-96

use std::path::PathBuf;

/// Recovery file naming scheme
/// Reference: par2cmdline-turbo/src/libpar2.h lines 91-96
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecoveryScheme {
    /// All PAR2 files have roughly the same number of blocks
    /// Reference: par2cmdline-turbo/src/par2creator.cpp lines 503-514
    Uniform,

    /// Each PAR2 file has 2x as many blocks as the previous (exponential)
    /// Reference: par2cmdline-turbo/src/par2creator.cpp lines 516-537
    #[default]
    Variable,

    /// Like Variable but limits maximum file size to largest source file size
    /// Reference: par2cmdline-turbo/src/par2creator.cpp lines 539-580
    Limited,
}

/// File allocation for a single recovery file
/// Reference: par2cmdline-turbo/src/par2creator.cpp FileAllocation class lines 465-476
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileAllocation {
    /// First recovery block exponent in this file
    exponent: u32,
    /// Number of recovery blocks in this file
    count: u32,
}

/// Generate recovery file allocation plan
///
/// # Arguments
/// * `recovery_file_count` - Number of recovery files to create
/// * `recovery_block_count` - Total number of recovery blocks
/// * `first_recovery_block` - First exponent value (usually 0)
/// * `scheme` - Distribution scheme (Uniform/Variable/Limited)
/// * `largest_file_size` - Size of largest source file (bytes, for Limited scheme)
/// * `block_size` - Block size (bytes)
///
/// # Returns
/// Vector of (exponent, count) pairs for each recovery file.
/// The last entry is the index file (base.par2) with count=0.
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp lines 489-589
fn allocate_recovery_blocks(
    recovery_file_count: u32,
    recovery_block_count: u32,
    first_recovery_block: u32,
    scheme: RecoveryScheme,
    largest_file_size: u64,
    block_size: u64,
) -> Vec<FileAllocation> {
    let mut allocations = vec![
        FileAllocation {
            exponent: 0,
            count: 0
        };
        (recovery_file_count + 1) as usize
    ];

    if recovery_file_count == 0 {
        // Only index file
        allocations[0] = FileAllocation {
            exponent: first_recovery_block,
            count: 0,
        };
        return allocations;
    }

    let mut exponent = first_recovery_block;

    match scheme {
        // Reference: par2cmdline-turbo/src/par2creator.cpp lines 503-514
        RecoveryScheme::Uniform => {
            // Files will have roughly the same number of recovery blocks each.
            let base = recovery_block_count / recovery_file_count;
            let remainder = recovery_block_count % recovery_file_count;

            for file_number in 0..recovery_file_count {
                let count = if file_number < remainder {
                    base + 1
                } else {
                    base
                };
                allocations[file_number as usize] = FileAllocation { exponent, count };
                exponent += count;
            }
        }

        // Reference: par2cmdline-turbo/src/par2creator.cpp lines 516-537
        RecoveryScheme::Variable => {
            // Files will have recovery blocks allocated in an exponential fashion.

            // Work out how many blocks to place in the smallest file
            let mut low_block_count = 1;
            let mut max_recovery_blocks = (1 << recovery_file_count) - 1;
            while max_recovery_blocks < recovery_block_count {
                low_block_count <<= 1;
                max_recovery_blocks <<= 1;
            }

            // Allocate the blocks
            let mut blocks = recovery_block_count;
            for file_number in 0..recovery_file_count {
                let count = low_block_count.min(blocks);
                allocations[file_number as usize] = FileAllocation { exponent, count };
                exponent += count;
                blocks -= count;
                low_block_count <<= 1;
            }
        }

        // Reference: par2cmdline-turbo/src/par2creator.cpp lines 539-580
        RecoveryScheme::Limited => {
            // Files will be allocated in an exponential fashion but the
            // maximum file size will be limited.

            let largest = largest_file_size.div_ceil(block_size) as u32;
            let mut file_number = recovery_file_count;
            let mut blocks = recovery_block_count;

            exponent = first_recovery_block + recovery_block_count;

            // Allocate uniformly at the top
            while blocks >= 2 * largest && file_number > 0 {
                file_number -= 1;
                exponent -= largest;
                blocks -= largest;

                allocations[file_number as usize] = FileAllocation {
                    exponent,
                    count: largest,
                };
            }

            assert!(blocks > 0 && file_number > 0);

            exponent = first_recovery_block;
            let mut count = 1;
            let files = file_number;

            // Allocate exponentially at the bottom
            for file_number in 0..files {
                let number = count.min(blocks);
                allocations[file_number as usize] = FileAllocation {
                    exponent,
                    count: number,
                };

                exponent += number;
                blocks -= number;
                count <<= 1;
            }
        }
    }

    // There will be an extra file with no recovery blocks (the index file)
    // Reference: par2cmdline-turbo/src/par2creator.cpp lines 584-585
    allocations[recovery_file_count as usize] = FileAllocation { exponent, count: 0 };

    allocations
}

/// Data for a single recovery volume file
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryFilePlan {
    /// Filename for this volume (e.g. "test.vol00+2.par2")
    pub filename: PathBuf,
    /// First recovery block exponent in this file
    pub first_exponent: u32,
    /// Number of recovery blocks in this file
    pub block_count: u32,
}

/// Generate recovery file plan (filenames + allocation data together)
///
/// Returns one entry per volume file. The index file (base.par2) is NOT
/// included — callers handle that separately.
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp InitialiseOutputFiles()
pub fn plan_recovery_files(
    base_name: &str,
    recovery_file_count: u32,
    recovery_block_count: u32,
    first_recovery_block: u32,
    scheme: RecoveryScheme,
    largest_file_size: u64,
    block_size: u64,
) -> Vec<RecoveryFilePlan> {
    let allocations = allocate_recovery_blocks(
        recovery_file_count,
        recovery_block_count,
        first_recovery_block,
        scheme,
        largest_file_size,
        block_size,
    );

    // Compute digit widths for filenames (same logic as generate_recovery_filenames)
    let mut limit_low = 0;
    let mut limit_count = 0;
    for alloc in &allocations {
        if limit_low < alloc.exponent {
            limit_low = alloc.exponent;
        }
        if limit_count < alloc.count {
            limit_count = alloc.count;
        }
    }
    let digits_low = count_digits(limit_low);
    let digits_count = count_digits(limit_count);

    // Build plan for volume files only (skip the last entry, which is the index)
    let mut plan = Vec::with_capacity(recovery_file_count as usize);
    for file_number in 0..recovery_file_count as usize {
        let alloc = &allocations[file_number];
        let filename = format!(
            "{}.vol{:0width_exp$}+{:0width_cnt$}.par2",
            base_name,
            alloc.exponent,
            alloc.count,
            width_exp = digits_low,
            width_cnt = digits_count,
        );
        plan.push(RecoveryFilePlan {
            filename: PathBuf::from(filename),
            first_exponent: alloc.exponent,
            block_count: alloc.count,
        });
    }

    plan
}

/// Compute default number of recovery files for the Variable scheme
///
/// Uses ceil(log2(recovery_block_count)), matching par2cmdline-turbo's default.
/// Minimum is 1.
pub fn default_recovery_file_count(recovery_block_count: u32) -> u32 {
    if recovery_block_count <= 1 {
        return 1;
    }
    // ceil(log2(n)) = number of bits needed to represent n-1
    let bits = u32::BITS - (recovery_block_count - 1).leading_zeros();
    bits.max(1)
}

/// Generate recovery file names
///
/// # Arguments
/// * `base_name` - Base filename (without .par2 extension)
/// * `recovery_file_count` - Number of recovery files to create
/// * `recovery_block_count` - Total number of recovery blocks
/// * `first_recovery_block` - First exponent value (usually 0)
/// * `scheme` - Distribution scheme
/// * `largest_file_size` - Size of largest source file (bytes)
/// * `block_size` - Block size (bytes)
///
/// # Returns
/// Vector of filenames. Last entry is base.par2 (index file).
///
/// # Examples
/// ```
/// use par2rs::create::file_naming::{generate_recovery_filenames, RecoveryScheme};
///
/// // Variable scheme with 10 recovery blocks, 3 files
/// // Algorithm: lowblockcount=2 (since (1<<3)-1=7 < 10), so allocates 2, 4, 4
/// let files = generate_recovery_filenames(
///     "testfile",
///     3,
///     10,
///     0,
///     RecoveryScheme::Variable,
///     1_000_000,
///     16384,
/// );
/// assert_eq!(files.len(), 4);
/// assert_eq!(files[0].to_str().unwrap(), "testfile.vol00+2.par2");
/// assert_eq!(files[1].to_str().unwrap(), "testfile.vol02+4.par2");
/// assert_eq!(files[2].to_str().unwrap(), "testfile.vol06+4.par2");
/// assert_eq!(files[3].to_str().unwrap(), "testfile.par2");
/// ```
///
/// Reference: par2cmdline-turbo/src/par2creator.cpp lines 587-630
pub fn generate_recovery_filenames(
    base_name: &str,
    recovery_file_count: u32,
    recovery_block_count: u32,
    first_recovery_block: u32,
    scheme: RecoveryScheme,
    largest_file_size: u64,
    block_size: u64,
) -> Vec<PathBuf> {
    let allocations = allocate_recovery_blocks(
        recovery_file_count,
        recovery_block_count,
        first_recovery_block,
        scheme,
        largest_file_size,
        block_size,
    );

    // Determine the format to use for filenames of recovery files
    // Reference: par2cmdline-turbo/src/par2creator.cpp lines 587-620
    let mut limit_low = 0;
    let mut limit_count = 0;
    for alloc in &allocations {
        if limit_low < alloc.exponent {
            limit_low = alloc.exponent;
        }
        if limit_count < alloc.count {
            limit_count = alloc.count;
        }
    }

    // Calculate number of digits needed for exponent and count
    let digits_low = count_digits(limit_low);
    let digits_count = count_digits(limit_count);

    // Generate filenames
    // Reference: par2cmdline-turbo/src/par2creator.cpp lines 622-629
    let mut filenames = Vec::with_capacity((recovery_file_count + 1) as usize);

    for file_number in 0..recovery_file_count {
        let alloc = &allocations[file_number as usize];
        let filename = format!(
            "{}.vol{:0width_exp$}+{:0width_cnt$}.par2",
            base_name,
            alloc.exponent,
            alloc.count,
            width_exp = digits_low,
            width_cnt = digits_count
        );
        filenames.push(PathBuf::from(filename));
    }

    // Index file (last entry)
    // Reference: par2cmdline-turbo/src/par2creator.cpp line 630
    filenames.push(PathBuf::from(format!("{}.par2", base_name)));

    filenames
}

/// Count number of decimal digits needed to represent a number
/// Reference: par2cmdline-turbo/src/par2creator.cpp lines 604-608, 611-615
fn count_digits(n: u32) -> usize {
    if n == 0 {
        return 1;
    }
    let mut digits = 1;
    let mut t = n;
    while t >= 10 {
        t /= 10;
        digits += 1;
    }
    digits
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test uniform distribution
    /// Reference: par2cmdline-turbo/src/par2creator.cpp lines 503-514
    #[test]
    fn test_uniform_scheme() {
        // 10 blocks into 3 files: 4, 3, 3
        let allocations = allocate_recovery_blocks(3, 10, 0, RecoveryScheme::Uniform, 0, 1);

        assert_eq!(allocations.len(), 4); // 3 recovery files + 1 index file

        // First file gets extra block (10/3 = 3 remainder 1)
        assert_eq!(allocations[0].exponent, 0);
        assert_eq!(allocations[0].count, 4);

        assert_eq!(allocations[1].exponent, 4);
        assert_eq!(allocations[1].count, 3);

        assert_eq!(allocations[2].exponent, 7);
        assert_eq!(allocations[2].count, 3);

        // Index file
        assert_eq!(allocations[3].exponent, 10);
        assert_eq!(allocations[3].count, 0);
    }

    /// Test variable (exponential) distribution
    /// Reference: par2cmdline-turbo/src/par2creator.cpp lines 516-537
    #[test]
    fn test_variable_scheme() {
        // 10 blocks into 3 files
        // max=(1<<3)-1=7, 7<10 so double: lowblockcount=2, max=14
        // Allocate: 2, 4, 4
        let allocations = allocate_recovery_blocks(3, 10, 0, RecoveryScheme::Variable, 0, 1);

        assert_eq!(allocations.len(), 4);

        // Exponential with lowblockcount=2
        assert_eq!(allocations[0].exponent, 0);
        assert_eq!(allocations[0].count, 2);

        assert_eq!(allocations[1].exponent, 2);
        assert_eq!(allocations[1].count, 4);

        assert_eq!(allocations[2].exponent, 6);
        assert_eq!(allocations[2].count, 4);

        // Index file
        assert_eq!(allocations[3].exponent, 10);
        assert_eq!(allocations[3].count, 0);
    }

    /// Test variable scheme with exact power of 2
    /// Reference: par2cmdline-turbo/src/par2creator.cpp lines 516-537
    #[test]
    fn test_variable_scheme_exact() {
        // 7 blocks into 3 files: 1, 2, 4 (exactly fits)
        let allocations = allocate_recovery_blocks(3, 7, 0, RecoveryScheme::Variable, 0, 1);

        assert_eq!(allocations.len(), 4);

        assert_eq!(allocations[0].count, 1);
        assert_eq!(allocations[1].count, 2);
        assert_eq!(allocations[2].count, 4);
        assert_eq!(allocations[3].count, 0); // index file
    }

    /// Test limited scheme (exponential with size cap)
    /// Reference: par2cmdline-turbo/src/par2creator.cpp lines 539-580
    #[test]
    fn test_limited_scheme() {
        // 100 blocks, 5 files, largest file = 30 blocks worth
        // Note: The Limited scheme may not allocate all blocks if the parameters
        // don't align well. This is expected behavior from par2cmdline-turbo.

        let block_size = 1024;
        let largest_file_size = 30 * block_size;
        let allocations = allocate_recovery_blocks(
            5,
            100,
            0,
            RecoveryScheme::Limited,
            largest_file_size,
            block_size,
        );

        assert_eq!(allocations.len(), 6);

        // Verify we allocated blocks (may not be all 100 due to algorithm design)
        let total: u32 = allocations[0..5].iter().map(|a| a.count).sum();
        assert!(total > 0, "Should allocate at least some blocks");
        assert!(total <= 100, "Should not allocate more than requested");

        // Verify index file has no blocks
        assert_eq!(allocations[5].count, 0);
    }

    /// Test filename generation with uniform scheme
    /// Reference: par2cmdline-turbo/src/par2creator.cpp lines 622-630
    #[test]
    fn test_generate_filenames_uniform() {
        let files = generate_recovery_filenames(
            "test",
            3,
            10,
            0,
            RecoveryScheme::Uniform,
            1_000_000,
            16384,
        );

        assert_eq!(files.len(), 4);

        // Uniform: 4, 3, 3 blocks
        // Exponents: 0, 4, 7
        // Max exponent: 10 (2 digits), max count: 4 (1 digit)
        assert_eq!(files[0].to_str().unwrap(), "test.vol00+4.par2");
        assert_eq!(files[1].to_str().unwrap(), "test.vol04+3.par2");
        assert_eq!(files[2].to_str().unwrap(), "test.vol07+3.par2");
        assert_eq!(files[3].to_str().unwrap(), "test.par2");
    }

    /// Test filename generation with variable scheme
    /// Reference: par2cmdline-turbo/src/par2creator.cpp lines 622-630
    #[test]
    fn test_generate_filenames_variable() {
        let files = generate_recovery_filenames(
            "testfile",
            3,
            10,
            0,
            RecoveryScheme::Variable,
            1_000_000,
            16384,
        );

        assert_eq!(files.len(), 4);

        // Variable: 2, 4, 4 blocks
        // Exponents: 0, 2, 6
        // Max exponent: 10 (2 digits), max count: 4 (1 digit)
        assert_eq!(files[0].to_str().unwrap(), "testfile.vol00+2.par2");
        assert_eq!(files[1].to_str().unwrap(), "testfile.vol02+4.par2");
        assert_eq!(files[2].to_str().unwrap(), "testfile.vol06+4.par2");
        assert_eq!(files[3].to_str().unwrap(), "testfile.par2");
    }

    /// Test filename formatting with large numbers
    /// Reference: par2cmdline-turbo/src/par2creator.cpp lines 604-620
    #[test]
    fn test_filename_digit_padding() {
        let files = generate_recovery_filenames(
            "big",
            2,
            1000,
            0,
            RecoveryScheme::Uniform,
            1_000_000,
            16384,
        );

        // Uniform: 500, 500
        // Max exponent: 1000 (4 digits), max count: 500 (3 digits)
        assert_eq!(files[0].to_str().unwrap(), "big.vol0000+500.par2");
        assert_eq!(files[1].to_str().unwrap(), "big.vol0500+500.par2");
        assert_eq!(files[2].to_str().unwrap(), "big.par2");
    }

    /// Test with non-zero first recovery block
    /// Reference: par2cmdline-turbo/src/par2creator.cpp line 491
    #[test]
    fn test_nonzero_first_block() {
        let files =
            generate_recovery_filenames("test", 2, 5, 10, RecoveryScheme::Uniform, 1_000_000, 1024);

        // Uniform: 3, 2 blocks starting at exponent 10
        // Exponents: 10, 13
        assert_eq!(files[0].to_str().unwrap(), "test.vol10+3.par2");
        assert_eq!(files[1].to_str().unwrap(), "test.vol13+2.par2");
        assert_eq!(files[2].to_str().unwrap(), "test.par2");
    }

    /// Test zero recovery files (only index file)
    /// Reference: par2cmdline-turbo/src/par2creator.cpp lines 484-489
    #[test]
    fn test_zero_recovery_files() {
        let files =
            generate_recovery_filenames("test", 0, 0, 0, RecoveryScheme::Variable, 1_000_000, 1024);

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].to_str().unwrap(), "test.par2");
    }

    /// Test count_digits helper
    /// Reference: par2cmdline-turbo/src/par2creator.cpp lines 604-615
    #[test]
    fn test_count_digits() {
        assert_eq!(count_digits(0), 1);
        assert_eq!(count_digits(9), 1);
        assert_eq!(count_digits(10), 2);
        assert_eq!(count_digits(99), 2);
        assert_eq!(count_digits(100), 3);
        assert_eq!(count_digits(999), 3);
        assert_eq!(count_digits(1000), 4);
    }

    /// Test single recovery file
    #[test]
    fn test_single_recovery_file() {
        let files =
            generate_recovery_filenames("test", 1, 5, 0, RecoveryScheme::Uniform, 1_000_000, 1024);

        assert_eq!(files.len(), 2);
        assert_eq!(files[0].to_str().unwrap(), "test.vol0+5.par2");
        assert_eq!(files[1].to_str().unwrap(), "test.par2");
    }

    /// Test plan_recovery_files returns filenames + allocation data together
    #[test]
    fn plan_recovery_files_returns_filenames_and_allocation() {
        let plan =
            plan_recovery_files("test", 3, 10, 0, RecoveryScheme::Variable, 1_000_000, 16384);
        // 3 volume files (index file is NOT included in plan)
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].filename.to_str().unwrap(), "test.vol00+2.par2");
        assert_eq!(plan[0].first_exponent, 0);
        assert_eq!(plan[0].block_count, 2);
        assert_eq!(plan[1].filename.to_str().unwrap(), "test.vol02+4.par2");
        assert_eq!(plan[1].first_exponent, 2);
        assert_eq!(plan[1].block_count, 4);
        assert_eq!(plan[2].filename.to_str().unwrap(), "test.vol06+4.par2");
        assert_eq!(plan[2].first_exponent, 6);
        assert_eq!(plan[2].block_count, 4);
    }

    /// Test plan_recovery_files with zero recovery files returns empty plan
    #[test]
    fn plan_recovery_files_zero_files_returns_empty() {
        let plan = plan_recovery_files("test", 0, 0, 0, RecoveryScheme::Variable, 1_000_000, 1024);
        assert!(plan.is_empty());
    }

    /// Test default_recovery_file_count gives ceil(log2(n))
    #[test]
    fn default_recovery_file_count_is_ceil_log2() {
        assert_eq!(default_recovery_file_count(1), 1);
        assert_eq!(default_recovery_file_count(2), 1);
        assert_eq!(default_recovery_file_count(3), 2);
        assert_eq!(default_recovery_file_count(4), 2);
        assert_eq!(default_recovery_file_count(5), 3);
        assert_eq!(default_recovery_file_count(8), 3);
        assert_eq!(default_recovery_file_count(9), 4);
        assert_eq!(default_recovery_file_count(10), 4);
        assert_eq!(default_recovery_file_count(16), 4);
        assert_eq!(default_recovery_file_count(17), 5);
    }

    /// Test variable scheme low_block_count calculation
    /// Reference: par2cmdline-turbo/src/par2creator.cpp lines 520-525
    #[test]
    fn test_variable_low_block_count() {
        // 3 files: max = (1<<3) - 1 = 7
        // If recovery_block_count = 8, need to double: lowblockcount = 2
        // 2 << 0 = 2, 2 << 1 = 4, 2 << 2 = 8 (total 14)
        let allocations = allocate_recovery_blocks(3, 14, 0, RecoveryScheme::Variable, 0, 1);

        assert_eq!(allocations[0].count, 2);
        assert_eq!(allocations[1].count, 4);
        assert_eq!(allocations[2].count, 8);
    }
}
