# Code Coverage

Coverage is generated with `cargo-llvm-cov` by default. The CI workflow uploads
LCOV data to Codecov and archives the generated report files.

## Setup

Install the coverage tool if it is not already available:

```bash
cargo install cargo-llvm-cov
```

The full test suite also expects the `par2` reference binary to be on `PATH`.
The Nix development shell provides it through `par2cmdline-turbo`; GitHub
Actions installs the same tool before running coverage.

## Local Reports

```bash
# Terminal summary
make coverage-quick

# HTML report
make coverage-html

# Open the HTML report
make coverage-open

# Generate all llvm-cov report formats
make coverage-llvm

# CI-style reports
make coverage-ci
```

Generated reports are written under `target/coverage/`:

- `summary.txt`: text report
- `html/index.html`: browsable HTML report
- `lcov.info`: LCOV report for Codecov and other services
- `cobertura.xml`: Cobertura XML report for CI systems
- `codecov.json`: Codecov custom coverage JSON

## Script Usage

The Makefile targets call `scripts/coverage.sh`. You can run it directly when
you need a specific format:

```bash
./scripts/coverage.sh html
./scripts/coverage.sh lcov
./scripts/coverage.sh cobertura
./scripts/coverage.sh all
./scripts/coverage.sh clean
```

Extra arguments are passed to the underlying test run:

```bash
./scripts/coverage.sh html -- test_main_packet_fields
```

## Optional Tarpaulin Reports

Tarpaulin remains available as an optional comparison tool:

```bash
cargo install cargo-tarpaulin
make coverage-both
```

Tarpaulin output is written to `target/coverage/tarpaulin/`.
