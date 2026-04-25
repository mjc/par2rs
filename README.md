# par2rs

A Rust implementation of PAR2 (Parity Archive) for data recovery and verification.

## Overview

`par2rs` is a modern, high-performance implementation of the PAR2 (Parity Archive 2.0) format written in Rust. PAR2 files are used to detect and repair corruption in data files, making them invaluable for archival storage, data transmission, and backup verification.

### Performance

#### Verification/Repair

par2rs achieves **1.1-2.9x speedup** over par2cmdline for verification and repair through:
- **Optimized I/O patterns** using full slice-size chunks instead of 64KB blocks (eliminates redundant reads)
- **Parallel Reed-Solomon reconstruction** using Rayon for multi-threaded chunk processing
- **SIMD-accelerated operations** (PSHUFB on x86_64, NEON on ARM64, portable_simd cross-platform)
- **Smart validation skipping** for files with matching MD5 checksums
- **Memory-efficient lazy loading** with LRU caching

**⚠️ Performance Regression Note:** These results show significantly lower speedups than previous benchmarks (which showed 2-200× improvements). This is considered a **regression** and is under investigation. The current implementation maintains correctness but has lost most of its performance advantages on Linux x86_64.

**Latest verification/repair benchmark results:**

**Linux x86_64 (AMD Ryzen 9 5950X, 64GB RAM):**
- 1MB: **1.23x speedup** (0.032s → 0.026s)
- 10MB: **1.54x speedup** (0.074s → 0.048s)
- 100MB: **1.20x speedup** (0.386s → 0.321s)
- 1GB: **1.11x speedup** (3.74s → 3.37s)
- 10GB: **1.53x speedup** (58.80s → 38.32s)

**macOS M1 (MacBook Air, 16GB RAM) - OUTDATED (October 2025):**
- 100MB: 2.77x speedup (2.26s → 0.81s)
- 1GB: **2.99x speedup** (22.7s → 7.6s)
- 10GB: 2.46x speedup (104.8s → 42.6s)
- 25GB: 2.36x speedup (349.6s → 147.8s)
- ⚠️ These results need re-testing to confirm current performance

The performance improvements come primarily from optimized I/O patterns and SIMD-accelerated Reed-Solomon operations. See [docs/BENCHMARK_RESULTS.md](docs/BENCHMARK_RESULTS.md) for comprehensive end-to-end benchmarks and [docs/SIMD_OPTIMIZATION.md](docs/SIMD_OPTIMIZATION.md) for SIMD implementation details.

## Quick Start

### Installation

#### Using Nix Flakes (Recommended)

```bash
# Run directly without installing
nix run github:mjc/par2rs -- verify myfile.par2

# Install to your profile
nix profile install github:mjc/par2rs

# Use in a flake.nix
{
  inputs.par2rs.url = "github:mjc/par2rs";
  
  # Then use as: inputs.par2rs.packages.${system}.default
}
```

#### From Source

```bash
# Clone the repository
git clone https://github.com/mjc/par2rs.git
cd par2rs

# Build the project
cargo build --release

# Binaries will be in target/release/
# - par2 (unified interface, par2cmdline compatible)
# - par2verify, par2repair, par2create (individual tools)
```

### Basic Usage

The `par2` binary provides a par2cmdline-compatible interface:

```bash
# Verify files
par2 verify myfile.par2
par2 v myfile.par2  # short form

# Repair damaged files
par2 repair myfile.par2
par2 r myfile.par2  # short form

# Create recovery files
par2 create myfile.par2 file1 file2
par2 c myfile.par2 file1 file2  # short form
```

#### Advanced Options

```bash
# Quiet mode (minimal output)
par2 v -q myfile.par2

# Repair and purge backup files on success
par2 r -p myfile.par2

# Use specific number of threads
par2 v -t 8 myfile.par2

# Create with explicit recovery settings
par2 c -s65536 -r10 myfile.par2 file1 file2

# Store source names relative to a base path
par2 c -B /data/archive myfile.par2 /data/archive/file1
par2 v -B /data/archive myfile.par2

# Scan renamed or relocated data while verifying/repairing
par2 v myfile.par2 renamed-file
par2 r myfile.par2 renamed-file

# Disable parallel processing (single-threaded)
par2 v --no-parallel myfile.par2
```

#### Legacy Binaries

Individual binaries are also available:

#### Verify PAR2 Files

```bash
# Verify integrity of files protected by PAR2
cargo run --bin par2verify tests/fixtures/testfile.par2
```

### Library Usage

```rust
use par2rs::{parse_packets, analysis, file_verification};
use std::fs::File;

// Parse PAR2 packets from a file
let mut file = File::open("example.par2")?;
let packets = parse_packets(&mut file);

// Analyze the PAR2 set
let stats = analysis::calculate_par2_stats(&packets, 0);
analysis::print_summary_stats(&stats);

// Verify file integrity
let file_info = analysis::collect_file_info_from_packets(&packets);
let results = file_verification::verify_files_and_collect_results(&file_info, true);
```

