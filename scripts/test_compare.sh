#!/usr/bin/env bash
# Compare par2cmdline and par2rs output side-by-side
# This script runs tests in isolated temp directories to avoid file corruption

set -e

# Get script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"
PAR2RS="$PROJECT_ROOT/target/release/par2repair"
PAR2CMDLINE="par2"
FIXTURES="$PROJECT_ROOT/tests/fixtures"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}PAR2 Comparison Test Suite${NC}"
echo -e "${BLUE}================================${NC}"
echo ""

# Build par2rs first
echo -e "${YELLOW}Building par2rs...${NC}"
cd "$PROJECT_ROOT"
cargo build --release 2>&1 | grep -E "(Compiling par2rs|Finished|error)" || true
echo ""

# Test 1: Verify intact file
echo -e "${GREEN}=== Test 1: Verify intact file ===${NC}"
echo ""

TEMP1=$(mktemp -d)
cp "$FIXTURES/testfile" "$TEMP1/"
cp "$FIXTURES"/*.par2 "$TEMP1/"

echo -e "${YELLOW}par2cmdline:${NC}"
(cd "$TEMP1" && $PAR2CMDLINE r testfile.par2 2>&1) > /tmp/par2cmd_intact.txt
cat /tmp/par2cmd_intact.txt
echo ""

echo -e "${YELLOW}par2rs:${NC}"
(cd "$TEMP1" && $PAR2RS testfile.par2 2>&1) > /tmp/par2rs_intact.txt
cat /tmp/par2rs_intact.txt
echo ""

rm -rf "$TEMP1"

# Test 2: Repair slightly corrupted file
echo -e "${GREEN}=== Test 2: Repair slightly corrupted file ===${NC}"
echo ""

# Create TWO separate temp directories with corrupted files
TEMP2=$(mktemp -d)
cp "$FIXTURES/testfile" "$TEMP2/"
cp "$FIXTURES"/*.par2 "$TEMP2/"
dd if=/dev/zero of="$TEMP2/testfile" bs=1 count=100 seek=1000 conv=notrunc 2>/dev/null

TEMP3=$(mktemp -d)
cp "$FIXTURES/testfile" "$TEMP3/"
cp "$FIXTURES"/*.par2 "$TEMP3/"
dd if=/dev/zero of="$TEMP3/testfile" bs=1 count=100 seek=1000 conv=notrunc 2>/dev/null

echo -e "${YELLOW}par2cmdline:${NC}"
(cd "$TEMP2" && $PAR2CMDLINE r testfile.par2 2>&1) > /tmp/par2cmd_repair.txt
cat /tmp/par2cmd_repair.txt
echo ""

echo -e "${YELLOW}par2rs:${NC}"
echo -e "${YELLOW}par2rs:\033[0m"
(cd "$TEMP3" && $PAR2RS testfile.par2 2>&1 || true) > /tmp/par2rs_repair.txt
cat /tmp/par2rs_repair.txt
echo ""
cat /tmp/par2rs_repair.txt
echo ""

# Verify both repairs succeeded by checking with par2cmdline
echo -e "${BLUE}--- Verification of repairs ---${NC}"
echo -e "${YELLOW}Verifying par2cmdline repair:${NC}"
(cd "$TEMP2" && $PAR2CMDLINE v testfile.par2 2>&1 | grep -E "(Target:|correct)")

echo -e "${YELLOW}Verifying par2rs repair:${NC}"
(cd "$TEMP3" && $PAR2CMDLINE v testfile.par2 2>&1 | grep -E "(Target:|correct)")
echo ""

rm -rf "$TEMP2" "$TEMP3"

# Test 3: Heavily corrupted file
echo -e "${GREEN}=== Test 3: Heavily corrupted file ===${NC}"
echo ""

# Create TWO separate temp directories with heavily corrupted files
TEMP4=$(mktemp -d)
cp "$FIXTURES/testfile" "$TEMP4/"
cp "$FIXTURES"/*.par2 "$TEMP4/"
dd if=/dev/zero of="$TEMP4/testfile" bs=528 count=5 seek=10 conv=notrunc 2>/dev/null

TEMP5=$(mktemp -d)
cp "$FIXTURES/testfile" "$TEMP5/"
cp "$FIXTURES"/*.par2 "$TEMP5/"
dd if=/dev/zero of="$TEMP5/testfile" bs=528 count=5 seek=10 conv=notrunc 2>/dev/null

echo -e "${YELLOW}par2cmdline:${NC}"
(cd "$TEMP4" && $PAR2CMDLINE r testfile.par2 2>&1) > /tmp/par2cmd_heavy.txt
cat /tmp/par2cmd_heavy.txt
echo ""

echo -e "${YELLOW}par2rs:${NC}"
(cd "$TEMP5" && $PAR2RS testfile.par2 2>&1) > /tmp/par2rs_heavy.txt
cat /tmp/par2rs_heavy.txt
echo ""

# Verify both repairs
echo -e "${BLUE}--- Verification of repairs ---${NC}"
echo -e "${YELLOW}Verifying par2cmdline repair:${NC}"
(cd "$TEMP4" && $PAR2CMDLINE v testfile.par2 2>&1 | grep -E "(Target:|correct)")

echo -e "${YELLOW}Verifying par2rs repair:${NC}"
(cd "$TEMP5" && $PAR2CMDLINE v testfile.par2 2>&1 | grep -E "(Target:|correct)")
echo ""

rm -rf "$TEMP4" "$TEMP5"

# Summary comparison
echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}Output Comparison Summary${NC}"
echo -e "${BLUE}================================${NC}"
echo ""

echo -e "${GREEN}Test 1 - Intact file:${NC}"
echo "Lines in par2cmdline output: $(wc -l < /tmp/par2cmd_intact.txt)"
echo "Lines in par2rs output: $(wc -l < /tmp/par2rs_intact.txt)"
echo ""

echo -e "${GREEN}Test 2 - Light corruption:${NC}"
echo "Lines in par2cmdline output: $(wc -l < /tmp/par2cmd_repair.txt)"
echo "Lines in par2rs output: $(wc -l < /tmp/par2rs_repair.txt)"
echo ""

echo -e "${GREEN}Test 3 - Heavy corruption:${NC}"
echo "Lines in par2cmdline output: $(wc -l < /tmp/par2cmd_heavy.txt)"
echo "Lines in par2rs output: $(wc -l < /tmp/par2rs_heavy.txt)"
echo ""

# Offer to show diffs
echo -e "${YELLOW}Output saved to:${NC}"
echo "  /tmp/par2cmd_intact.txt vs /tmp/par2rs_intact.txt"
echo "  /tmp/par2cmd_repair.txt vs /tmp/par2rs_repair.txt"
echo "  /tmp/par2cmd_heavy.txt vs /tmp/par2rs_heavy.txt"
echo ""
echo -e "${YELLOW}To see detailed diffs, run:${NC}"
echo "  diff -u /tmp/par2cmd_intact.txt /tmp/par2rs_intact.txt"
echo "  diff -u /tmp/par2cmd_repair.txt /tmp/par2rs_repair.txt"
echo "  diff -u /tmp/par2cmd_heavy.txt /tmp/par2rs_heavy.txt"
