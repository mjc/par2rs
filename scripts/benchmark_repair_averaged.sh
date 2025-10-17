#!/usr/bin/env bash
# Benchmark par2rs repair performance against par2cmdline with averaging
# Runs 10 iterations and computes averages
# Usage: ./benchmark_repair_averaged.sh [size_in_mb] [temp_dir]
# Example: ./benchmark_repair_averaged.sh 1000  # for 1GB in /tmp
# Example: ./benchmark_repair_averaged.sh 10000 /mnt/scratch  # for 10GB in /mnt/scratch

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PAR2RS="$PROJECT_ROOT/target/release/par2repair"
PAR2CMDLINE="par2"
ITERATIONS=${ITERATIONS:-10}
SIZE_MB=${1:-100}  # Default to 100MB if no argument provided
TEMP_BASE=${2:-}   # Optional: base directory for temp files

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}PAR2 Repair Benchmark (Averaged)${NC}"
echo -e "${BLUE}Testing ${SIZE_MB}MB file repair${NC}"
echo -e "${BLUE}Iterations: $ITERATIONS${NC}"
if [ -n "$TEMP_BASE" ]; then
    echo -e "${BLUE}Temp directory: $TEMP_BASE${NC}"
fi
echo -e "${BLUE}================================${NC}"
echo ""

# Build par2rs first
echo -e "${YELLOW}Building par2rs...${NC}"
cd "$PROJECT_ROOT"
cargo build --release 2>&1 | grep -E "(Compiling par2rs|Finished|error)" || true
echo ""

# Arrays to store times
declare -a PAR2CMD_TIMES
declare -a PAR2RS_TIMES

# Corrupt 512 bytes at midpoint of file
CORRUPT_OFFSET=$((SIZE_MB * 1024 * 1024 / 2))

# Generate test file once for all iterations
if [ -n "$TEMP_BASE" ]; then
    TEMP=$(mktemp -d -p "$TEMP_BASE")
else
    TEMP=$(mktemp -d)
fi
echo -e "${YELLOW}Generating ${SIZE_MB}MB test file in $TEMP...${NC}"
if ! dd if=/dev/urandom of="$TEMP/testfile_${SIZE_MB}mb" bs=1M count=${SIZE_MB} 2>&1; then
    echo -e "${RED}✗ Failed to generate test file!${NC}"
    echo "This could be due to insufficient disk space."
    df -h "$TEMP"
    exit 1
fi
echo ""

# Create PAR2 files once for all iterations
echo -e "${YELLOW}Creating PAR2 files...${NC}"
# Capture output to check for errors but suppress verbose messages
PAR2_OUTPUT=$($PAR2CMDLINE c -q -r5 "$TEMP/testfile_${SIZE_MB}mb.par2" "$TEMP/testfile_${SIZE_MB}mb" 2>&1)
PAR2_EXIT_CODE=$?
if [ $PAR2_EXIT_CODE -ne 0 ]; then
    echo -e "${RED}✗ Failed to create PAR2 files!${NC}"
    echo "PAR2 output:"
    echo "$PAR2_OUTPUT"
    echo "This could be due to insufficient disk space, timeout, or other errors."
    # Show what's in the temp directory
    echo "Files in $TEMP:"
    ls -lh "$TEMP"
    exit 1
fi
echo ""

# Verify PAR2 files were created
PAR2_FILE_COUNT=$(ls -1 "$TEMP"/*.par2 2>/dev/null | wc -l)
if [ "$PAR2_FILE_COUNT" -eq 0 ]; then
    echo -e "${RED}✗ No PAR2 files were created!${NC}"
    exit 1
fi
echo "Created $PAR2_FILE_COUNT PAR2 files"

# Save original MD5 for verification
MD5_ORIGINAL=$(md5sum "$TEMP/testfile_${SIZE_MB}mb" | awk '{print $1}')
echo "Original MD5: $MD5_ORIGINAL"
echo ""

for i in $(seq 1 $ITERATIONS); do
    echo -e "${GREEN}=== Iteration $i/$ITERATIONS ===${NC}"
    
    # Verify PAR2 files still exist before iteration
    PAR2_FILE_COUNT=$(ls -1 "$TEMP"/*.par2 2>/dev/null | wc -l)
    if [ "$PAR2_FILE_COUNT" -eq 0 ]; then
        echo -e "${RED}✗ PAR2 files disappeared before iteration $i!${NC}"
        echo "Files in $TEMP:"
        ls -lh "$TEMP"
        exit 1
    fi
    echo "  PAR2 files present: $PAR2_FILE_COUNT"
    
    # === Test par2cmdline ===
    # Corrupt file (will be repaired, then used as base for next iteration)
    echo -e "${YELLOW}  Corrupting file for par2cmdline...${NC}"
    if ! dd if=/dev/zero of="$TEMP/testfile_${SIZE_MB}mb" bs=512 count=1 seek=$((CORRUPT_OFFSET / 512)) conv=notrunc 2>&1; then
        echo -e "${RED}✗ Failed to corrupt file!${NC}"
        exit 1
    fi
    
    echo -e "${YELLOW}  Running par2cmdline repair...${NC}"
    START=$(date +%s.%N)
    if ! $PAR2CMDLINE r -q -N "$TEMP/testfile_${SIZE_MB}mb.par2" 2>&1; then
        echo -e "${RED}✗ par2cmdline repair failed in iteration $i!${NC}"
        exit 1
    fi
    END=$(date +%s.%N)
    PAR2CMD_TIME=$(echo "$END - $START" | bc)
    PAR2CMD_TIMES+=($PAR2CMD_TIME)
    
    # Verify par2cmdline repair
    MD5_PAR2CMD=$(md5sum "$TEMP/testfile_${SIZE_MB}mb" | awk '{print $1}')
    
    # === Test par2rs ===
    # Corrupt file again (will be repaired, then used as base for next iteration)
    echo -e "${YELLOW}  Corrupting file for par2rs...${NC}"
    if ! dd if=/dev/zero of="$TEMP/testfile_${SIZE_MB}mb" bs=512 count=1 seek=$((CORRUPT_OFFSET / 512)) conv=notrunc 2>&1; then
        echo -e "${RED}✗ Failed to corrupt file!${NC}"
        exit 1
    fi
    
    echo -e "${YELLOW}  Running par2rs repair...${NC}"
    START=$(date +%s.%N)
    if ! $PAR2RS "$TEMP/testfile_${SIZE_MB}mb.par2" 2>&1; then
        echo -e "${RED}✗ par2rs repair failed in iteration $i!${NC}"
        exit 1
    fi
    END=$(date +%s.%N)
    PAR2RS_TIME=$(echo "$END - $START" | bc)
    PAR2RS_TIMES+=($PAR2RS_TIME)
    
    # Verify par2rs repair
    MD5_PAR2RS=$(md5sum "$TEMP/testfile_${SIZE_MB}mb" | awk '{print $1}')
    
    echo "  par2cmdline: ${PAR2CMD_TIME}s"
    echo "  par2rs:      ${PAR2RS_TIME}s"
    
    # Verify correctness
    if [ "$MD5_PAR2CMD" != "$MD5_ORIGINAL" ] || [ "$MD5_PAR2RS" != "$MD5_ORIGINAL" ]; then
        echo -e "${RED}✗ Repair verification failed in iteration $i!${NC}"
        echo "  Expected: $MD5_ORIGINAL"
        echo "  par2cmdline: $MD5_PAR2CMD"
        echo "  par2rs: $MD5_PAR2RS"
        exit 1
    fi
    
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
