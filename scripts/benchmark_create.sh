#!/usr/bin/env bash
# Benchmark par2rs creation performance against par2cmdline with averaging
# Runs multiple iterations and computes averages
#
# Usage: ./benchmark_create.sh [file_size_mb] [block_size_kb] [redundancy_pct] [iterations]
#
# Parameters:
#   file_size_mb   - Size of test file in MB (default: 1024, i.e., 1GB)
#   block_size_kb  - Block size in KB (default: 1024, i.e., 1MB)
#   redundancy_pct - Redundancy percentage (default: 5)
#   iterations     - Number of iterations to run (default: 3)
#
# Examples:
#   ./benchmark_create.sh                    # 1GB file, 1MB blocks, 5% redundancy, 3 iterations
#   ./benchmark_create.sh 100 2048 10 5      # 100MB file, 2MB blocks, 10% redundancy, 5 iterations
#   ./benchmark_create.sh 2048 512 15 10     # 2GB file, 512KB blocks, 15% redundancy, 10 iterations

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PAR2RS="$PROJECT_ROOT/target/release/par2"
PAR2CMDLINE="par2"

# Parse parameters with defaults
FILE_SIZE_MB="${1:-1024}"
BLOCK_SIZE_KB="${2:-1024}"
REDUNDANCY_PCT="${3:-5}"
ITERATIONS="${4:-3}"

# Temporary directories to clean up
TEMP=""
KEEP_TEMP=0

# Cleanup function
cleanup() {
    if [ $KEEP_TEMP -eq 0 ]; then
        echo ""
        echo -e "${BLUE}Cleaning up temporary files...${NC}"
        [ -n "$TEMP" ] && rm -rf "$TEMP" 2>/dev/null || true
        echo "Done!"
    else
        echo ""
        echo -e "${RED}Keeping temporary files for debugging:${NC}"
        echo -e "  Base: $TEMP"
        echo -e "  par2cmdline output: $PAR2CMD_DIR"
        echo -e "  par2rs output: $PAR2RS_DIR"
    fi
}

# Register cleanup on exit (including errors)
trap cleanup EXIT

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}PAR2 Creation Benchmark (Averaged)${NC}"
echo -e "${BLUE}File Size: ${FILE_SIZE_MB}MB${NC}"
echo -e "${BLUE}Block Size: ${BLOCK_SIZE_KB}KB${NC}"
echo -e "${BLUE}Redundancy: ${REDUNDANCY_PCT}%${NC}"
echo -e "${BLUE}Iterations: $ITERATIONS${NC}"
echo -e "${BLUE}================================${NC}"
echo ""

# Build par2rs first
echo -e "${YELLOW}Building par2rs...${NC}"
cargo build --release 2>&1 | grep -E "(Compiling par2rs|Finished|error)" || true
echo ""

# Create test directory with subdirectories for each tool
TEMP=$(mktemp -d)
PAR2CMD_DIR="$TEMP/par2cmdline"
PAR2RS_DIR="$TEMP/par2rs"
mkdir -p "$PAR2CMD_DIR" "$PAR2RS_DIR"

echo -e "${YELLOW}Creating test file in $TEMP...${NC}"

# Generate test file once in base directory
echo -e "${YELLOW}Creating test file in $TEMP...${NC}"
dd if=/dev/urandom of="$TEMP/testfile.dat" bs=1M count=$FILE_SIZE_MB 2>&1 | tail -2
echo ""

# Copy test file to each subdirectory for isolated testing
cp "$TEMP/testfile.dat" "$PAR2CMD_DIR/testfile.dat"
cp "$TEMP/testfile.dat" "$PAR2RS_DIR/testfile.dat"

# Calculate block size parameter (par2cmdline uses -s for block size in bytes)
BLOCK_SIZE_BYTES=$((BLOCK_SIZE_KB * 1024))

# Main benchmark
echo -e "${YELLOW}Benchmarking PAR2 creation...${NC}"
declare -a PAR2CMD_TIMES
declare -a PAR2RS_TIMES

