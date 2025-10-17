#!/usr/bin/env bash
# Benchmark par2rs repair performance against par2cmdline with averaging
# Runs multiple iterations and computes averages
#
# Usage: 
#   ./benchmark_repair_averaged.sh [size_in_mb] [temp_dir]
#   ./benchmark_repair_averaged.sh -d DIRECTORY -p PAR2_FILE
#
# Examples:
#   ./benchmark_repair_averaged.sh 1000                    # 1GB in /tmp
#   ./benchmark_repair_averaged.sh 10000 /mnt/scratch      # 10GB in /mnt/scratch
#   ./benchmark_repair_averaged.sh -d /path -p file.par2   # Use existing files

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PAR2RS="$PROJECT_ROOT/target/release/par2repair"
PAR2CMDLINE="par2"
ITERATIONS=${ITERATIONS:-10}

# Configuration
USE_EXISTING_DIR=""
PAR2_FILE=""
SIZE_MB=${1:-100}
TEMP_BASE=${2:-}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -d|--directory)
            USE_EXISTING_DIR="$2"
            shift 2
            ;;
        -p|--par2)
            PAR2_FILE="$2"
            shift 2
            ;;
        -i|--iterations)
            ITERATIONS="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS] [SIZE_MB] [TEMP_DIR]"
            echo ""
            echo "Options:"
            echo "  -d, --directory DIR   Use existing directory with data files and PAR2 volumes"
            echo "  -p, --par2 FILE      PAR2 file to repair (required with --directory)"
            echo "  -i, --iterations N   Number of iterations (default: 10)"
            echo "  -h, --help           Show this help message"
            echo ""
            echo "Examples:"
            echo "  $0 1000                              # 1GB test file"
            echo "  $0 10000 /mnt/scratch                # 10GB in /mnt/scratch"
            echo "  $0 -d /path/to/files -p file.par2    # Use existing files"
            exit 0
            ;;
        *)
            if [[ "$1" =~ ^[0-9]+$ ]]; then
                SIZE_MB=$1
                shift
                if [[ $# -gt 0 ]] && [[ ! "$1" =~ ^- ]]; then
                    TEMP_BASE=$1
                    shift
                fi
            else
                echo "Unknown option: $1"
                echo "Use --help for usage information"
                exit 1
            fi
            ;;
    esac
done

# Validate directory mode arguments
if [ -n "$USE_EXISTING_DIR" ]; then
    if [ -z "$PAR2_FILE" ]; then
        echo "Error: --par2 is required when using --directory"
        exit 1
    fi
    if [ ! -d "$USE_EXISTING_DIR" ]; then
        echo "Error: Directory not found: $USE_EXISTING_DIR"
        exit 1
    fi
    if [ ! -f "$USE_EXISTING_DIR/$PAR2_FILE" ]; then
        echo "Error: PAR2 file not found: $USE_EXISTING_DIR/$PAR2_FILE"
        exit 1
    fi
fi

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}PAR2 Repair Benchmark (Averaged)${NC}"
if [ -n "$USE_EXISTING_DIR" ]; then
    echo -e "${BLUE}Testing directory: $USE_EXISTING_DIR${NC}"
    echo -e "${BLUE}PAR2 file: $PAR2_FILE${NC}"
else
    echo -e "${BLUE}Testing ${SIZE_MB}MB file repair${NC}"
    if [ -n "$TEMP_BASE" ]; then
        echo -e "${BLUE}Temp directory: $TEMP_BASE${NC}"
    fi
fi
echo -e "${BLUE}Iterations: $ITERATIONS${NC}"
echo -e "${BLUE}================================${NC}"
echo ""

# Build par2rs first
echo -e "${YELLOW}Building par2rs...${NC}"
cd "$PROJECT_ROOT"
cargo build --release 2>&1 | grep -E "(Compiling par2rs|Finished|error)" || true
echo ""

if [ -n "$USE_EXISTING_DIR" ]; then
    # Use existing directory mode
    echo -e "${YELLOW}Using existing directory: $USE_EXISTING_DIR${NC}"
    echo -e "${YELLOW}PAR2 file: $PAR2_FILE${NC}"
    echo ""
    
    # Extract data file name from PAR2 file
    DATA_FILE=$(cd "$USE_EXISTING_DIR" && "$PAR2CMDLINE" v -q "$PAR2_FILE" 2>/dev/null | grep -v "PAR2" | awk '{print $NF}' | head -1 || echo "")
    if [ -z "$DATA_FILE" ]; then
        echo -e "${RED}Error: Could not determine data file from PAR2${NC}"
        exit 1
    fi
    
    echo -e "${YELLOW}Data file: $DATA_FILE${NC}"
    
    # Check if data file exists
    if [ ! -f "$USE_EXISTING_DIR/$DATA_FILE" ]; then
        echo -e "${RED}Error: Data file not found: $USE_EXISTING_DIR/$DATA_FILE${NC}"
        exit 1
    fi
    
    # Get file size
    FILE_SIZE=$(stat -f%z "$USE_EXISTING_DIR/$DATA_FILE" 2>/dev/null || stat -c%s "$USE_EXISTING_DIR/$DATA_FILE")
    SIZE_MB=$((FILE_SIZE / 1024 / 1024))
    echo -e "${YELLOW}File size: ${SIZE_MB}MB${NC}"
    echo ""
    
    # Save original file for restoration
    TEMP=$(mktemp -d)
    cp "$USE_EXISTING_DIR/$DATA_FILE" "$TEMP/original_$DATA_FILE"
    
    # Copy all PAR2 files to temp
    cp "$USE_EXISTING_DIR"/*.par2 "$TEMP/" 2>/dev/null || true
    
    # Compute original MD5
    echo -e "${YELLOW}Computing original file MD5...${NC}"
    MD5_ORIGINAL=$(md5sum "$USE_EXISTING_DIR/$DATA_FILE" | awk '{print $1}')
    echo "Original MD5: $MD5_ORIGINAL"
    echo ""
    
    # Corrupt 1MB at 10% offset for testing
    CORRUPT_OFFSET=$((FILE_SIZE / 10))
    CORRUPT_SIZE=$((1024 * 1024))
    
    # Set TEST_FILE and TEST_PAR2 for iteration loop
    TEST_FILE="$DATA_FILE"
    TEST_PAR2="$PAR2_FILE"
    
else
    # Original generated file mode
    # Arrays to store times (will be populated in directory mode too)
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
    
    # Set TEST_FILE and TEST_PAR2 for iteration loop
    TEST_FILE="testfile_${SIZE_MB}mb"
    TEST_PAR2="testfile_${SIZE_MB}mb.par2"
fi

# Arrays to store times
declare -a PAR2CMD_TIMES
declare -a PAR2RS_TIMES

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
    if ! dd if=/dev/zero of="$TEMP/$TEST_FILE" bs=512 count=1 seek=$((CORRUPT_OFFSET / 512)) conv=notrunc 2>&1; then
        echo -e "${RED}✗ Failed to corrupt file!${NC}"
        exit 1
    fi
    
    echo -e "${YELLOW}  Running par2cmdline repair...${NC}"
    START=$(date +%s.%N)
    if ! $PAR2CMDLINE r -q -N "$TEMP/$TEST_PAR2" 2>&1; then
        echo -e "${RED}✗ par2cmdline repair failed in iteration $i!${NC}"
        exit 1
    fi
    END=$(date +%s.%N)
    PAR2CMD_TIME=$(echo "$END - $START" | bc)
    PAR2CMD_TIMES+=($PAR2CMD_TIME)
    
    # Verify par2cmdline repair
    MD5_PAR2CMD=$(md5sum "$TEMP/$TEST_FILE" | awk '{print $1}')
    
    # === Test par2rs ===
    # Corrupt file again (will be repaired, then used as base for next iteration)
    echo -e "${YELLOW}  Corrupting file for par2rs...${NC}"
    if ! dd if=/dev/zero of="$TEMP/$TEST_FILE" bs=512 count=1 seek=$((CORRUPT_OFFSET / 512)) conv=notrunc 2>&1; then
        echo -e "${RED}✗ Failed to corrupt file!${NC}"
        exit 1
    fi
    
    echo -e "${YELLOW}  Running par2rs repair...${NC}"
    START=$(date +%s.%N)
    if ! $PAR2RS "$TEMP/$TEST_PAR2" 2>&1; then
        echo -e "${RED}✗ par2rs repair failed in iteration $i!${NC}"
        exit 1
    fi
    END=$(date +%s.%N)
    PAR2RS_TIME=$(echo "$END - $START" | bc)
    PAR2RS_TIMES+=($PAR2RS_TIME)
    
    # Verify par2rs repair
    MD5_PAR2RS=$(md5sum "$TEMP/$TEST_FILE" | awk '{print $1}')
    
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
if [ -n "$USE_EXISTING_DIR" ]; then
    # Restore original file
    cp "$TEMP/original_$DATA_FILE" "$USE_EXISTING_DIR/$DATA_FILE"
    rm -rf "$TEMP"
    echo -e "${YELLOW}Restored original file${NC}"
else
    rm -rf "$TEMP"
fi

echo -e "${GREEN}✓ All repairs verified correct${NC}"
