#!/usr/bin/env bash
# Run all benchmarks and save results
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS_FILE="$SCRIPT_DIR/../benchmark_results.txt"

# Clear previous results
> "$RESULTS_FILE"

echo "Running comprehensive benchmarks..."
echo "Results will be saved to: $RESULTS_FILE"
echo ""

# 10MB - 10 iterations
echo "=== Running 10MB benchmark (10 iterations) ===" | tee -a "$RESULTS_FILE"
ITERATIONS=10 "$SCRIPT_DIR/benchmark_repair_averaged.sh" 10 2>&1 | tee -a "$RESULTS_FILE"
echo "" | tee -a "$RESULTS_FILE"

# 100MB - 10 iterations
echo "=== Running 100MB benchmark (10 iterations) ===" | tee -a "$RESULTS_FILE"
ITERATIONS=10 "$SCRIPT_DIR/benchmark_repair_averaged.sh" 100 2>&1 | tee -a "$RESULTS_FILE"
echo "" | tee -a "$RESULTS_FILE"

# 1GB - 10 iterations
echo "=== Running 1GB benchmark (10 iterations) ===" | tee -a "$RESULTS_FILE"
ITERATIONS=10 "$SCRIPT_DIR/benchmark_repair_averaged.sh" 1000 2>&1 | tee -a "$RESULTS_FILE"
echo "" | tee -a "$RESULTS_FILE"

# 10GB - 5 iterations
echo "=== Running 10GB benchmark (5 iterations) ===" | tee -a "$RESULTS_FILE"
ITERATIONS=5 "$SCRIPT_DIR/benchmark_repair_averaged.sh" 10000 2>&1 | tee -a "$RESULTS_FILE"
echo "" | tee -a "$RESULTS_FILE"

# 25GB - 3 iterations
echo "=== Running 25GB benchmark (3 iterations) ===" | tee -a "$RESULTS_FILE"
ITERATIONS=3 "$SCRIPT_DIR/benchmark_repair_averaged.sh" 25000 2>&1 | tee -a "$RESULTS_FILE"
echo "" | tee -a "$RESULTS_FILE"

echo "All benchmarks complete!"
echo "Results saved to: $RESULTS_FILE"
