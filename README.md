# par2rs

A Rust implementation of PAR2 (Parity Archive) for data recovery and verification.

## Overview

`par2rs` is a modern, high-performance implementation of the PAR2 (Parity Archive 2.0) format written in Rust. PAR2 files are used to detect and repair corruption in data files, making them invaluable for archival storage, data transmission, and backup verification.

### Performance

par2rs achieves **1.6x - 2.9x speedup** over par2cmdline through:
- **Parallel Reed-Solomon reconstruction** using Rayon for multi-threaded chunk processing
- **SIMD-accelerated operations** (PSHUFB on x86_64, NEON on ARM64, portable_simd cross-platform)
- **Smart validation skipping** for files with matching MD5 checksums
- **Sequential I/O optimization** to minimize disk seeks
- **Memory-efficient lazy loading** with 8MB buffers

**Platform-specific results:**
- **Linux x86_64 (AVX2)**: 1.93x - 2.90x speedup
- **macOS Apple Silicon (M1)**: 1.57x - 1.99x speedup

See [docs/BENCHMARK_RESULTS.md](docs/BENCHMARK_RESULTS.md) for comprehensive end-to-end benchmarks and [docs/SIMD_OPTIMIZATION.md](docs/SIMD_OPTIMIZATION.md) for SIMD implementation details.

## Quick Start

### Installation

```bash
# Clone the repository
git clone https://github.com/YOURUSERNAME/par2rs.git
cd par2rs

# Build the project
cargo build --release
```

### Basic Usage

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

### Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Build all binaries
cargo build --release --bins
```

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

# Generate coverage for CI (multiple formats)
make coverage-ci

# LLVM-based coverage
make coverage-llvm

# Compare both tools
make coverage-both
```

For detailed coverage options, see [COVERAGE.md](COVERAGE.md).

### Coverage Status

[![codecov](https://codecov.io/gh/YOURUSERNAME/par2rs/branch/main/graph/badge.svg)](https://codecov.io/gh/YOURUSERNAME/par2rs)

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
- Progress reporting
- Detailed statistics

### par2create (Planned)
Creates PAR2 recovery files for data protection.

### par2repair (Planned)
Repairs corrupted files using PAR2 recovery data.

### split_par2 (Utility)
Development utility to split PAR2 files into individual packets for analysis.

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

## Known Issues

- **Repair Hanging**: The repair functionality occasionally hangs on small files within large multi-file PAR2 sets. The root cause is still under investigation. Workaround: Process smaller PAR2 sets or single files where possible.

## Roadmap

- [x] **Phase 1**: Complete packet parsing and verification
- [ ] **Phase 2**: PAR2 file creation (`par2create`)
- [x] **Phase 3**: File repair functionality (`par2repair`)
- [x] **Phase 4**: SIMD optimizations (PSHUFB, NEON, portable_simd)
- [ ] **Phase 5**: Runtime SIMD dispatch
- [ ] **Phase 6**: Advanced features (progress callbacks, custom block sizes)

## Documentation

- **[README.md](README.md)**: This file - project overview and quick start
- **[BENCHMARK_RESULTS.md](docs/BENCHMARK_RESULTS.md)**: Comprehensive end-to-end performance benchmarks
- **[SIMD_OPTIMIZATION.md](docs/SIMD_OPTIMIZATION.md)**: Technical details on SIMD implementations
- **[COVERAGE.md](COVERAGE.md)**: Code coverage tooling and instructions
- **[par2_parsing.md](par2_parsing.md)**: Internal implementation notes (development reference)

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- **PAR2 Specification**: Based on the PAR2 format specification
- **par2cmdline**: Reference implementation for compatibility testing
- **Rust Community**: For excellent crates and tooling ecosystem
