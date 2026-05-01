# Makefile for par2rs project

.PHONY: help test benchmark-create-perf coverage coverage-html coverage-llvm coverage-lcov coverage-cobertura coverage-codecov coverage-all coverage-both coverage-quick coverage-tests coverage-open coverage-ci coverage-clean clean check-tools

# Default target
help:
	@echo "par2rs Makefile"
	@echo ""
	@echo "Available targets:"
	@echo "  test           - Run all tests"
	@echo "  benchmark-create-perf - Compare par2rs create vs par2cmdline-turbo with profiling support"
	@echo "  coverage       - Generate HTML coverage report"
	@echo "  coverage-html  - Generate HTML coverage report"
	@echo "  coverage-llvm  - Generate text, HTML, LCOV, Cobertura, and Codecov JSON reports"
	@echo "  coverage-lcov  - Generate LCOV coverage report"
	@echo "  coverage-ci    - Generate CI coverage reports"
	@echo "  coverage-both  - Generate llvm-cov and Tarpaulin reports"
	@echo "  coverage-open  - Open coverage report in browser"
	@echo "  coverage-clean - Clean coverage artifacts"
	@echo "  clean          - Clean build artifacts and coverage artifacts"
	@echo "  help           - Show this help message"

# Run tests
test:
	cargo test

benchmark-create-perf:
	./scripts/benchmark_create_perf.sh

# Generate an HTML coverage report
coverage:
	./scripts/coverage.sh html

# Generate HTML coverage report
coverage-html:
	./scripts/coverage.sh html

# Generate all LLVM coverage report formats
coverage-llvm:
	./scripts/coverage.sh all

# Generate LCOV coverage report
coverage-lcov:
	./scripts/coverage.sh lcov

# Generate Cobertura coverage report
coverage-cobertura:
	./scripts/coverage.sh cobertura

# Generate Codecov JSON coverage report
coverage-codecov:
	./scripts/coverage.sh codecov

# Generate all LLVM coverage report formats
coverage-all:
	./scripts/coverage.sh all

# Generate coverage with both tools
coverage-both:
	./scripts/coverage.sh both

# Quick coverage summary
coverage-quick:
	./scripts/coverage.sh quick

# Coverage for tests only
coverage-tests:
	./scripts/coverage.sh tests

# Open coverage report in browser
coverage-open:
	./scripts/coverage.sh open

# Generate coverage for CI systems
coverage-ci:
	./scripts/coverage.sh ci

# Clean coverage data only
coverage-clean:
	./scripts/coverage.sh clean

# Clean build artifacts and coverage data
clean:
	cargo clean
	rm -rf target/coverage
	@echo "Cleaned build artifacts and coverage data"

# Check if coverage tools are installed
check-tools:
	@cargo llvm-cov --version >/dev/null 2>&1 || { echo "cargo-llvm-cov not found. Run: cargo install cargo-llvm-cov"; exit 1; }
	@echo "Required coverage tools are installed"
