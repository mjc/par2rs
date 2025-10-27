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

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Configuration
PAR2_FILE=""
SIZE_MB=100  # Default size
TEMP_BASE=""
MULTIFILE=false

# Functions
show_help() {
    cat << EOF
Usage: $0 [OPTIONS] [SIZE_MB] [TEMP_DIR]

Options:
  -p, --par2 FILE      PAR2 file path to repair (infers directory automatically)
  -i, --iterations N   Number of iterations (default: 10)
                       When N=1, flamegraph profiling is enabled automatically
  -m, --multifile      Create multiple files of varying sizes for PAR2 set
  -h, --help           Show this help message

Examples:
  $0 1000                                    # 1GB test file
  $0 10000 /mnt/scratch                      # 10GB in /mnt/scratch
  $0 -m 1000                                 # Multiple files totaling ~1GB
  $0 -p /path/to/files/file.par2             # Use existing files
  $0 -p file.par2 -i 5                       # 5 iterations on existing
  $0 -p file.par2 -i 1                       # Single run with flamegraph

Note: When iterations=1, flamegraph profiling is automatically enabled.
      Install with: cargo install flamegraph
EOF
    exit 0
}

error_exit() {
    echo -e "${RED}✗ $1${NC}" >&2
    exit 1
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -p|--par2)
            PAR2_FILE="$2"
            shift 2
            ;;
        -i|--iterations)
            ITERATIONS="$2"
            shift 2
            ;;
        -m|--multifile)
            MULTIFILE=true
            shift
            ;;
        -h|--help)
            show_help
            ;;
        -*)
            error_exit "Unknown option: $1. Use --help for usage information"
            ;;
        *)
            # Positional argument - size
            if [[ "$1" =~ ^[0-9]+$ ]]; then
                SIZE_MB=$1
                shift
                # Next positional arg is temp directory (if not a flag)
                if [[ $# -gt 0 ]] && [[ ! "$1" =~ ^- ]]; then
                    TEMP_BASE=$1
                    shift
                fi
            else
                error_exit "Invalid argument: $1. Use --help for usage information"
            fi
            ;;
    esac
done

validate_par2_file() {
    [ -n "$PAR2_FILE" ] || return 0
    
    [ -f "$PAR2_FILE" ] || error_exit "PAR2 file not found: $PAR2_FILE"
    
    USE_EXISTING_DIR=$(dirname "$PAR2_FILE")
    PAR2_FILE=$(basename "$PAR2_FILE")
    
    echo "Inferred directory: $USE_EXISTING_DIR"
    echo "PAR2 filename: $PAR2_FILE"
    
    [ -d "$USE_EXISTING_DIR" ] || error_exit "Directory not found: $USE_EXISTING_DIR"
    [ -f "$USE_EXISTING_DIR/$PAR2_FILE" ] || error_exit "PAR2 file not found: $USE_EXISTING_DIR/$PAR2_FILE"
}

validate_par2_file

create_multifile_set() {
    echo -e "${YELLOW}Generating multiple test files totaling ~${SIZE_MB}MB in $TEMP...${NC}"
    
    local large_mb=$((SIZE_MB / 2))
    local medium_mb=$((SIZE_MB * 3 / 10))
    local small_mb=$((SIZE_MB * 15 / 100))
    
    echo "  Creating large file (${large_mb}MB)..."
    dd if=/dev/urandom of="$TEMP/large_file.bin" bs=1M count=${large_mb} 2>&1 | grep -v records
    
    echo "  Creating medium file (${medium_mb}MB)..."
    dd if=/dev/urandom of="$TEMP/medium_file.bin" bs=1M count=${medium_mb} 2>&1 | grep -v records
    
    echo "  Creating small file (${small_mb}MB)..."
    dd if=/dev/urandom of="$TEMP/small_file.bin" bs=1M count=${small_mb} 2>&1 | grep -v records
    
    echo "  Creating tiny file (32KB - less than one block)..."
    dd if=/dev/urandom of="$TEMP/tiny_file.bin" bs=1K count=32 2>&1 | grep -v records
    
    FILES_PATTERN="$TEMP/*.bin"
    CORRUPT_FILE="$TEMP/large_file.bin"
    CORRUPT_OFFSET=$((large_mb * 1024 * 1024 / 2))
    TINY_FILE="$TEMP/tiny_file.bin"
}

create_single_file() {
    echo -e "${YELLOW}Generating ${SIZE_MB}MB test file in $TEMP...${NC}"
    dd if=/dev/urandom of="$TEMP/testfile_${SIZE_MB}mb" bs=1M count=${SIZE_MB} 2>&1 || \
        error_exit "Failed to generate test file! Check disk space."
    
    FILES_PATTERN="$TEMP/testfile_${SIZE_MB}mb"
    CORRUPT_FILE="$TEMP/testfile_${SIZE_MB}mb"
    CORRUPT_OFFSET=$((SIZE_MB * 1024 * 1024 / 2))
}

create_par2_archives() {
    echo -e "${YELLOW}Creating PAR2 files...${NC}"
    
    local par2_output
    if [ "$MULTIFILE" = true ]; then
        par2_output=$($PAR2CMDLINE c -q -r5 "$TEMP/multifile.par2" $FILES_PATTERN 2>&1)
    else
        par2_output=$($PAR2CMDLINE c -q -r5 "$TEMP/testfile_${SIZE_MB}mb.par2" "$TEMP/testfile_${SIZE_MB}mb" 2>&1)
    fi
    
    if [ $? -ne 0 ]; then
        echo -e "${RED}✗ Failed to create PAR2 files!${NC}"
        echo "PAR2 output: $par2_output"
        ls -lh "$TEMP"
        exit 1
    fi
    
    local par2_count=$(ls -1 "$TEMP"/*.par2 2>/dev/null | wc -l)
    [ "$par2_count" -eq 0 ] && error_exit "No PAR2 files were created!"
    echo "Created $par2_count PAR2 files"
}

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
    
    # Use par2 verify with timeout (it can be slow on large sets)
    echo -e "${YELLOW}Analyzing PAR2 set (this may take a moment for large sets)...${NC}"
    
    # Run with timeout of 5 minutes
    PAR2_VERIFY_EXIT=0
    if ! PAR2_VERIFY_OUTPUT=$(timeout 300 "$PAR2CMDLINE" v "$USE_EXISTING_DIR/$PAR2_FILE" 2>&1); then
        PAR2_VERIFY_EXIT=$?
        if [ $PAR2_VERIFY_EXIT -eq 124 ]; then
            echo -e "${RED}Error: par2 verify timed out after 5 minutes${NC}"
            echo -e "${RED}The PAR2 set may be too large. Try a smaller test set.${NC}"
            exit 1
        elif [ $PAR2_VERIFY_EXIT -ne 1 ] && [ $PAR2_VERIFY_EXIT -ne 0 ]; then
            # Exit code 0 is success (no repair needed), 1 is normal (repair needed), anything else is an error
            echo -e "${RED}Error: par2 verify failed with exit code $PAR2_VERIFY_EXIT${NC}"
            echo "Output:"
            echo "$PAR2_VERIFY_OUTPUT"
            exit 1
        fi
    fi
    
    # Extract data file names from PAR2 file (lines starting with "Target:" or "File:")
    # Format: Target: "filename" - found.
    # Format: File: "filename" - no data found.
    DATA_FILES=$(echo "$PAR2_VERIFY_OUTPUT" | grep -E "^(Target|File):" | sed 's/^.*: "\(.*\)" - .*/\1/' || echo "")
    
    if [ -z "$DATA_FILES" ]; then
        echo -e "${RED}Error: Could not determine data files from PAR2${NC}"
        echo "Verify output:"
        echo "$PAR2_VERIFY_OUTPUT"
        exit 1
    fi
    
    # Count number of data files
    NUM_FILES=$(echo "$DATA_FILES" | wc -l)
    echo -e "${YELLOW}Found $NUM_FILES data file(s) protected by PAR2${NC}"
    
    # For multi-file sets, pick one file that exists to corrupt for benchmarking
    if [ "$NUM_FILES" -gt 1 ]; then
        echo -e "${YELLOW}Multi-file PAR2 set detected${NC}"
        echo -e "${YELLOW}Will select one file to corrupt for benchmarking${NC}"
        echo ""
        
        # Find files that exist and are not missing
        EXISTING_FILES=""
        while IFS= read -r file; do
            if [ -f "$USE_EXISTING_DIR/$file" ]; then
                EXISTING_FILES="$EXISTING_FILES$file"$'\n'
            fi
        done <<< "$DATA_FILES"
        
        if [ -z "$EXISTING_FILES" ]; then
            echo -e "${RED}Error: No existing files found to corrupt${NC}"
            exit 1
        fi
        
        # Pick a random existing file
        DATA_FILE=$(echo "$EXISTING_FILES" | grep -v '^$' | shuf -n 1)
        echo -e "${YELLOW}Selected file: $DATA_FILE${NC}"
    else
        DATA_FILE="$DATA_FILES"
        echo -e "${YELLOW}Data file: $DATA_FILE${NC}"
    fi
    
    # Check if selected data file exists
    if [ ! -f "$USE_EXISTING_DIR/$DATA_FILE" ]; then
        echo -e "${RED}Error: Data file not found: $USE_EXISTING_DIR/$DATA_FILE${NC}"
        exit 1
    fi
    
    # Get file size
    FILE_SIZE=$(stat -f%z "$USE_EXISTING_DIR/$DATA_FILE" 2>/dev/null || stat -c%s "$USE_EXISTING_DIR/$DATA_FILE")
    SIZE_MB=$((FILE_SIZE / 1024 / 1024))
    echo -e "${YELLOW}File size: ${SIZE_MB}MB${NC}"
    echo ""
    
    # Check verify output to see if repair is needed
    if echo "$PAR2_VERIFY_OUTPUT" | grep -q "Repair is not required"; then
        echo -e "${GREEN}✓ All files verified correct${NC}"
        NEED_INITIAL_REPAIR=false
    elif echo "$PAR2_VERIFY_OUTPUT" | grep -q "Repair is required"; then
        echo -e "${YELLOW}⚠ Some files need repair - will repair before benchmarking${NC}"
        NEED_INITIAL_REPAIR=true
    elif echo "$PAR2_VERIFY_OUTPUT" | grep -q "All files are correct"; then
        echo -e "${GREEN}✓ All files verified correct${NC}"
        NEED_INITIAL_REPAIR=false
    else
        # Assume repair needed if we can't determine
        echo -e "${YELLOW}⚠ Cannot determine repair status - will repair before benchmarking${NC}"
        NEED_INITIAL_REPAIR=true
    fi
    
    # If files need repair, repair them first using par2cmdline
    if [ "$NEED_INITIAL_REPAIR" = true ]; then
        echo -e "${YELLOW}Repairing files before benchmarking...${NC}"
        if ! "$PAR2CMDLINE" r "$USE_EXISTING_DIR/$PAR2_FILE" 2>&1 | tail -20; then
            echo -e "${RED}Error: Failed to repair files${NC}"
            exit 1
        fi
        echo -e "${GREEN}✓ Files repaired${NC}"
        echo ""
    fi
    
    # Compute original MD5 of the file we'll corrupt
    echo -e "${YELLOW}Computing original file MD5...${NC}"
    MD5_ORIGINAL=$(md5sum "$USE_EXISTING_DIR/$DATA_FILE" | awk '{print $1}')
    echo "Original MD5: $MD5_ORIGINAL"
    echo ""
    
    # Working directory is the existing directory (no temp needed)
    TEMP="$USE_EXISTING_DIR"
    
    # Corrupt 1MB at 10% offset for testing
    CORRUPT_OFFSET=$((FILE_SIZE / 10))
    CORRUPT_SIZE=$((1024 * 1024))
    
    # Set TEST_FILE and TEST_PAR2 for iteration loop
    TEST_FILE="$DATA_FILE"
    TEST_PAR2="$PAR2_FILE"
    
    # Arrays to store times
    declare -a PAR2CMD_TIMES
    declare -a PAR2RS_TIMES
    
else
    # Original generated file mode
    
    # Generate test file once for all iterations
    if [ -n "$TEMP_BASE" ]; then
        TEMP=$(mktemp -d -p "$TEMP_BASE")
    else
        TEMP=$(mktemp -d)
    fi
    
    if [ "$MULTIFILE" = true ]; then
        create_multifile_set
    else
        create_single_file
    fi
    echo ""
    
    create_par2_archives
    echo ""
    
    # Save original MD5 for verification
    if [ "$MULTIFILE" = true ]; then
        # Verify large file and save tiny file MD5 for later
        MD5_ORIGINAL=$(md5sum "$CORRUPT_FILE" | awk '{print $1}')
        MD5_TINY=$(md5sum "$TINY_FILE" | awk '{print $1}')
        echo "Original MD5 (large file): $MD5_ORIGINAL"
        echo "Original MD5 (tiny file): $MD5_TINY"
    else
        MD5_ORIGINAL=$(md5sum "$TEMP/testfile_${SIZE_MB}mb" | awk '{print $1}')
        echo "Original MD5: $MD5_ORIGINAL"
    fi
    echo ""
    
    # Set TEST_FILE and TEST_PAR2 for iteration loop
    if [ "$MULTIFILE" = true ]; then
        TEST_FILE=$(basename "$CORRUPT_FILE")
        TEST_PAR2="multifile.par2"
    else
        TEST_FILE="testfile_${SIZE_MB}mb"
        TEST_PAR2="testfile_${SIZE_MB}mb.par2"
    fi
fi

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
    
    # In multifile mode, also delete the tiny file to test complete file recovery
    if [ "$MULTIFILE" = true ]; then
        echo -e "${YELLOW}  Deleting tiny file for par2cmdline...${NC}"
        rm -f "$TINY_FILE"
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
    
    # Remove backup files created by par2cmdline (without -p flag which also removes PAR2 files)
    rm -f "$TEMP/$TEST_FILE".1 "$TEMP/$TEST_FILE".bak
    
    # Verify par2cmdline repair
    MD5_PAR2CMD=$(md5sum "$TEMP/$TEST_FILE" | awk '{print $1}')
    
    # === Test par2rs ===
    # Corrupt file again (will be repaired, then used as base for next iteration)
    echo -e "${YELLOW}  Corrupting file for par2rs...${NC}"
    if ! dd if=/dev/zero of="$TEMP/$TEST_FILE" bs=512 count=1 seek=$((CORRUPT_OFFSET / 512)) conv=notrunc 2>&1; then
        echo -e "${RED}✗ Failed to corrupt file!${NC}"
        exit 1
    fi
    
    # In multifile mode, also delete the tiny file to test complete file recovery
    if [ "$MULTIFILE" = true ]; then
        echo -e "${YELLOW}  Deleting tiny file for par2rs...${NC}"
        rm -f "$TINY_FILE"
    fi
    
    echo -e "${YELLOW}  Running par2rs repair...${NC}"
    START=$(date +%s.%N)
    
    # If only 1 iteration, use flamegraph for profiling
    if [ "$ITERATIONS" -eq 1 ]; then
        echo -e "${YELLOW}  Single iteration detected - running with flamegraph profiling${NC}"
        FLAMEGRAPH_OUTPUT="$TEMP/flamegraph.svg"
        if command -v cargo-flamegraph >/dev/null 2>&1; then
            # Run with flamegraph
            cd "$PROJECT_ROOT"
            if ! cargo flamegraph --output="$FLAMEGRAPH_OUTPUT" --root -- "$TEMP/$TEST_PAR2" 2>&1; then
                echo -e "${RED}✗ par2rs repair (with flamegraph) failed in iteration $i!${NC}"
                exit 1
            fi
            echo -e "${GREEN}  ✓ Flamegraph saved to: $FLAMEGRAPH_OUTPUT${NC}"
        else
            echo -e "${YELLOW}  Warning: cargo-flamegraph not found, running without profiling${NC}"
            echo -e "${YELLOW}  Install with: cargo install flamegraph${NC}"
            if ! $PAR2RS "$TEMP/$TEST_PAR2" 2>&1; then
                echo -e "${RED}✗ par2rs repair failed in iteration $i!${NC}"
                exit 1
            fi
        fi
    else
        # Normal run without flamegraph
        if ! $PAR2RS "$TEMP/$TEST_PAR2" 2>&1; then
            echo -e "${RED}✗ par2rs repair failed in iteration $i!${NC}"
            exit 1
        fi
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
    
    # In multifile mode, verify tiny file was recovered correctly
    if [ "$MULTIFILE" = true ]; then
        if [ ! -f "$TINY_FILE" ]; then
            echo -e "${RED}✗ Tiny file was not recovered!${NC}"
            exit 1
        fi
        MD5_TINY_RECOVERED=$(md5sum "$TINY_FILE" | awk '{print $1}')
        if [ "$MD5_TINY_RECOVERED" != "$MD5_TINY" ]; then
            echo -e "${RED}✗ Tiny file recovery verification failed in iteration $i!${NC}"
            echo "  Expected: $MD5_TINY"
            echo "  Recovered: $MD5_TINY_RECOVERED"
            exit 1
        fi
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

# Cleanup (only for generated file mode)
if [ -z "$USE_EXISTING_DIR" ]; then
    # Preserve flamegraph if it was generated
    if [ -f "$TEMP/flamegraph.svg" ] && [ "$ITERATIONS" -eq 1 ]; then
        FLAMEGRAPH_DEST="$PROJECT_ROOT/flamegraph_${SIZE_MB}mb.svg"
        mv "$TEMP/flamegraph.svg" "$FLAMEGRAPH_DEST"
        echo -e "${GREEN}✓ Flamegraph saved to: $FLAMEGRAPH_DEST${NC}"
    fi
    rm -rf "$TEMP"
else
    # For existing directory mode, flamegraph stays in the directory
    if [ -f "$TEMP/flamegraph.svg" ] && [ "$ITERATIONS" -eq 1 ]; then
        echo -e "${GREEN}✓ Flamegraph saved to: $TEMP/flamegraph.svg${NC}"
    fi
fi

echo -e "${GREEN}✓ All repairs verified correct${NC}"
