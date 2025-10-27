#!/bin/bash
# Generate test files and PAR2 archives for multifile repair bug tests
# This script should be run once to create the PAR2 files that will be committed.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "Generating test files..."

# Function to generate file content (just zeros for speed)
generate_file() {
    local filename=$1
    local start_slice=$2
    local num_slices=$3
    local slice_size=32768  # 32KB
    
    echo "  Creating $filename: $num_slices slices (global $start_slice-$((start_slice + num_slices - 1)))"
    
    # Use dd to quickly create a file filled with zeros
    dd if=/dev/zero of="$filename" bs=$slice_size count=$num_slices 2>/dev/null
}

# Clean up any existing files
rm -f file_a.bin file_b.bin file_c.bin *.par2

# Generate small test files - just enough to demonstrate the bug
# file_a.bin: 5 slices (global 0-4) - 160KB
generate_file "file_a.bin" 0 5

# file_b.bin: 3 slices (global 5-7) - 96KB  
generate_file "file_b.bin" 5 3

# file_c.bin: 2 slices (global 8-9) - 64KB (will be deleted in test)
generate_file "file_c.bin" 8 2

echo ""
echo "Generating PAR2 archive with recovery blocks..."

# Check if par2 command exists
if ! command -v par2 &> /dev/null; then
    echo "ERROR: par2 command not found. Please install par2cmdline:"
    echo "  macOS: brew install par2"
    echo "  Linux: apt-get install par2 or yum install par2cmdline"
    exit 1
fi

# Create PAR2 with just enough redundancy for the test
# Force block size with -s (slice size)
# -s32768 = 32KB slices, giving us exactly 10 blocks total
# -c5 = create 5 recovery blocks
par2 c -s32768 -c5 multifile.par2 file_a.bin file_b.bin file_c.bin

echo ""
echo "Verifying PAR2 archive..."
par2 verify multifile.par2

echo ""
echo "✓ PAR2 files generated successfully!"
echo ""
echo "Files created:"
ls -lh *.bin *.par2

echo ""
echo "Expected MD5s:"
md5sum file_a.bin file_b.bin file_c.bin 2>/dev/null || md5 file_a.bin file_b.bin file_c.bin

echo ""
echo "These PAR2 files should now be committed to the repository."
echo "The .bin files are temporary and should NOT be committed."
echo ""
echo "To clean up .bin files: rm -f file_a.bin file_b.bin file_c.bin"
