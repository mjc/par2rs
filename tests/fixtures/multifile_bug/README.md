# Multifile PAR2 Test Fixtures

This directory contains PAR2 recovery files used for testing multifile repair scenarios.

## Test Scenario

The test verifies that PAR2 repair works correctly when multiple files need repair:

- **file_a.bin**: 5 slices (160KB) - remains intact during test
- **file_b.bin**: 3 slices (96KB) - gets corrupted (1 slice damaged)
- **file_c.bin**: 2 slices (64KB) - **gets deleted** (all slices missing)

Total: 10 data slices, 5 recovery slices

During the test:

- 7 slices are available (file_a = 5, file_b partial = 2)
- 3 slices need reconstruction (file_b = 1, file_c = 2)
- The repair successfully reconstructs both damaged files

## Files

- `multifile.par2` - Main PAR2 index file
- `multifile.vol0+1.par2` - Recovery volume 0 (1 block)
- `multifile.vol1+2.par2` - Recovery volume 1 (2 blocks)
- `multifile.vol3+2.par2` - Recovery volume 3 (2 blocks)
- `generate_par2.sh` - Script to regenerate PAR2 files

## Regenerating PAR2 Files

If you need to regenerate the PAR2 files (e.g., to test with different parameters):

```bash
cd tests/fixtures/multifile_bug
bash generate_par2.sh
# Review the generated files
# Clean up temporary .bin files when done:
rm -f file_a.bin file_b.bin file_c.bin
```

**Note**: The test dynamically generates the data files (file_a.bin, file_b.bin, file_c.bin) at runtime. Only the PAR2 files are committed to the repository.

## Test Coverage

This fixture specifically tests:
1. Repairing multiple files in a single PAR2 set
2. Handling a completely missing/deleted file
3. Handling a partially corrupted file
4. Correct Reed-Solomon reconstruction across file boundaries
