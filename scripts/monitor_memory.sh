#!/bin/bash
# Simple memory monitoring for par2repair

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}=== PAR2 Memory Monitor ===${NC}"

# Build release binary
echo -e "${YELLOW}Building release binary...${NC}"
cargo build --release --bin par2repair

# Get test file
TEST_FILE="${1:-tests/fixtures/testfile}"
PAR2_FILE="${TEST_FILE}.par2"

if [ ! -f "$PAR2_FILE" ]; then
    echo -e "${RED}Error: PAR2 file not found: $PAR2_FILE${NC}"
    exit 1
fi

# Create a temporary directory
TEMP_DIR="target/memtest_$(date +%s)"
mkdir -p "$TEMP_DIR"

echo -e "${YELLOW}Copying test files to $TEMP_DIR${NC}"
cp tests/fixtures/testfile* "$TEMP_DIR/"

# Corrupt the file
echo -e "${YELLOW}Corrupting test file...${NC}"
dd if=/dev/zero of="$TEMP_DIR/testfile" bs=1024 count=10 conv=notrunc seek=500 2>/dev/null

echo -e "${GREEN}Running repair with memory monitoring...${NC}"
echo ""

# Use /usr/bin/time to get memory stats
/usr/bin/time -v target/release/par2repair "$TEMP_DIR/testfile.par2" 2>&1 | grep -E "(Maximum resident set size|elapsed|CPU)"

echo ""
echo -e "${GREEN}Test complete!${NC}"
echo -e "${YELLOW}Temp directory: $TEMP_DIR${NC}"

# Cleanup option
read -p "Remove temp directory? (y/n) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    rm -rf "$TEMP_DIR"
    echo -e "${GREEN}Cleaned up${NC}"
fi
