# Makefile for par2rs project

.PHONY: help test coverage coverage-html coverage-open coverage-ci clean

# Default target
help:
	@echo "par2rs Makefile"
	@echo ""
	@echo "Available targets:"
	@echo "  test           - Run all tests"
	@echo "  coverage       - Generate coverage report (Tarpaulin)"
	@echo "  coverage-html  - Generate HTML coverage report"
	@echo "  coverage-llvm  - Generate coverage with LLVM-cov"
	@echo "  coverage-both  - Generate coverage with both tools"
	@echo "  coverage-open  - Open coverage report in browser"
	@echo "  coverage-ci    - Generate coverage for CI (XML format)"
	@echo "  clean          - Clean build artifacts and coverage"
	@echo "  help           - Show this help message"

# Run tests
test:
	cargo test

# Generate coverage report using the script (default: tarpaulin)
coverage:
	./scripts/coverage.sh tarpaulin

# Generate HTML coverage report
coverage-html:
	./scripts/coverage.sh tarpaulin

# Generate LLVM coverage report
coverage-llvm:
	./scripts/coverage.sh llvm

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

# Generate coverage for CI systems (XML output)
coverage-ci:
	@mkdir -p target/coverage
	cargo tarpaulin --out Xml --output-dir target/coverage --skip-clean
	@echo "Coverage report generated: target/coverage/cobertura.xml"

# Clean build artifacts and coverage data
clean:
	cargo clean
	rm -rf target/coverage
	@echo "Cleaned build artifacts and coverage data"

# Check if coverage tools are installed
check-tools:
	@command -v cargo-tarpaulin >/dev/null 2>&1 || { echo "cargo-tarpaulin not found. Run: cargo install cargo-tarpaulin"; exit 1; }
	@command -v cargo-llvm-cov >/dev/null 2>&1 || { echo "cargo-llvm-cov not found. Run: cargo install cargo-llvm-cov"; exit 1; }
	@echo "All coverage tools are installed ✓"
