# SIMD Optimization for PAR2 Reed-Solomon Operations

## Overview

This document describes the SIMD optimizations implemented in par2rs for Reed-Solomon error correction, achieving **1.66x speedup** over par2cmdline (0.607s vs 1.008s average for 100MB file repair, averaged over 10 runs).

## Test System

- **CPU**: AMD Ryzen 9 5950X 16-Core Processor (32 threads, up to 5.27 GHz)
- **RAM**: 64 GB DDR4
- **OS**: Linux x86_64
- **Compiler**: rustc with `release` profile (optimized)

## Performance Results

### Real-World Performance (100MB File Repair, Averaged over 10 runs)

```
par2cmdline:
  Average: 1.008s
  Min:     0.991s
  Max:     1.044s

par2rs:
  Average: 0.607s
  Min:     0.596s
  Max:     0.624s

Speedup: 1.66x
```

**Results:**
- **par2rs (with PSHUFB)**: 0.607s average
- **par2cmdline**: 1.008s average
- **Speedup**: **1.66x faster**

### Microbenchmark Results (528-byte PAR2 blocks)

```
simd_multiply_add_comparison/with_pshufb
                        time:   [54.622 ns 54.723 ns 54.841 ns]

simd_multiply_add_comparison/unrolled_only
                        time:   [102.85 ns 103.18 ns 103.56 ns]

simd_multiply_add_comparison/scalar_fallback
                        time:   [150.58 ns 150.90 ns 151.28 ns]
```

| Implementation | Time | Speedup vs Scalar |
|---|---|---|
| **PSHUFB (AVX2)** | **54.7 ns** | **2.76x faster** ✅ |
| Unrolled AVX2 | 103.2 ns | 1.46x faster |
| Scalar baseline | 150.9 ns | 1.00x (baseline) |

**Key findings:**
- PSHUFB is **2.76x faster** than scalar code
- PSHUFB is **1.89x faster** than unrolled AVX2
- Performance scales to real-world workloads

### Reed-Solomon Reconstruction Benchmarks

```
reed_solomon_reconstruct/with_pshufb/1
                        time:   [76.549 µs 77.221 µs 77.884 µs]

reed_solomon_reconstruct/with_pshufb/5
                        time:   [6.8131 ms 6.8540 ms 6.9009 ms]

reed_solomon_reconstruct/with_pshufb/10
                        time:   [16.821 ms 17.097 ms 17.402 ms]
```

| Missing Slices | Time (mean) |
|---|---|
| 1 slice | 77.2 µs |
| 5 slices | 6.85 ms |
| 10 slices | 17.1 ms |

## Implementation Details

### Why We Can't Use Existing Crates

**PAR2 uses specific Vandermonde polynomials:**
- **GF(2^16)**: polynomial **0x1100B** (x¹⁶ + x¹² + x³ + x + 1) - primary for Reed-Solomon
- **GF(2^8)**: polynomial **0x11D** (x⁸ + x⁴ + x³ + x² + 1) - also supported

These are **primitive irreducible polynomials** used as the field generator to construct the Vandermonde matrix for Reed-Solomon encoding/decoding.

Existing Rust crates are incompatible:

1. **`galois_2p8`**: 
   - Field: GF(2^8) - wrong size (8-bit vs 16-bit)
   - Use: Borrowed SIMD **technique** only

