#!/usr/bin/env bash

# Code Coverage Report Generator for par2rs
# This script provides multiple options for generating coverage reports

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Create coverage directory
mkdir -p target/coverage

echo -e "${BLUE}=== par2rs Code Coverage Generator ===${NC}"
echo ""

# Function to run tarpaulin coverage
run_tarpaulin() {
    echo -e "${YELLOW}Running Tarpaulin coverage analysis...${NC}"
    cargo tarpaulin \
        --out Html \
        --out Lcov \
        --out Xml \
        --output-dir target/coverage \
        --skip-clean \
        --verbose
    
    echo -e "${GREEN}✓ Tarpaulin coverage report generated${NC}"
    echo -e "  📁 HTML Report: target/coverage/tarpaulin-report.html"
    echo -e "  📁 LCOV File: target/coverage/lcov.info"
    echo -e "  📁 XML Report: target/coverage/cobertura.xml"
}

# Function to run LLVM coverage
run_llvm_cov() {
    echo -e "${YELLOW}Running LLVM coverage analysis...${NC}"
    
    # Clean previous coverage data
    cargo llvm-cov clean
    
    # Generate HTML report
    cargo llvm-cov --html --output-dir target/coverage/llvm-html
    
    # Generate LCOV file
    cargo llvm-cov --lcov --output-path target/coverage/llvm-lcov.info
    
    echo -e "${GREEN}✓ LLVM coverage report generated${NC}"
    echo -e "  📁 HTML Report: target/coverage/llvm-html/index.html"
    echo -e "  📁 LCOV File: target/coverage/llvm-lcov.info"
}

# Function to run quick coverage summary
run_quick() {
    echo -e "${YELLOW}Running quick coverage summary...${NC}"
    cargo tarpaulin --out Stdout
}

# Function to show coverage for tests only
run_tests_only() {
    echo -e "${YELLOW}Running coverage for tests only...${NC}"
    cargo tarpaulin --tests --out Html --output-dir target/coverage/tests-only
    echo -e "${GREEN}✓ Tests-only coverage report generated${NC}"
    echo -e "  📁 HTML Report: target/coverage/tests-only/tarpaulin-report.html"
}

# Function to open coverage report
open_report() {
    if [ -f "target/coverage/tarpaulin-report.html" ]; then
        echo -e "${BLUE}Opening Tarpaulin coverage report...${NC}"
        xdg-open target/coverage/tarpaulin-report.html 2>/dev/null || \
        open target/coverage/tarpaulin-report.html 2>/dev/null || \
        echo -e "${YELLOW}Please open target/coverage/tarpaulin-report.html in your browser${NC}"
    elif [ -f "target/coverage/llvm-html/index.html" ]; then
        echo -e "${BLUE}Opening LLVM coverage report...${NC}"
        xdg-open target/coverage/llvm-html/index.html 2>/dev/null || \
        open target/coverage/llvm-html/index.html 2>/dev/null || \
        echo -e "${YELLOW}Please open target/coverage/llvm-html/index.html in your browser${NC}"
    else
        echo -e "${RED}No coverage report found. Run coverage first.${NC}"
        exit 1
    fi
}

# Show help
show_help() {
    echo -e "${BLUE}Usage: $0 [OPTION]${NC}"
    echo ""
    echo "Options:"
    echo "  tarpaulin     Generate coverage using cargo-tarpaulin (default)"
    echo "  llvm          Generate coverage using cargo-llvm-cov"
    echo "  both          Generate coverage using both tools"
    echo "  quick         Show quick coverage summary in terminal"
    echo "  tests         Generate coverage for tests only"
    echo "  open          Open the coverage report in browser"
    echo "  help          Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0 tarpaulin   # Generate Tarpaulin coverage report"
    echo "  $0 both        # Generate both Tarpaulin and LLVM reports"
    echo "  $0 quick       # Quick terminal summary"
    echo "  $0 open        # Open coverage report in browser"
}

# Check if coverage tools are installed
check_tools() {
    if ! command -v cargo-tarpaulin &> /dev/null && [[ "$1" == *"tarpaulin"* || "$1" == "both" || -z "$1" ]]; then
        echo -e "${RED}Error: cargo-tarpaulin is not installed${NC}"
        echo "Run: cargo install cargo-tarpaulin"
        exit 1
    fi
    
    if ! command -v cargo-llvm-cov &> /dev/null && [[ "$1" == *"llvm"* || "$1" == "both" ]]; then
        echo -e "${RED}Error: cargo-llvm-cov is not installed${NC}"
        echo "Run: cargo install cargo-llvm-cov"
        exit 1
    fi
}

# Main logic
case "${1:-tarpaulin}" in
    "tarpaulin")
        check_tools "tarpaulin"
        run_tarpaulin
        ;;
    "llvm")
        check_tools "llvm"
        run_llvm_cov
        ;;
    "both")
        check_tools "both"
        run_tarpaulin
        echo ""
        run_llvm_cov
        ;;
    "quick")
        check_tools "tarpaulin"
        run_quick
        ;;
    "tests")
        check_tools "tarpaulin"
        run_tests_only
        ;;
    "open")
        open_report
        ;;
    "help"|"-h"|"--help")
        show_help
        ;;
    *)
        echo -e "${RED}Unknown option: $1${NC}"
        echo ""
        show_help
        exit 1
        ;;
esac

echo ""
echo -e "${GREEN}Coverage analysis complete!${NC}"
