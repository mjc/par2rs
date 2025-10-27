#!/usr/bin/env bash
# Comprehensive benchmark suite for par2rs
# Tests multiple file sizes with appropriate iteration counts

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${CYAN}========================================${NC}"
echo -e "${CYAN}  PAR2RS COMPREHENSIVE BENCHMARK SUITE${NC}"
echo -e "${CYAN}========================================${NC}"
echo ""
echo "This will benchmark repair operations on:"
echo "  - 1MB, 10MB, 100MB (10 iterations each)"
echo "  - 1GB, 10GB (10 iterations each)"
echo "  - 100GB (3 iterations)"
echo ""
echo "Estimated time: 2-3 hours depending on hardware"
echo ""

# Check if we have enough disk space (need ~250GB for 100GB tests)
AVAILABLE=$(df -BG "$TMPDIR" 2>/dev/null || df -BG /tmp | tail -1 | awk '{print $4}' | sed 's/G//')
if [ "$AVAILABLE" -lt 250 ]; then
    echo -e "${YELLOW}Warning: Less than 250GB available in temp directory${NC}"
    echo "Available: ${AVAILABLE}GB"
    echo -e "${YELLOW}100GB test may fail. Continue? (y/n)${NC}"
    read -r response
    if [ "$response" != "y" ]; then
        exit 0
    fi
fi

# Create results directory with timestamp
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RESULTS_DIR="$PROJECT_ROOT/benchmark_results_$TIMESTAMP"
mkdir -p "$RESULTS_DIR"
RESULTS_FILE="$RESULTS_DIR/benchmark_results.txt"

echo -e "${BLUE}Results will be saved to: $RESULTS_DIR${NC}"
echo ""

# Function to run benchmark and capture output
run_benchmark() {
    local size_mb=$1
    local iterations=$2
    local size_label=$3
    
    echo -e "${GREEN}========================================${NC}"
    echo -e "${GREEN}Testing $size_label ($size_mb MB)${NC}"
    echo -e "${GREEN}Iterations: $iterations${NC}"
    echo -e "${GREEN}========================================${NC}"
    
    # Run benchmark and save output
    ITERATIONS=$iterations "$SCRIPT_DIR/benchmark_repair_averaged.sh" "$size_mb" 2>&1 | tee "$RESULTS_DIR/benchmark_${size_label}.txt"
    
    echo ""
    echo -e "${CYAN}Completed $size_label benchmark${NC}"
    echo ""
    sleep 2
}

# Start comprehensive benchmark
{
    echo "PAR2RS BENCHMARK RESULTS"
    echo "======================="
    echo "Date: $(date)"
    echo "System: $(uname -a)"
    echo "CPU: $(lscpu | grep 'Model name' | sed 's/Model name: *//')"
    echo "Memory: $(free -h | awk '/^Mem:/ {print $2}')"
    echo ""
    echo "======================="
    echo ""
} | tee "$RESULTS_FILE"

# Run benchmarks with appropriate iteration counts
run_benchmark 1 10 "1MB"
run_benchmark 10 10 "10MB"
run_benchmark 100 10 "100MB"
run_benchmark 1000 10 "1GB"
run_benchmark 10000 10 "10GB"
run_benchmark 100000 3 "100GB"

# Parse results and create summary
echo -e "${CYAN}========================================${NC}"
echo -e "${CYAN}  BENCHMARK SUMMARY${NC}"
echo -e "${CYAN}========================================${NC}"

{
    echo ""
    echo "SUMMARY OF ALL BENCHMARKS"
    echo "========================="
    echo ""
    printf "%-8s | %-12s | %-12s | %-8s\n" "Size" "par2cmdline" "par2rs" "Speedup"
    printf "%-8s-+-%-12s-+-%-12s-+-%-8s\n" "--------" "------------" "------------" "--------"
    
    for size in "1MB" "10MB" "100MB" "1GB" "10GB" "100GB"; do
        file="$RESULTS_DIR/benchmark_${size}.txt"
        if [ -f "$file" ]; then
            par2cmd=$(grep "Average:" "$file" | head -1 | awk '{print $2}')
            par2rs=$(grep "Average:" "$file" | tail -1 | awk '{print $2}')
            speedup=$(grep "Speedup:" "$file" | awk '{print $2}')
            printf "%-8s | %12s | %12s | %8s\n" "$size" "$par2cmd" "$par2rs" "$speedup"
        fi
    done
    
    echo ""
    echo "Detailed results saved to: $RESULTS_DIR"
    echo ""
} | tee -a "$RESULTS_FILE"

echo -e "${GREEN}✓ All benchmarks completed successfully!${NC}"
echo ""
echo -e "${BLUE}Results directory: $RESULTS_DIR${NC}"
echo -e "${BLUE}Summary file: $RESULTS_FILE${NC}"
