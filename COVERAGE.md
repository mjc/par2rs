# Code Coverage Configuration

This project includes several tools for generating code coverage reports:

## Available Coverage Tools

### 1. cargo-tarpaulin (Recommended for Linux)
Tarpaulin is specifically designed for Rust and works well on Linux systems.

### 2. cargo-llvm-cov 
LLVM-based coverage that works across platforms and provides detailed line-by-line coverage.

## Quick Commands

### Generate HTML Coverage Report with Tarpaulin
```bash
cargo tarpaulin --out Html --output-dir target/coverage
```

### Generate HTML Coverage Report with LLVM-cov
```bash
cargo llvm-cov --html --output-dir target/coverage
```

### Generate Multiple Format Reports with Tarpaulin
```bash
cargo tarpaulin --out Html --out Lcov --out Xml --output-dir target/coverage
```

### Generate Coverage for Specific Tests
```bash
# Run coverage on unit tests only
cargo tarpaulin --tests --out Html --output-dir target/coverage

# Run coverage on integration tests only  
cargo tarpaulin --test integration --out Html --output-dir target/coverage
```

### View Coverage Summary in Terminal
```bash
cargo tarpaulin --out Stdout
```

### Generate Coverage with LLVM-cov (Alternative)
```bash
# Terminal output
cargo llvm-cov

# HTML output
cargo llvm-cov --html

# LCOV format (for CI integration)
cargo llvm-cov --lcov --output-path target/coverage/lcov.info
```

## Coverage Output Locations

- **HTML Reports**: `target/coverage/tarpaulin-report.html` or `target/coverage/html/`
- **LCOV Files**: `target/coverage/lcov.info`
- **XML Reports**: `target/coverage/cobertura.xml`

## CI Integration

For GitHub Actions, add to your workflow:

```yaml
- name: Install cargo-tarpaulin
  run: cargo install cargo-tarpaulin

- name: Generate coverage report
  run: cargo tarpaulin --out Xml --output-dir target/coverage

- name: Upload coverage to Codecov
  uses: codecov/codecov-action@v3
  with:
    file: target/coverage/cobertura.xml
```

## Coverage Configuration

The `.cargo/config.toml` file contains configuration for LLVM-based coverage instrumentation.

## Exclusions

To exclude files from coverage, add to your `Cargo.toml`:

```toml
[package.metadata.tarpaulin]
exclude = ["src/generated/*", "tests/integration/*"]
```
