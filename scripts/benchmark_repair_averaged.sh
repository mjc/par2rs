#!/usr/bin/env bash
# Benchmark par2rs repair performance against par2cmdline with averaging
# Runs 10 iterations and computes averages

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PAR2RS="$PROJECT_ROOT/target/release/par2repair"
PAR2CMDLINE="par2"
ITERATIONS=10

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}PAR2 Repair Benchmark (Averaged)${NC}"
echo -e "${BLUE}Testing 100MB file repair${NC}"
echo -e "${BLUE}Iterations: $ITERATIONS${NC}"
echo -e "${BLUE}================================${NC}"
echo ""

# Build par2rs first
echo -e "${YELLOW}Building par2rs...${NC}"
cd "$PROJECT_ROOT"
cargo build --release 2>&1 | grep -E "(Compiling par2rs|Finished|error)" || true
echo ""

# Create test file once
TEMP=$(mktemp -d)
echo -e "${YELLOW}Creating 100MB test file in $TEMP...${NC}"
dd if=/dev/urandom of="$TEMP/testfile_100mb" bs=1M count=100 2>&1 | tail -2
echo ""

# Create PAR2 files once
echo -e "${YELLOW}Creating PAR2 files with 5% redundancy...${NC}"
$PAR2CMDLINE c -r5 "$TEMP/testfile_100mb.par2" "$TEMP/testfile_100mb" 2>&1 | tail -5
echo ""

# Save original MD5
MD5_ORIGINAL=$(md5sum "$TEMP/testfile_100mb" | awk '{print $1}')
echo "Original MD5: $MD5_ORIGINAL"
echo ""

# Arrays to store times
declare -a PAR2CMD_TIMES
declare -a PAR2RS_TIMES

for i in $(seq 1 $ITERATIONS); do
    echo -e "${GREEN}=== Iteration $i/$ITERATIONS ===${NC}"
    
    # Create working directory for this iteration
    TEMP_ITER=$(mktemp -d)
    cp "$TEMP/testfile_100mb" "$TEMP_ITER/"
    cp "$TEMP"/*.par2 "$TEMP_ITER/"
    
    # Corrupt the file (1MB at offset 50MB)
    dd if=/dev/zero of="$TEMP_ITER/testfile_100mb" bs=1M count=1 seek=50 conv=notrunc 2>&1 > /dev/null
    
    # Benchmark par2cmdline
    cd "$TEMP_ITER"
    START=$(date +%s.%N)
    $PAR2CMDLINE r testfile_100mb.par2 > /dev/null 2>&1
    END=$(date +%s.%N)
    PAR2CMD_TIME=$(echo "$END - $START" | bc)
    PAR2CMD_TIMES+=($PAR2CMD_TIME)
    
    # Verify par2cmdline repair
    MD5_PAR2CMD=$(md5sum "$TEMP_ITER/testfile_100mb" | awk '{print $1}')
    
    # Corrupt again for par2rs
    dd if=/dev/zero of="$TEMP_ITER/testfile_100mb" bs=1M count=1 seek=50 conv=notrunc 2>&1 > /dev/null
    
    # Benchmark par2rs
    START=$(date +%s.%N)
    $PAR2RS testfile_100mb.par2 > /dev/null 2>&1
    END=$(date +%s.%N)
    PAR2RS_TIME=$(echo "$END - $START" | bc)
    PAR2RS_TIMES+=($PAR2RS_TIME)
    
    # Verify par2rs repair
    MD5_PAR2RS=$(md5sum "$TEMP_ITER/testfile_100mb" | awk '{print $1}')
    
    echo "  par2cmdline: ${PAR2CMD_TIME}s"
    echo "  par2rs:      ${PAR2RS_TIME}s"
    
    # Verify correctness
    if [ "$MD5_PAR2CMD" != "$MD5_ORIGINAL" ] || [ "$MD5_PAR2RS" != "$MD5_ORIGINAL" ]; then
        echo -e "${RED}✗ Repair verification failed in iteration $i!${NC}"
        exit 1
    fi
    
    # Cleanup iteration
    rm -rf "$TEMP_ITER"
    echo ""
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

# Calculate min/max
PAR2CMD_MIN=${PAR2CMD_TIMES[0]}
PAR2CMD_MAX=${PAR2CMD_TIMES[0]}
PAR2RS_MIN=${PAR2RS_TIMES[0]}
PAR2RS_MAX=${PAR2RS_TIMES[0]}

for time in "${PAR2CMD_TIMES[@]}"; do
    if (( $(echo "$time < $PAR2CMD_MIN" | bc -l) )); then
        PAR2CMD_MIN=$time
    fi
    if (( $(echo "$time > $PAR2CMD_MAX" | bc -l) )); then
        PAR2CMD_MAX=$time
    fi
done

for time in "${PAR2RS_TIMES[@]}"; do
    if (( $(echo "$time < $PAR2RS_MIN" | bc -l) )); then
        PAR2RS_MIN=$time
    fi
    if (( $(echo "$time > $PAR2RS_MAX" | bc -l) )); then
        PAR2RS_MAX=$time
    fi
done

# Print results
echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}Results (${ITERATIONS} iterations)${NC}"
echo -e "${BLUE}================================${NC}"
echo ""
echo -e "${YELLOW}par2cmdline:${NC}"
echo "  Average: ${PAR2CMD_AVG}s"
echo "  Min:     ${PAR2CMD_MIN}s"
echo "  Max:     ${PAR2CMD_MAX}s"
echo ""
echo -e "${YELLOW}par2rs:${NC}"
echo "  Average: ${PAR2RS_AVG}s"
echo "  Min:     ${PAR2RS_MIN}s"
echo "  Max:     ${PAR2RS_MAX}s"
echo ""
echo -e "${GREEN}Speedup: ${SPEEDUP}x${NC}"
echo ""

# Individual times
echo -e "${YELLOW}Individual times:${NC}"
echo "Iteration | par2cmdline | par2rs"
echo "----------|-------------|--------"
for i in $(seq 0 $((ITERATIONS - 1))); do
    printf "%9d | %11ss | %6ss\n" $((i + 1)) "${PAR2CMD_TIMES[$i]}" "${PAR2RS_TIMES[$i]}"
done
echo ""

# Cleanup
rm -rf "$TEMP"

echo -e "${GREEN}✓ All repairs verified correct${NC}"