2. **`reed-solomon-16`**: 
   - Field: GF(2^16) ✓
   - Polynomial: Leopard-RS polynomial ✗ (not PAR2's 0x1100B)
   - Performance: O(n log n) but incompatible

3. **`reed-solomon-erasure`**:
   - Configurable but not optimized for PAR2

**Why the polynomial matters:**
- Defines multiplication/division in the Galois Field
- PAR2 files encoded with 0x1100B can ONLY be decoded with 0x1100B
- Must match par2cmdline exactly for compatibility

**Our approach:**
- Implement GF(2^16) with PAR2's polynomial (0x1100B)
- Adapt SIMD techniques from `galois_2p8` for our specific field
- This ensures compatibility while achieving maximum performance

### PSHUFB Technique

Based on "Screaming Fast Galois Field Arithmetic Using Intel SIMD Instructions" by James Plank:
- Paper: http://web.eecs.utk.edu/~plank/plank/papers/FAST-2013-GF.html
- Inspired by: `galois_2p8` crate (MIT licensed)

**Key Insight:**
- PSHUFB can do 16-entry (4-bit) lookups
- We have 256-entry (8-bit) tables
- **Solution**: Split bytes into nibbles, do two lookups per byte

**For GF(2^16) multiplication:**
```
Input: 16-bit word = [high_byte:low_byte]
Result: tables.low[low_byte] ^ tables.high[high_byte]
```

**PSHUFB approach:**
1. Build 8 nibble tables (each 16 bytes = 128 bytes total vs 512 bytes for full tables)
2. For each input byte, split into low/high nibbles
3. Use `_mm256_shuffle_epi8` (PSHUFB) for parallel lookups
4. Combine results with XOR

**Memory efficiency:**
- Traditional: 2 × 256 × 2 bytes = 1024 bytes per coefficient
- PSHUFB: 8 × 16 bytes = 128 bytes per coefficient
- **8x reduction** in lookup table size (better cache utilization)

### Implementation Files

- **`src/reed_solomon/simd_pshufb.rs`**: PSHUFB implementation
  - `build_pshufb_tables()`: Convert 256-entry tables to 8 nibble tables
  - `process_slice_multiply_add_pshufb()`: Main AVX2 SIMD loop (32 bytes/iteration)

- **`src/reed_solomon/simd.rs`**: SIMD dispatcher and fallbacks
  - Runtime CPU feature detection (AVX2/SSSE3)
  - Fallback to unrolled AVX2 or scalar code

- **`src/reed_solomon/reedsolomon.rs`**: Core Reed-Solomon operations
  - `build_split_mul_table()`: Creates split low/high byte tables
  - `reconstruct_missing_slices_global()`: Main reconstruction function (50% of runtime)

- **`benches/repair_benchmark.rs`**: Performance benchmarks
  - Comparison benchmarks: PSHUFB vs unrolled vs scalar
  - Reed-Solomon reconstruction benchmarks
  - 30-second measurement time for accuracy

### Optimization Progression

1. **Initial**: 1.27s (slower than par2cmdline's ~1.0s)
2. **Eliminated duplicate loads**: ~1.15s
3. **16-word SIMD unrolling**: ~1.10s
4. **32-word SIMD unrolling**: ~0.976s
5. **PSHUFB implementation**: **0.607s** ✅ (1.66x faster than par2cmdline's 1.008s average)

## Attribution

### Licenses
- **par2rs**: MIT License
- **galois_2p8** (technique source): MIT License
- All dependencies: MIT or Apache-2.0 (compatible)

### Credits
- PSHUFB technique: James Plank's "Screaming Fast Galois Field Arithmetic"
- Implementation inspired by: `galois_2p8` by Dani Sweet (https://github.com/djsweet/galois_2p8)
- Adapted for GF(2^16) with PAR2's polynomial 0x1100B

## Building and Testing

### Requirements
- x86_64 CPU with AVX2 support
- Rust 1.70+ (for SIMD intrinsics)
- Linux/macOS/Windows
- par2cmdline (for comparison benchmarks)

### Running Microbenchmarks
```bash
# All benchmarks
cargo bench

# SIMD comparison only (30 second measurement time)
cargo bench simd_multiply_add_comparison

# Reed-Solomon reconstruction
cargo bench reed_solomon_reconstruct
```

### Running Real-World Benchmark
```bash
# Single run benchmark (with flamegraph generation)
./scripts/benchmark_repair.sh

# Averaged benchmark (10 iterations, more reliable)
./scripts/benchmark_repair_averaged.sh
```

**Single run script** (`benchmark_repair.sh`):
1. Creates a 100MB test file
2. Generates PAR2 files with 5% redundancy
3. Corrupts 1MB of data
4. Repairs with both par2cmdline and par2rs
5. Verifies correctness
6. Generates flamegraph for profiling

**Averaged benchmark script** (`benchmark_repair_averaged.sh`):
- Runs 10 iterations to get reliable averages
- Reports min/max/average times
- Calculates speedup
- Shows individual iteration results

**Example Output (Averaged):**
```
par2cmdline:
  Average: 1.008s
  Min:     0.991s
  Max:     1.044s

par2rs:
  Average: 0.607s
  Min:     0.596s
  Max:     0.624s

Speedup: 1.66x

✓ All repairs verified correct
```

## Future Optimizations

Potential improvements:
- [ ] AVX-512 support for newer CPUs
- [ ] ARM NEON SIMD for ARM64
- [ ] Multi-threading for large files (already using Rayon for file I/O)
- [ ] Further cache optimization for very large repair operations

## References

1. [PAR2 Specification](https://parchive.sourceforge.net/docs/specifications/parity-volume-spec/article-spec.html)
2. [Screaming Fast Galois Field Arithmetic (Plank)](http://web.eecs.utk.edu/~plank/plank/papers/FAST-2013-GF.html)
3. [galois_2p8 crate](https://github.com/djsweet/galois_2p8)
4. [par2cmdline](https://github.com/Parchive/par2cmdline)
