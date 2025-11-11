#!/usr/bin/env bash
# Debug script to investigate intermittent creation failures
# Creates PAR2 file with logging and then verifies it

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PAR2RS="$PROJECT_ROOT/target/release/par2"

# File parameters
FILE_SIZE_MB=1000
BLOCK_SIZE_KB=1536
REDUNDANCY_PCT=5

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}PAR2 Creation Debug${NC}"
echo -e "${BLUE}File Size: ${FILE_SIZE_MB}MB${NC}"
echo -e "${BLUE}Block Size: ${BLOCK_SIZE_KB}KB${NC}"
echo -e "${BLUE}================================${NC}"
echo ""

# Create temp directory
TEMP=$(mktemp -d)
echo "Working directory: $TEMP"
echo ""

# Cleanup on exit
cleanup() {
    echo ""
    echo "Temporary files preserved in: $TEMP"
    echo "To clean up: rm -rf $TEMP"
}
trap cleanup EXIT

# Create test file
echo -e "${YELLOW}Creating test file...${NC}"
dd if=/dev/urandom of="$TEMP/testfile.dat" bs=1M count=$FILE_SIZE_MB 2>&1 | tail -2
echo ""

# Calculate MD5 of original file for reference
echo -e "${YELLOW}Computing original file MD5...${NC}"
ORIG_MD5=$(md5sum "$TEMP/testfile.dat" | awk '{print $1}')
echo "Original MD5: $ORIG_MD5"
echo ""

# Calculate block size in bytes
BLOCK_SIZE_BYTES=$((BLOCK_SIZE_KB * 1024))

# Create PAR2 file with par2rs
echo -e "${YELLOW}Creating PAR2 with par2rs...${NC}"
RUST_LOG=par2rs=debug $PAR2RS c -s$BLOCK_SIZE_BYTES -r$REDUNDANCY_PCT "$TEMP/testfile.par2" "$TEMP/testfile.dat" 2>&1 | tee "$TEMP/create_log.txt"
echo ""

# List created files
echo -e "${YELLOW}Created files:${NC}"
ls -lh "$TEMP"/*.par2 2>/dev/null || echo "No PAR2 files created!"
echo ""

# Verify with par2rs (verbose)
echo -e "${YELLOW}Verifying with par2rs (verbose)...${NC}"
RUST_LOG=par2rs=debug $PAR2RS v "$TEMP/testfile.par2" 2>&1 | tee "$TEMP/verify_log.txt"
VERIFY_EXIT=$?
echo ""

if [ $VERIFY_EXIT -eq 0 ]; then
    echo -e "${GREEN}SUCCESS: Verification passed${NC}"
else
    echo -e "${RED}FAILURE: Verification failed (exit code: $VERIFY_EXIT)${NC}"
    echo ""
    echo -e "${YELLOW}Analyzing failure...${NC}"
    
    # Check if any blocks matched
    echo "Blocks found:"
    grep -i "block.*found\|matched" "$TEMP/verify_log.txt" || echo "  No matches found"
    
    echo ""
    echo "File size verification:"
    ls -lh "$TEMP/testfile.dat"
    
    # Check current file MD5
    CURRENT_MD5=$(md5sum "$TEMP/testfile.dat" | awk '{print $1}')
    echo "Current file MD5: $CURRENT_MD5"
    if [ "$ORIG_MD5" != "$CURRENT_MD5" ]; then
        echo -e "${RED}ERROR: File was modified!${NC}"
    fi
fi

echo ""
echo "Logs saved to:"
echo "  Creation: $TEMP/create_log.txt"
echo "  Verification: $TEMP/verify_log.txt"
