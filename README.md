# par2rs

A Rust implementation of PAR2 (Parity Archive) for data recovery and verification.

## Features

- PAR2 file creation, verification, and repair
- Cross-platform support
- Fast parallel processing
- Comprehensive test coverage

## Quick Start

### Building

```bash
cargo build --release
```

### Running Tests

```bash
cargo test
```

### Code Coverage

This project includes comprehensive code coverage tools and reporting:

```bash
# Quick coverage summary
make coverage-quick

# Generate HTML coverage report
make coverage-html

# Open coverage report in browser
make coverage-open

# Generate coverage for CI
make coverage-ci
```

For more coverage options, see [COVERAGE.md](COVERAGE.md).

### Coverage Status

[![codecov](https://codecov.io/gh/YOURUSERNAME/par2rs/branch/main/graph/badge.svg)](https://codecov.io/gh/YOURUSERNAME/par2rs)

Coverage reports are automatically generated on every commit and pull request via GitHub Actions.

## Development

### Prerequisites

- Rust 1.70+ (see `rust-toolchain.toml`)
- `cargo-tarpaulin` for coverage (optional): `cargo install cargo-tarpaulin`
- `cargo-llvm-cov` for LLVM coverage (optional): `cargo install cargo-llvm-cov`

### Testing

```bash
# Run all tests
cargo test

# Run specific test suite
cargo test --test test_unit
cargo test --test test_integration
cargo test --test test_packets
```

### Coverage Tools

The project includes several coverage tools:

- **make coverage** - Generate coverage with Tarpaulin
- **make coverage-llvm** - Generate coverage with LLVM
- **make coverage-both** - Generate coverage with both tools
- **./scripts/coverage.sh** - Flexible coverage script with multiple options

See [COVERAGE.md](COVERAGE.md) for detailed coverage documentation.

## License

[Insert your license here]