for iter in $(seq 1 $ITERATIONS); do
    echo -e "${GREEN}  Iteration $iter/$ITERATIONS${NC}"
    
    # par2cmdline - in its own directory with its own copy of the file
    rm -f "$PAR2CMD_DIR"/*.par2
    START=$(date +%s.%N)
    $PAR2CMDLINE c -s$BLOCK_SIZE_BYTES -r$REDUNDANCY_PCT -q -q "$PAR2CMD_DIR/testfile.par2" "$PAR2CMD_DIR/testfile.dat" 2>&1 > /dev/null
    END=$(date +%s.%N)
    TIME=$(echo "$END - $START" | bc)
    PAR2CMD_TIMES+=($TIME)
    echo "    par2cmdline: ${TIME}s"
    
    # par2rs - in its own directory with its own copy of the file
    rm -f "$PAR2RS_DIR"/*.par2
    START=$(date +%s.%N)
    $PAR2RS c -s$BLOCK_SIZE_BYTES -r$REDUNDANCY_PCT -q "$PAR2RS_DIR/testfile.par2" "$PAR2RS_DIR/testfile.dat" 2>&1 > /dev/null
    END=$(date +%s.%N)
    TIME=$(echo "$END - $START" | bc)
    PAR2RS_TIMES+=($TIME)
    echo "    par2rs:      ${TIME}s"
done

# Calculate averages
PAR2CMD_SUM=0
PAR2RS_SUM=0
for time in "${PAR2CMD_TIMES[@]}"; do
    PAR2CMD_SUM=$(echo "$PAR2CMD_SUM + $time" | bc)
done
for time in "${PAR2RS_TIMES[@]}"; do
    PAR2RS_SUM=$(echo "$PAR2RS_SUM + $time" | bc)
done

PAR2CMD_AVG=$(echo "scale=3; $PAR2CMD_SUM / $ITERATIONS" | bc)
PAR2RS_AVG=$(echo "scale=3; $PAR2RS_SUM / $ITERATIONS" | bc)
SPEEDUP=$(echo "scale=2; $PAR2CMD_AVG / $PAR2RS_AVG" | bc)

echo ""
echo -e "${BLUE}Results:${NC}"
echo "  par2cmdline average: ${PAR2CMD_AVG}s"
echo "  par2rs average:      ${PAR2RS_AVG}s"
echo -e "${GREEN}  Speedup: ${SPEEDUP}x${NC}"
echo ""

# Verify both outputs are valid and can verify the file
echo -e "${YELLOW}Verifying PAR2 outputs...${NC}"

# Verify par2cmdline output (in its own directory)
echo -n "  par2cmdline verification: "
if $PAR2CMDLINE v -q -q "$PAR2CMD_DIR/testfile.par2" >/dev/null 2>&1; then
    echo -e "${GREEN}PASS${NC}"
else
    echo -e "${RED}FAIL - par2cmdline output invalid!${NC}"
    echo ""
    $PAR2CMDLINE v "$PAR2CMD_DIR/testfile.par2" 2>&1
    KEEP_TEMP=1
    exit 1
fi

# Verify par2rs output with par2cmdline (cross-validation, in its own directory)
echo -n "  par2rs verification (par2cmdline): "
if $PAR2CMDLINE v -q -q "$PAR2RS_DIR/testfile.par2" >/dev/null 2>&1; then
    echo -e "${GREEN}PASS${NC}"
else
    echo -e "${RED}FAIL - par2rs output invalid per par2cmdline!${NC}"
    echo ""
    echo -e "${YELLOW}par2cmdline verification output:${NC}"
    $PAR2CMDLINE v "$PAR2RS_DIR/testfile.par2" 2>&1
    echo ""
    echo -e "${YELLOW}par2rs file list:${NC}"
    ls -lh "$PAR2RS_DIR/" 2>&1
    KEEP_TEMP=1
    exit 1
fi

# Verify par2rs output with par2rs (self-validation, in its own directory)
echo -n "  par2rs verification (par2rs):      "
if $PAR2RS v -q "$PAR2RS_DIR/testfile.par2" >/dev/null 2>&1; then
    echo -e "${GREEN}PASS${NC}"
else
    echo -e "${RED}FAIL - par2rs output invalid per par2rs!${NC}"
    echo ""
    echo -e "${YELLOW}par2rs verification output:${NC}"
    $PAR2RS v "$PAR2RS_DIR/testfile.par2" 2>&1
    KEEP_TEMP=1
    exit 1
fi

# Test repair capability - corrupt the file and repair with both
echo ""
echo -e "${YELLOW}Testing repair capability...${NC}"

# Test par2cmdline repair (corrupt its own copy)
dd if=/dev/urandom of="$PAR2CMD_DIR/testfile.dat" bs=1K count=100 seek=500 conv=notrunc 2>/dev/null
echo -n "  par2cmdline repair: "
if $PAR2CMDLINE r -q -q "$PAR2CMD_DIR/testfile.par2" >/dev/null 2>&1; then
    echo -e "${GREEN}PASS${NC}"
else
    echo -e "${RED}FAIL - par2cmdline repair failed!${NC}"
    echo ""
    $PAR2CMDLINE r "$PAR2CMD_DIR/testfile.par2" 2>&1
    KEEP_TEMP=1
    exit 1
fi

# Test par2rs repair (corrupt its own copy)
dd if=/dev/urandom of="$PAR2RS_DIR/testfile.dat" bs=1K count=100 seek=500 conv=notrunc 2>/dev/null
echo -n "  par2rs repair (par2rs):      "
if $PAR2RS r -q "$PAR2RS_DIR/testfile.par2" >/dev/null 2>&1; then
    echo -e "${GREEN}PASS${NC}"
else
    echo -e "${RED}FAIL - par2rs repair failed!${NC}"
    echo ""
    $PAR2RS r "$PAR2RS_DIR/testfile.par2" 2>&1
    KEEP_TEMP=1
    exit 1
fi

echo ""
echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}Benchmark Complete - All Tests PASSED${NC}"
echo -e "${BLUE}================================${NC}"
