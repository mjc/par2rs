#!/usr/bin/env bash
# Extract benchmark results summary from the comprehensive benchmark output

if [ $# -eq 0 ]; then
    echo "Usage: $0 <benchmark_results_file>"
    exit 1
fi

RESULTS_FILE="$1"

if [ ! -f "$RESULTS_FILE" ]; then
    echo "Error: File not found: $RESULTS_FILE"
    exit 1
fi

echo "# PAR2RS Comprehensive Benchmark Results"
echo ""
echo "## Test System"
echo "- CPU: $(lscpu | grep 'Model name' | cut -d: -f2 | xargs)"
echo "- Threads: $(nproc)"
echo "- Date: $(date '+%Y-%m-%d')"
echo ""
echo "## Performance Summary"
echo ""
echo "| File Size | par2cmdline (avg) | par2rs (avg) | Speedup |"
echo "|-----------|-------------------|--------------|---------|"

# Extract results for each size
for size in "100MB" "1GB" "10GB" "100GB"; do
    # Find the results section for this size
    result=$(grep -A 20 "Benchmarking $size" "$RESULTS_FILE" | grep -A 8 "Results (" | grep "Average:")
    
    if [ -n "$result" ]; then
        par2cmd_avg=$(echo "$result" | head -1 | awk '{print $2}')
        par2rs_avg=$(echo "$result" | tail -1 | awk '{print $2}')
        
        # Extract speedup
        speedup=$(grep -A 20 "Benchmarking $size" "$RESULTS_FILE" | grep "Speedup:" | head -1 | awk '{print $2}')
        
        echo "| $size | $par2cmd_avg | $par2rs_avg | $speedup |"
    fi
done

echo ""
echo "## Detailed Results"
echo ""

# Print detailed results for each successful benchmark
for size in "100MB" "1GB" "10GB" "100GB"; do
    if grep -q "Benchmarking $size" "$RESULTS_FILE"; then
        echo "### $size"
        echo ""
        echo "\`\`\`"
        grep -A 25 "Benchmarking $size" "$RESULTS_FILE" | grep -A 22 "Results (" | head -23
        echo "\`\`\`"
        echo ""
    fi
done