## Architecture

### Packet Types Supported

| Packet Type | Description | Status |
|-------------|-------------|---------|
| Main Packet | Core metadata and file list | ✅ Implemented |
| Packed Main Packet | Compressed main packet variant | ✅ Implemented |
| File Description | File metadata and checksums | ✅ Implemented |
| Input File Slice Checksum | Slice-level checksums | ✅ Implemented |
| Recovery Slice | Reed-Solomon recovery data | ✅ Implemented |
| Creator | Software identification | ✅ Implemented |

### Key Components

- **`packets/`**: Binary packet parsing and serialization using `binrw`
- **`analysis.rs`**: PAR2 set analysis and statistics calculation
- **`verify.rs`**: File integrity verification with MD5 checksums
- **`file_ops.rs`**: File discovery and PAR2 collection management
- **`file_verification.rs`**: Comprehensive file verification with detailed results

## Development

### Prerequisites

- **Rust**: 1.70+ (see `rust-toolchain.toml` for exact version)
- **Optional Tools**:
  - `cargo-llvm-cov`: `cargo install cargo-llvm-cov` (for code coverage)
  - `cargo-tarpaulin`: `cargo install cargo-tarpaulin` (optional comparison coverage)

### Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Build all binaries
cargo build --release --bins
```

### Create Performance Benchmarking

For create-side comparisons, use the benchmark harness:

```bash
nix develop --command make benchmark-create-perf
```

The benchmark compares `par2rs` create variants against `par2cmdline-turbo`
(`par2` by default), runs measured iterations per case, verifies outputs across
tools, reports wall time, records Linux `perf` counters when available, and
writes a selected `par2rs` create flamegraph. On macOS, the flamegraph is
generated with `xctrace`, `inferno-collapse-xctrace`, and `inferno-flamegraph`.
Results are saved under
`target/perf-results/create/`.

Useful overrides:

```bash
nix develop --command env ITERATIONS=10 THREADS=16 PROFILE_CASE=single_5g \
  CASES='single_256m:1:256:1048576,multi_1g:64:16:1048576,single_5g:1:5120:1048576' \
  scripts/benchmark_create_perf.sh
```

The pass/fail signal is mean wall time: both `par2rs-xor-jit-port` and
`par2rs-xor-jit-clean` must match or beat `turbo-auto` for every configured
case. Linux `perf` counters are recorded to explain the wall-time result when
the benchmark is run on Linux.

### Testing

```bash
# Run all tests
cargo test

# Run specific test suites
cargo test --test test_unit           # Unit tests
cargo test --test test_integration    # Integration tests
cargo test --test test_packets        # Packet serialization tests
cargo test --test test_verification   # Verification tests

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_main_packet_fields
```

### Code Coverage

The project includes comprehensive code coverage tools:

```bash
# Quick coverage summary
make coverage-quick

# Generate HTML coverage report
make coverage-html

# Open coverage report in browser
make coverage-open

# Generate coverage for CI (text, LCOV, Cobertura)
make coverage-ci

# Generate all llvm-cov report formats
make coverage-llvm

# Compare both tools
make coverage-both
```

Reports are written under `target/coverage/`. For detailed coverage options,
see [COVERAGE.md](COVERAGE.md).

### Coverage Status

[![codecov](https://codecov.io/gh/mjc/par2rs/branch/main/graph/badge.svg)](https://codecov.io/gh/mjc/par2rs)

Coverage reports are automatically generated on every commit and pull request via GitHub Actions.

## Testing Infrastructure

### Test Organization

```
tests/
├── test_unit.rs              # Unit tests for core functionality
├── test_integration.rs       # End-to-end integration tests
├── test_packets.rs           # Packet parsing and serialization
├── test_verification.rs      # File verification tests
├── fixtures/                 # Test PAR2 files and data
└── unit/                     # Detailed unit test modules
    ├── analysis.rs
    ├── file_ops.rs
    ├── file_verification.rs
    └── repair.rs
