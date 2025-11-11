#!/usr/bin/env bash
# Stress test creation with various sizes to trigger intermittent bug

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PAR2RS="$PROJECT_ROOT/target/release/par2"

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

FAILURES=0
SUCCESSES=0

test_size() {
    local size_mb=$1
    local block_kb=$2
    local iter=$3
    
    TEMP=$(mktemp -d)
    
    # Create test file
    dd if=/dev/urandom of="$TEMP/test.dat" bs=1M count=$size_mb 2>/dev/null
    
    # Create PAR2
    $PAR2RS c -s$((block_kb * 1024)) -r5 "$TEMP/test.par2" "$TEMP/test.dat" >/dev/null 2>&1
    
    # Verify
    if $PAR2RS v -q "$TEMP/test.par2" >/dev/null 2>&1; then
        echo -e "${GREEN}.${NC}"
        ((SUCCESSES++))
    else
        echo -e "${RED}F${NC}"
        echo "FAILED: ${size_mb}MB, ${block_kb}KB blocks (iteration $iter)"
        echo "  Temp dir: $TEMP"
        ((FAILURES++))
        return 1
    fi
    
    rm -rf "$TEMP"
    return 0
}

echo "Stress testing PAR2 creation..."
echo "Testing various sizes and block sizes..."
echo ""

# Test configurations that are near the threshold
CONFIGS=(
    "920 1536"   # Just below threshold
    "930 1536"   # Near threshold
    "940 1536"   # Above threshold (original failing case)
    "950 1536"   # Well above
    "1000 1024"  # Different block size
    "500 2048"   # Larger blocks
)

for config in "${CONFIGS[@]}"; do
    read size_mb block_kb <<< "$config"
    echo -n "Testing ${size_mb}MB with ${block_kb}KB blocks: "
    
    for i in {1..10}; do
        test_size $size_mb $block_kb $i || break
    done
    echo ""
done

echo ""
echo "Results: $SUCCESSES successes, $FAILURES failures"

if [ $FAILURES -gt 0 ]; then
    exit 1
fi
