#!/usr/bin/env bash
# Benchmark par2rs creation performance against par2cmdline
# Tests PAR2 file creation with various redundancy levels and file sizes

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PAR2RS="$PROJECT_ROOT/target/release/par2"
PAR2CMDLINE="par2"

# Temporary directories to clean up
TEMP=""

# Cleanup function
cleanup() {
    echo ""
    echo -e "${BLUE}Cleaning up temporary files...${NC}"
    [ -n "$TEMP" ] && rm -rf "$TEMP" 2>/dev/null || true
    echo "Done!"
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
echo -e "${BLUE}PAR2 Creation Benchmark${NC}"
echo -e "${BLUE}Testing creation with various redundancy levels${NC}"
echo -e "${BLUE}================================${NC}"
echo ""

# Build par2rs first
echo -e "${YELLOW}Building par2rs...${NC}"
cd "$PROJECT_ROOT"
cargo build --release 2>&1 | grep -E "(Compiling par2rs|Finished|error)" || true
echo ""

# Create test directory
TEMP=$(mktemp -d)
echo -e "${YELLOW}Creating test files in $TEMP...${NC}"

# Test 1: Single 10MB file
echo -e "${YELLOW}Test 1: 10MB file with 5% redundancy${NC}"
dd if=/dev/urandom of="$TEMP/test_10mb.dat" bs=1M count=10 2>&1 | tail -2
echo ""

echo -e "${GREEN}=== par2cmdline creation ===${NC}"
(time $PAR2CMDLINE c -r5 "$TEMP/test_10mb_par2cmd.par2" "$TEMP/test_10mb.dat" 2>&1) 2>&1 | tail -10
echo ""

echo -e "${GREEN}=== par2rs creation ===${NC}"
(time $PAR2RS c -r5 "$TEMP/test_10mb_par2rs.par2" "$TEMP/test_10mb.dat" 2>&1) 2>&1 | tail -10
echo ""

# Test 2: Single 10MB file with 10% redundancy
echo -e "${YELLOW}Test 2: 10MB file with 10% redundancy${NC}"
echo -e "${GREEN}=== par2cmdline creation ===${NC}"
(time $PAR2CMDLINE c -r10 "$TEMP/test_10mb_r10_par2cmd.par2" "$TEMP/test_10mb.dat" 2>&1) 2>&1 | tail -10
echo ""

echo -e "${GREEN}=== par2rs creation ===${NC}"
(time $PAR2RS c -r10 "$TEMP/test_10mb_r10_par2rs.par2" "$TEMP/test_10mb.dat" 2>&1) 2>&1 | tail -10
echo ""

# Test 3: Multiple files
echo -e "${YELLOW}Test 3: Multiple files (5 x 2MB) with 5% redundancy${NC}"
for i in {1..5}; do
    dd if=/dev/urandom of="$TEMP/multi_$i.dat" bs=1M count=2 2>&1 | tail -1
done
echo ""

echo -e "${GREEN}=== par2cmdline creation (multifile) ===${NC}"
(time $PAR2CMDLINE c -r5 "$TEMP/multi_par2cmd.par2" "$TEMP/multi_"*.dat 2>&1) 2>&1 | tail -10
echo ""

echo -e "${GREEN}=== par2rs creation (multifile) ===${NC}"
(time $PAR2RS c -r5 "$TEMP/multi_par2rs.par2" "$TEMP/multi_"*.dat 2>&1) 2>&1 | tail -10
echo ""

echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}Benchmark Complete${NC}"
echo -e "${BLUE}================================${NC}"