```

### Test Fixtures

The project includes comprehensive test fixtures:
- **Real PAR2 Files**: `testfile.par2` with volume files
- **Individual Packets**: Isolated packet files for focused testing
- **Repair Scenarios**: Test files for repair functionality
  - `testfile_corrupted`: File with single corruption point
  - `testfile_heavily_corrupted`: File with multiple corruption points
  - `repair_scenarios/`: PAR2 files without data files (missing file scenario)
- **Corrupted Data**: Test cases for error handling

## Binaries

### par2verify
Verifies the integrity of files using PAR2 archives.

**Features:**
- Complete PAR2 set analysis
- File integrity verification
- Byte-by-byte block scanning for renamed or displaced data
- Progress reporting
- Detailed statistics

### par2create
Creates PAR2 recovery files for data protection.

**Features:**
- par2cmdline-style create options for block size/count, redundancy, recovery volume layout, recursion, base paths, and quiet/verbose modes
- PAR2 index and recovery volume generation
- Reed-Solomon recovery block generation
- Compatibility coverage against par2cmdline for generated sets

### par2repair
Repairs corrupted files using PAR2 recovery data.

**Features:**
- Recovery set loading from main and volume PAR2 files
- Corrupt or missing file reconstruction
- Base path support for relocated data files
- Extra file scanning for renamed or relocated protected files
- Optional purge of backup and PAR2 files after successful repair

## Performance

- **Parallel Processing**: Multi-threaded operations using Rayon
- **Memory Efficient**: Streaming packet parser
- **Fast Verification**: Optimized MD5 checksumming
- **Minimal Dependencies**: Carefully selected crate dependencies

## Dependencies

### Core Dependencies
- **binrw**: Binary reading/writing with derive macros
- **md5**: Fast MD5 hashing implementation
- **rayon**: Data parallelism library
- **clap**: Command-line argument parsing
- **hex**: Hexadecimal encoding/decoding

### Development Dependencies
- **cargo-llvm-cov**: Code coverage analysis
- **criterion**: Benchmarking framework

## Contributing

1. **Fork the repository**
2. **Create a feature branch**: `git checkout -b feature/amazing-feature`
3. **Make your changes** with tests
4. **Run the test suite**: `cargo test`
5. **Check coverage**: `make coverage-html`
6. **Commit your changes**: `git commit -m 'Add amazing feature'`
7. **Push to the branch**: `git push origin feature/amazing-feature`
8. **Open a Pull Request**

### Development Guidelines

- **Code Quality**: All code must pass `cargo clippy` and `cargo fmt`
- **Test Coverage**: Maintain high test coverage (aim for >90%)
- **Documentation**: Document all public APIs with examples
- **Performance**: Consider performance implications of changes

## PAR2 Format Support

This implementation follows the PAR2 specification and supports:
- **PAR2 2.0 Specification**: Full compliance with the standard
- **Multiple Recovery Volumes**: Support for volume files
- **Variable Block Sizes**: Flexible slice size configuration
- **Reed-Solomon Codes**: Error correction mathematics

## Compatibility Notes

### File Scanning Strategy

`par2rs` uses a **global block scanner** modeled after `par2cmdline`:

- **Fast path**: Aligned blocks are checked first for the common case where files are present at their expected paths and offsets.

- **Compatibility path**: When needed, verification and repair scan byte-by-byte with rolling CRC32 to find protected data blocks at displaced offsets or inside extra files passed on the command line.

**Practical Impact:**
- ✅ Intact files still take the fast aligned path.
- ✅ Renamed or relocated files can be supplied as extra arguments to `verify` or `repair`.
- ✅ Displaced blocks from inserted or deleted bytes are detected by the byte-scanning path.

This keeps the common case efficient while matching `par2cmdline` behavior for the recovery cases where data is present but not at the protected filename or expected block offset.

## Known Issues

- **Repair Hanging**: The repair functionality occasionally hangs on small files within large multi-file PAR2 sets. The root cause is still under investigation. Workaround: Process smaller PAR2 sets or single files where possible.

## Roadmap

- [x] **Phase 1**: Complete packet parsing and verification
- [x] **Phase 2**: PAR2 file creation (`par2create`)
- [x] **Phase 3**: File repair functionality (`par2repair`)
- [x] **Phase 4**: SIMD optimizations (PSHUFB, NEON, portable_simd)
- [ ] **Phase 5**: Runtime SIMD dispatch
- [ ] **Phase 6**: Advanced features (progress callbacks, custom block sizes)
- [ ] **Performance**: Investigate the Linux x86_64 verification/repair regression and restore prior benchmark speedups
- [ ] **Create Optimization**: Merge hashing and recovery generation into a single pass to avoid reading source files twice
- [ ] **Repair Reliability**: Reproduce and fix the repair hang on small files within large multi-file PAR2 sets
- [ ] **Benchmarks**: Re-test and refresh macOS Apple Silicon results

## Documentation

- **[README.md](README.md)**: This file - project overview and quick start
- **[BENCHMARK_RESULTS.md](docs/BENCHMARK_RESULTS.md)**: Comprehensive end-to-end performance benchmarks
- **[SIMD_OPTIMIZATION.md](docs/SIMD_OPTIMIZATION.md)**: Technical details on SIMD implementations
- **[COVERAGE.md](COVERAGE.md)**: Code coverage tooling and instructions
- **[par2_parsing.md](par2_parsing.md)**: Internal implementation notes (development reference)

## License

This project is licensed under the GNU General Public License v3.0 only - see the [LICENSE](LICENSE) file for details.

The project was relicensed to GPL-3.0-only so future PAR2 compatibility and performance work can incorporate GPL-compatible implementation details from established PAR2 tools when appropriate, with attribution and source availability preserved.

## Acknowledgments

- **PAR2 Specification**: Based on the PAR2 format specification
- **par2cmdline**: Reference implementation for compatibility testing
- **Rust Community**: For excellent crates and tooling ecosystem
