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

### GitHub Actions

The project includes a GitHub Actions workflow (`.github/workflows/rust.yml`) that automatically generates coverage reports on every push and pull request.

#### Coverage Workflow Features:
- Runs tests first to ensure they pass
- Generates coverage using `cargo-tarpaulin`
- Outputs multiple formats: XML, HTML, and LCOV
- Uploads results to Codecov
- Archives coverage reports as build artifacts

#### Setting up Codecov (Optional)

1. Go to [codecov.io](https://codecov.io) and sign up with your GitHub account
2. Add your repository to Codecov
3. Get your Codecov token from the repository settings
4. Add the token as a repository secret:
   - Go to your GitHub repo → Settings → Secrets and variables → Actions
   - Add a new secret named `CODECOV_TOKEN` with your token value

#### Manual CI Setup

For other CI systems, add to your workflow:

```yaml
- name: Install cargo-tarpaulin
  run: cargo install cargo-tarpaulin

- name: Generate coverage report
  run: |
    cargo tarpaulin --verbose --all-features --workspace --timeout 120 \
      --out Xml --out Html --out Lcov \
      --output-dir target/coverage

- name: Upload coverage to Codecov
  uses: codecov/codecov-action@v4
  with:
    file: target/coverage/cobertura.xml
```

## Coverage Configuration

The `.cargo/config.toml` file contains configuration for LLVM-based coverage instrumentation.

The `Cargo.toml` file includes tarpaulin configuration under `[package.metadata.tarpaulin]`.

## Viewing Coverage Reports

### Local HTML Reports
After generating HTML coverage:
```bash
# Open the HTML report in your browser
open target/coverage/tarpaulin-report.html
# Or for LLVM-cov:
open target/coverage/html/index.html
```

### Understanding Coverage Metrics
- **Line Coverage**: Percentage of code lines executed during tests
- **Branch Coverage**: Percentage of conditional branches tested
- **Function Coverage**: Percentage of functions called during tests

### Coverage Thresholds
Consider setting coverage targets:
- **Good**: 70-80% line coverage
- **Great**: 80-90% line coverage  
- **Excellent**: 90%+ line coverage

## Exclusions

To exclude files from coverage, add to your `Cargo.toml`:

```toml
[package.metadata.tarpaulin]
exclude = ["src/generated/*", "tests/integration/*"]
```
