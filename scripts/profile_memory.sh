#!/bin/bash
# Memory profiling script for par2repair

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}=== PAR2 Memory Profiling ===${NC}"

# Build release binary
echo -e "${YELLOW}Building release binary...${NC}"
cargo build --release --bin par2repair

# Check if heaptrack is available
if command -v heaptrack &> /dev/null; then
    echo -e "${GREEN}Using heaptrack for memory profiling${NC}"
    PROFILER="heaptrack"
elif command -v valgrind &> /dev/null; then
    echo -e "${GREEN}Using valgrind massif for memory profiling${NC}"
    PROFILER="valgrind"
else
    echo -e "${RED}No memory profiler found. Please install heaptrack or valgrind:${NC}"
    echo "  Ubuntu/Debian: sudo apt install heaptrack valgrind"
    echo "  Arch: sudo pacman -S heaptrack valgrind"
    exit 1
fi

# Get test file
TEST_FILE="${1:-tests/fixtures/testfile}"
PAR2_FILE="${TEST_FILE}.par2"

if [ ! -f "$PAR2_FILE" ]; then
    echo -e "${RED}Error: PAR2 file not found: $PAR2_FILE${NC}"
    exit 1
fi

# Create a temporary directory for profiling
PROFILE_DIR="target/profile_$(date +%s)"
mkdir -p "$PROFILE_DIR"

echo -e "${YELLOW}Copying test files to $PROFILE_DIR${NC}"
cp tests/fixtures/testfile* "$PROFILE_DIR/"

# Corrupt the file
echo -e "${YELLOW}Corrupting test file...${NC}"
dd if=/dev/zero of="$PROFILE_DIR/testfile" bs=1024 count=1 conv=notrunc seek=100 2>/dev/null

echo -e "${GREEN}Starting memory profile...${NC}"

if [ "$PROFILER" = "heaptrack" ]; then
    heaptrack --output "$PROFILE_DIR/heaptrack.out" \
        target/release/par2repair "$PROFILE_DIR/testfile.par2"
    
    echo -e "${GREEN}Profile complete. Analyzing...${NC}"
    heaptrack --analyze "$PROFILE_DIR/heaptrack.out.gz" | head -100
    
    echo -e "${YELLOW}Full report saved to: $PROFILE_DIR/heaptrack.out.gz${NC}"
    echo -e "${YELLOW}To view full report: heaptrack --analyze $PROFILE_DIR/heaptrack.out.gz${NC}"
    
elif [ "$PROFILER" = "valgrind" ]; then
    valgrind --tool=massif \
        --massif-out-file="$PROFILE_DIR/massif.out" \
        --pages-as-heap=yes \
        --stacks=yes \
        target/release/par2repair "$PROFILE_DIR/testfile.par2"
    
    echo -e "${GREEN}Profile complete. Analyzing...${NC}"
    ms_print "$PROFILE_DIR/massif.out" | head -200
    
    echo -e "${YELLOW}Full report saved to: $PROFILE_DIR/massif.out${NC}"
    echo -e "${YELLOW}To view full report: ms_print $PROFILE_DIR/massif.out${NC}"
fi

echo -e "${GREEN}Profiling complete!${NC}"
