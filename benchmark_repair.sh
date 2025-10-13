#!/usr/bin/env bash
# Benchmark par2rs repair performance against par2cmdline
# Tests with 100MB files

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"
PAR2RS="$PROJECT_ROOT/target/release/par2repair"
PAR2CMDLINE="par2"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}PAR2 Repair Benchmark${NC}"
echo -e "${BLUE}Testing 100MB file repair${NC}"
echo -e "${BLUE}================================${NC}"
echo ""

# Build par2rs first
echo -e "${YELLOW}Building par2rs...${NC}"
cd "$PROJECT_ROOT"
cargo build --release 2>&1 | grep -E "(Compiling par2rs|Finished|error)" || true
echo ""

# Create test file
TEMP=$(mktemp -d)
echo -e "${YELLOW}Creating 100MB test file in $TEMP...${NC}"
dd if=/dev/urandom of="$TEMP/testfile_100mb" bs=1M count=100 2>&1 | tail -2
echo ""

# Create PAR2 files
echo -e "${YELLOW}Creating PAR2 files with 5% redundancy...${NC}"
$PAR2CMDLINE c -r5 "$TEMP/testfile_100mb.par2" "$TEMP/testfile_100mb" 2>&1 | tail -5
echo ""

# Save original MD5 before corruption
echo -e "${YELLOW}Computing original file MD5...${NC}"
MD5_ORIGINAL=$(md5sum "$TEMP/testfile_100mb" | awk '{print $1}')
echo "Original MD5: $MD5_ORIGINAL"
echo ""

# Corrupt the file (1MB at offset 50MB - small enough that 5% recovery can fix it)
echo -e "${YELLOW}Corrupting 1MB of data (offset 50MB)...${NC}"
dd if=/dev/zero of="$TEMP/testfile_100mb" bs=1M count=1 seek=50 conv=notrunc 2>&1 | tail -1
MD5_CORRUPTED=$(md5sum "$TEMP/testfile_100mb" | awk '{print $1}')
echo "Corrupted MD5: $MD5_CORRUPTED"
echo ""

# Benchmark par2cmdline
echo -e "${GREEN}=== Benchmarking par2cmdline ===${NC}"
TEMP_PAR2CMD=$(mktemp -d)
cp "$TEMP/testfile_100mb" "$TEMP_PAR2CMD/"
cp "$TEMP"/*.par2 "$TEMP_PAR2CMD/"
cd "$TEMP_PAR2CMD"
(time $PAR2CMDLINE r testfile_100mb.par2 2>&1) 2>&1 | tail -15
echo ""

# Benchmark par2rs
echo -e "${GREEN}=== Benchmarking par2rs ===${NC}"
TEMP_PAR2RS=$(mktemp -d)
cp "$TEMP/testfile_100mb" "$TEMP_PAR2RS/"
cp "$TEMP"/*.par2 "$TEMP_PAR2RS/"
cd "$TEMP_PAR2RS"
(time $PAR2RS testfile_100mb.par2 2>&1) 2>&1 | tail -15
echo ""

# Generate flamegraph
echo -e "${GREEN}=== Generating flamegraph ===${NC}"
TEMP_FLAMEGRAPH=$(mktemp -d)
cp "$TEMP/testfile_100mb" "$TEMP_FLAMEGRAPH/"
cp "$TEMP"/*.par2 "$TEMP_FLAMEGRAPH/"
cd "$PROJECT_ROOT"
echo "Running cargo flamegraph..."
cargo flamegraph --root --bin par2repair -- "$TEMP_FLAMEGRAPH/testfile_100mb.par2" > /dev/null 2>&1
if [ -f flamegraph.svg ]; then
    echo -e "${GREEN}✓ Flamegraph generated: $PROJECT_ROOT/flamegraph.svg${NC}"
    echo "Open it with: code flamegraph.svg"
else
    echo -e "${YELLOW}⚠ Flamegraph generation failed${NC}"
fi
echo ""

# Verify repairs
echo -e "${YELLOW}Verifying repairs...${NC}"
MD5_PAR2CMD=$(md5sum "$TEMP_PAR2CMD/testfile_100mb" | awk '{print $1}')
MD5_PAR2RS=$(md5sum "$TEMP_PAR2RS/testfile_100mb" | awk '{print $1}')

echo "Original:        $MD5_ORIGINAL"
echo "Corrupted:       $MD5_CORRUPTED"
echo "par2cmdline:     $MD5_PAR2CMD"
echo "par2rs:          $MD5_PAR2RS"
echo ""

if [ "$MD5_PAR2CMD" = "$MD5_ORIGINAL" ] && [ "$MD5_PAR2RS" = "$MD5_ORIGINAL" ]; then
    echo -e "${GREEN}✓ Both repairs restored original file correctly${NC}"
elif [ "$MD5_PAR2CMD" = "$MD5_PAR2RS" ]; then
    echo -e "${YELLOW}⚠ Both tools produced same output but differs from original${NC}"
else
    echo -e "${RED}✗ Repairs produced different outputs!${NC}"
fi

echo ""
echo -e "${BLUE}Cleaning up...${NC}"
rm -rf "$TEMP" "$TEMP_PAR2CMD" "$TEMP_PAR2RS" "$TEMP_FLAMEGRAPH"
echo "Done!"
