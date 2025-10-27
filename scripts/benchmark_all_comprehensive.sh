#!/usr/bin/env bash
# Comprehensive benchmark for all file sizes
# Runs averaged benchmarks for 1MB, 10MB, 100MB, 1GB, 10GB, 100GB
# Usage: ./benchmark_all_comprehensive.sh [temp_dir]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEMP_BASE=${1:-}

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${CYAN}========================================${NC}"
echo -e "${CYAN}PAR2RS COMPREHENSIVE BENCHMARK SUITE${NC}"
echo -e "${CYAN}========================================${NC}"
echo ""
echo "Testing file sizes:"
echo "  • 1MB, 10MB, 100MB (10 iterations each)"
echo "  • 1GB, 10GB (10 iterations each)"
echo "  • 100GB (3 iterations)"
echo ""
if [ -n "$TEMP_BASE" ]; then
    echo -e "Temp directory: ${YELLOW}$TEMP_BASE${NC}"
    echo ""
fi

# Store results
RESULTS_FILE="/tmp/par2rs_benchmark_results_$(date +%Y%m%d_%H%M%S).txt"
echo "Results will be saved to: $RESULTS_FILE"
echo ""

# Function to run benchmark and capture output
run_benchmark() {
    local size=$1
    local iterations=$2
    local label=$3
    
    echo -e "${BLUE}========================================${NC}"
    echo -e "${BLUE}Benchmarking ${label}${NC}"
    echo -e "${BLUE}========================================${NC}"
    echo ""
    
    if [ -n "$TEMP_BASE" ]; then
        ITERATIONS=$iterations "$SCRIPT_DIR/benchmark_repair_averaged.sh" $size "$TEMP_BASE" | tee -a "$RESULTS_FILE"
    else
        ITERATIONS=$iterations "$SCRIPT_DIR/benchmark_repair_averaged.sh" $size | tee -a "$RESULTS_FILE"
    fi
    
    echo "" | tee -a "$RESULTS_FILE"
    echo "" | tee -a "$RESULTS_FILE"
}

# Start timestamp
echo "Benchmark started at: $(date)" | tee "$RESULTS_FILE"
echo "" | tee -a "$RESULTS_FILE"

# Run benchmarks
run_benchmark 1 10 "1MB"
run_benchmark 10 10 "10MB"
run_benchmark 100 10 "100MB"
run_benchmark 1000 10 "1GB"
run_benchmark 10000 10 "10GB"
run_benchmark 100000 3 "100GB"

# End timestamp
echo "Benchmark completed at: $(date)" | tee -a "$RESULTS_FILE"
echo "" | tee -a "$RESULTS_FILE"

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}ALL BENCHMARKS COMPLETED${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo -e "Results saved to: ${YELLOW}$RESULTS_FILE${NC}"
echo ""
echo "Summary extraction:"
echo "  grep -A 8 'Results' $RESULTS_FILE"
