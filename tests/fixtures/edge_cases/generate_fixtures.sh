#!/usr/bin/env bash
# Generate pre-computed PAR2 test fixtures for edge case tests

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "Generating edge case test fixtures..."

# 1. test_no_repair_needed_path
echo "Valid content that won't be corrupted" > test_valid.txt
par2 c -r5 -q test_valid.par2 test_valid.txt

# 2. test_insufficient_recovery_error_path
printf '%0.s\x55' {1..100000} > large_test_original.txt
par2 c -r1 -q large_test.par2 large_test_original.txt

# 3. test_file_verification_after_repair
echo -n "Content to verify after repair" > verify_test_original.txt
par2 c -r5 -q verify_test.par2 verify_test_original.txt

# 4. test_multiple_files_scenario
echo -n "First file content" > file1.txt
echo -n "Second file content" > file2.txt
echo -n "Third file content" > file3.txt
par2 c -r5 -q multifile.par2 file1.txt file2.txt file3.txt

# 5. test_single_byte_file
echo -n "X" > single_original.txt
par2 c -r5 -q single.par2 single_original.txt

# 6. test_large_file_with_many_slices
printf '%0.s\x42' {1..500000} > large_original.txt
par2 c -r5 -q large.par2 large_original.txt

# 7. test_size_mismatch_detection (from test_repair_coverage.rs)
printf '%0.s\x33' {1..10000} > size_test_original.txt
par2 c -r5 -q size_test.par2 size_test_original.txt

# 8. test_hash_mismatch_detection (from test_repair_coverage.rs)
printf '%0.s\x44' {1..10000} > hash_test_original.txt
par2 c -r5 -q hash_test.par2 hash_test_original.txt

# 9. test_corrupted_file_repair (from test_repair_coverage.rs)
printf '%0.s\x42' {1..10000} > corrupt_repair_original.txt
par2 c -r5 -q corrupt_repair.par2 corrupt_repair_original.txt

# 10. test_missing_file_repair (from test_repair_coverage.rs)
echo -n "Test content for missing file" > missing_test_original.txt
par2 c -r5 -q missing_test.par2 missing_test_original.txt

echo "Fixtures generated successfully in $SCRIPT_DIR"
