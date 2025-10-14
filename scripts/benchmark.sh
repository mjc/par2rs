#!/usr/bin/env bash
# Comprehensive benchmarking script for par2rs repair operations
# Compares par2rs against par2cmdline with statistical analysis

set -euo pipefail

# Configuration
ITERATIONS=${1:-20}
TEST_FILE="tests/fixtures/testfile.par2"
WARMUP_RUNS=3

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo "========================================"
echo "PAR2 Repair Benchmark"
echo "========================================"
echo "Iterations: $ITERATIONS"
echo "Warmup runs: $WARMUP_RUNS"
echo "Test file: $TEST_FILE"
echo ""

# Build release version
echo -e "${BLUE}Building par2rs (release mode)...${NC}"
cargo build --release --quiet
echo ""

# Check if par2cmdline is available
if ! command -v par2 &> /dev/null; then
    echo -e "${RED}Error: par2cmdline not found. Please install it for comparison.${NC}"
    exit 1
fi

# Warmup runs
echo -e "${YELLOW}Performing warmup runs...${NC}"
for i in $(seq 1 $WARMUP_RUNS); do
    ./target/release/par2repair -q "$TEST_FILE" 2>/dev/null || true
    par2 r -q "$TEST_FILE" 2>/dev/null || true
done
echo ""

# Benchmark par2rs
echo -e "${BLUE}Benchmarking par2rs...${NC}"
PAR2RS_TIMES=()
for i in $(seq 1 $ITERATIONS); do
    # Use GNU time for precise measurements
    TIME_OUTPUT=$( { /usr/bin/time -f "%e" ./target/release/par2repair -q "$TEST_FILE" 2>&1 1>/dev/null; } 2>&1 )
    PAR2RS_TIMES+=("$TIME_OUTPUT")
    printf "  Run %2d/%d: %s seconds\n" "$i" "$ITERATIONS" "$TIME_OUTPUT"
done
echo ""

# Benchmark par2cmdline
echo -e "${BLUE}Benchmarking par2cmdline...${NC}"
PAR2CMD_TIMES=()
for i in $(seq 1 $ITERATIONS); do
    TIME_OUTPUT=$( { /usr/bin/time -f "%e" par2 r -q "$TEST_FILE" 2>&1 1>/dev/null; } 2>&1 )
    PAR2CMD_TIMES+=("$TIME_OUTPUT")
    printf "  Run %2d/%d: %s seconds\n" "$i" "$ITERATIONS" "$TIME_OUTPUT"
done
echo ""

# Calculate statistics using awk
calculate_stats() {
    local times=("$@")
    awk -v times="${times[*]}" '
    BEGIN {
        n = split(times, arr)
        sum = 0
        for (i = 1; i <= n; i++) {
            sum += arr[i]
        }
        mean = sum / n
        
        # Calculate variance
        var_sum = 0
        for (i = 1; i <= n; i++) {
            var_sum += (arr[i] - mean)^2
        }
        stddev = sqrt(var_sum / n)
        
        # Find min and max
        min = arr[1]
        max = arr[1]
        for (i = 2; i <= n; i++) {
            if (arr[i] < min) min = arr[i]
            if (arr[i] > max) max = arr[i]
        }
        
        printf "%.4f %.4f %.4f %.4f", mean, stddev, min, max
    }'
}

echo "========================================"
echo "Results"
echo "========================================"

# Calculate par2rs stats
PAR2RS_STATS=$(calculate_stats "${PAR2RS_TIMES[@]}")
read -r PAR2RS_MEAN PAR2RS_STDDEV PAR2RS_MIN PAR2RS_MAX <<< "$PAR2RS_STATS"

# Calculate par2cmdline stats
PAR2CMD_STATS=$(calculate_stats "${PAR2CMD_TIMES[@]}")
read -r PAR2CMD_MEAN PAR2CMD_STDDEV PAR2CMD_MIN PAR2CMD_MAX <<< "$PAR2CMD_STATS"

echo ""
echo -e "${GREEN}par2rs:${NC}"
printf "  Mean:   %.4f seconds\n" "$PAR2RS_MEAN"
printf "  StdDev: %.4f seconds\n" "$PAR2RS_STDDEV"
printf "  Min:    %.4f seconds\n" "$PAR2RS_MIN"
printf "  Max:    %.4f seconds\n" "$PAR2RS_MAX"

echo ""
echo -e "${GREEN}par2cmdline:${NC}"
printf "  Mean:   %.4f seconds\n" "$PAR2CMD_MEAN"
printf "  StdDev: %.4f seconds\n" "$PAR2CMD_STDDEV"
printf "  Min:    %.4f seconds\n" "$PAR2CMD_MIN"
printf "  Max:    %.4f seconds\n" "$PAR2CMD_MAX"

echo ""
echo "========================================"

# Calculate speedup
SPEEDUP=$(awk -v par2rs="$PAR2RS_MEAN" -v par2cmd="$PAR2CMD_MEAN" '
    BEGIN {
        if (par2rs < par2cmd) {
            speedup = (par2cmd / par2rs - 1) * 100
            printf "par2rs is %.2f%% FASTER", speedup
        } else {
            slowdown = (par2rs / par2cmd - 1) * 100
            printf "par2rs is %.2f%% SLOWER", slowdown
        }
    }')

if [[ "$PAR2RS_MEAN" < "$PAR2CMD_MEAN" ]]; then
    echo -e "${GREEN}✓ $SPEEDUP${NC}"
else
    echo -e "${RED}✗ $SPEEDUP${NC}"
fi

# Statistical significance test (simple comparison)
DIFF=$(awk -v par2rs="$PAR2RS_MEAN" -v par2cmd="$PAR2CMD_MEAN" -v par2rs_std="$PAR2RS_STDDEV" -v par2cmd_std="$PAR2CMD_STDDEV" '
    BEGIN {
        diff = par2cmd - par2rs
        combined_std = sqrt(par2rs_std^2 + par2cmd_std^2)
        printf "%.4f %.4f", diff, combined_std
    }')
read -r MEAN_DIFF COMBINED_STD <<< "$DIFF"

echo ""
printf "Mean difference: %.4f seconds\n" "$MEAN_DIFF"
printf "Combined std dev: %.4f seconds\n" "$COMBINED_STD"

# Simple significance check (difference > 2*combined_stddev)
SIGNIFICANT=$(awk -v diff="$MEAN_DIFF" -v std="$COMBINED_STD" '
    BEGIN {
        if (diff < 0) diff = -diff
        if (diff > 2 * std) print "YES"
        else print "NO"
    }')

if [[ "$SIGNIFICANT" == "YES" ]]; then
    echo -e "${GREEN}Difference is statistically significant (>2σ)${NC}"
else
    echo -e "${YELLOW}Difference is NOT statistically significant (<2σ)${NC}"
fi

echo "========================================"
