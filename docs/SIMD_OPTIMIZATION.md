# SIMD Optimization for PAR2 Reed-Solomon Operations

## Overview

This document describes the SIMD (Single Instruction, Multiple Data) optimizations implemented in par2rs for Reed-Solomon error correction operations. These optimizations provide **2.2-2.8x speedup** for GF(2^16) multiply-add operations, which are the computational core of PAR2 repair.

For end-to-end benchmark results, see [BENCHMARK_RESULTS.md](BENCHMARK_RESULTS.md).

## SIMD Implementations

par2rs includes three SIMD implementations, all using the same nibble-based table lookup strategy:

### 1. PSHUFB (x86_64 - AVX2/SSSE3)

Platform-specific implementation for x86_64 CPUs with AVX2 or SSSE3 support.

**Instructions Used:**
- `_mm256_shuffle_epi8` (PSHUFB) - 16-byte table lookups
- `_mm256_xor_si256` - XOR operations
- `_mm256_and_si256` / `_mm256_srli_epi16` - Nibble extraction

**Performance:** 2.76x faster than scalar (54.7ns vs 150.9ns for 528B blocks)

### 2. ARM NEON (ARM64/AArch64)

Platform-specific implementation for ARM processors.

**Instructions Used:**
- `vqtbl1q_u8` - Table lookup (equivalent to PSHUFB)
- `vuzpq_u8` - De-interleave even/odd bytes
- `vzipq_u8` - Re-interleave results
- `veorq_u8` - XOR operations

**Performance:** 2.2-2.4x faster than scalar (matches portable_simd)

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

### SIMD Techniques

#### PSHUFB (x86_64)

Based on "Screaming Fast Galois Field Arithmetic Using Intel SIMD Instructions" by James Plank:

## Galois Field GF(2^16) Implementation

### Polynomial and Operations

par2rs uses the PAR2 specification's Galois Field GF(2^16):
- **Polynomial**: `0x1100B` (x^16 + x^12 + x^3 + x + 1)
- **Operations**: Addition (XOR), Multiplication (polynomial mod reduction)
- **Vandermonde matrices**: Used for Reed-Solomon error correction

### Split Multiplication Tables

Traditional GF(2^16) multiplication uses 256 KB lookup tables:
- 65536 entries × 2 bytes = 128 KB per operation
- High/low byte split: 2 tables × 128 KB = 256 KB total
- **Problem**: Exceeds L1 cache (32-48 KB typical)

**Nibble-based approach:**
```
Result = LOW_TABLE[low_nibble] ^ HIGH_TABLE[high_nibble]
```
- 16 entries × 2 bytes = 32 bytes per nibble table
- 4 nibble tables (2 for low byte, 2 for high byte) = 128 bytes total
- Actually uses 8 nibble tables for SIMD (16 bytes each) = 128 bytes
- **8x memory reduction**, fits in L1 cache

### References

- PAR2 Specification: https://parchive.sourceforge.net/docs/specifications/parity-volume-spec/article-spec.html
- Galois Field arithmetic paper: http://web.eecs.utk.edu/~plank/plank/papers/FAST-2013-GF.html
- Inspired by: `galois_2p8` crate (MIT licensed)

## Implementation Files

- **`src/reed_solomon/simd_pshufb.rs`**: PSHUFB implementation (x86_64)
  - `build_pshufb_tables()`: Convert 256-entry tables to 8 nibble tables
  - `process_slice_multiply_add_pshufb()`: Main AVX2 SIMD loop (32 bytes/iteration)

- **`src/reed_solomon/simd_neon.rs`**: ARM NEON implementation (ARM64)
  - `build_neon_tables()`: Build nibble lookup tables for vtbl
  - `process_slice_multiply_add_neon()`: NEON SIMD loop (16 bytes/iteration)
  - Byte interleaving using vuzpq_u8/vzipq_u8

- **`src/reed_solomon/simd.rs`**: SIMD dispatcher and portable_simd
  - Runtime CPU feature detection (AVX2/SSSE3 on x86_64)
  - `process_slice_multiply_add_portable_simd()`: Cross-platform SIMD using swizzle_dyn
  - Fallback to unrolled AVX2 or scalar code
  - **TODO**: Implement runtime dispatch to choose best implementation

- **`src/reed_solomon/reedsolomon.rs`**: Core Reed-Solomon operations
  - `build_split_mul_table()`: Creates split low/high byte tables
  - `reconstruct_missing_slices_global()`: Main reconstruction function

- **`benches/repair_benchmark.rs`**: Performance benchmarks
  - Comparison benchmarks: PSHUFB/NEON/portable_simd vs scalar
  - Reed-Solomon reconstruction benchmarks
  - Multiple data sizes (528B, 4KB, 64KB, 1MB)
  - See results in [BENCHMARK_RESULTS.md](BENCHMARK_RESULTS.md)

## Cross-Platform Support

### Current Status

The codebase supports SIMD optimizations across multiple architectures:

- **x86_64 (Linux/macOS)**: Full AVX2/SSSE3 SIMD optimizations with PSHUFB
- **ARM64/AArch64 (Apple Silicon)**: ARM NEON + portable_simd (both provide 2.2-2.4x speedup)
- **Other architectures**: portable_simd provides cross-platform SIMD

### Architecture-Specific Implementation

The SIMD code uses conditional compilation to provide platform-specific implementations:

```rust
// x86_64: PSHUFB implementation
#[cfg(target_arch = "x86_64")]
pub fn process_slice_multiply_add_simd(...) {
    match simd_level {
        SimdLevel::Avx2 => { /* AVX2 PSHUFB */ },
        SimdLevel::Ssse3 => { /* SSSE3 fallback */ },
        SimdLevel::None => { /* Scalar */ }
    }
}

// ARM64: NEON implementation
#[cfg(target_arch = "aarch64")]
pub unsafe fn process_slice_multiply_add_neon(...) {
    // vtbl-based table lookups
}

// All platforms: portable_simd
pub unsafe fn process_slice_multiply_add_portable_simd(...) {
    // swizzle_dyn for cross-platform SIMD
}
```

**Benefits:**
- No warnings on non-x86_64 platforms
- Clean compilation on all architectures
- Prepares for future ARM NEON optimizations

### Future Work: ARM NEON Optimizations

Apple Silicon and other ARM processors support NEON SIMD instructions that can achieve similar performance to x86 AVX2:


✅ **x86_64 (PSHUFB)**: Fully implemented and tested (2.76x speedup)  
✅ **ARM64 (NEON)**: Fully implemented and tested (2.2-2.4x speedup)  
✅ **Cross-platform (portable_simd)**: Fully implemented and tested (matches NEON on M1)  
✅ **Conditional compilation**: Correct cfg guards for platform-specific code  
⏳ **Runtime dispatch**: TODO - Currently uses compile-time selection

### Platform-Specific Notes

**x86_64:**
- Prefers PSHUFB (AVX2) when available (detected at runtime)
- Falls back to SSSE3 PSHUFB if no AVX2
- Falls back to scalar if no SIMD support

**ARM64:**
- Uses NEON intrinsics (always available on ARM64)
- portable_simd provides identical performance
- Future: Add runtime dispatch to prefer NEON over portable_simd

**Other platforms (via portable_simd):**
- RISC-V with vector extensions
- WebAssembly with SIMD
- Future x86 platforms
- Performance depends on backend SIMD support

### Future Improvements

1. **Runtime dispatch for best implementation**
   - x86_64: PSHUFB → portable_simd → scalar
   - ARM64: NEON → portable_simd → scalar
   - Currently uses compile-time selection

2. **AVX-512 support**
   - 64-byte processing (vs 32-byte AVX2)
   - Potential for additional speedup on latest CPUs

3. **Auto-vectorization improvements**
   - Continue optimizing scalar code for better LLVM auto-vectorization
   - May benefit platforms without explicit SIMD support

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
- **Rust**: nightly toolchain (for portable_simd feature)
- **Platform**: x86_64 (with AVX2/SSSE3) or ARM64
- **OS**: Linux, macOS, or Windows
- **Optional**: par2cmdline (for comparison benchmarks)

### Running Microbenchmarks
```bash
# All benchmarks (includes SIMD variants)
cargo bench

# SIMD comparison only
cargo bench --bench repair_benchmark

# With gnuplot visualization (if installed)
cargo bench --bench repair_benchmark -- --plotting-backend gnuplot
```

### Running End-to-End Benchmarks

See [BENCHMARK_RESULTS.md](BENCHMARK_RESULTS.md) for detailed results.

```bash
# Averaged benchmark (recommended for accuracy)
./scripts/benchmark_repair_averaged.sh [iterations]

# Example: 10 iterations on 1GB file
./scripts/benchmark_repair_averaged.sh 10
```

**Benchmark script features:**
1. Creates test file of specified size
2. Generates PAR2 files with 5% redundancy
3. Corrupts data to trigger repair
4. Repairs with both par2cmdline and par2rs
5. Verifies correctness
6. Reports timing statistics (mean, min, max, stddev)

## Performance Tips

### For Maximum Performance

1. **Use release builds:**
   ```bash
   cargo build --release
   ```
   Debug builds are ~10-100x slower due to missing optimizations.

2. **Enable CPU-specific optimizations:**
   ```bash
   RUSTFLAGS="-C target-cpu=native" cargo build --release
   ```
   Allows the compiler to use all available CPU features.

3. **Run on dedicated hardware:**
   - Disable CPU frequency scaling
   - Close background applications
   - Use averaged benchmarks for statistical significance

4. **File size considerations:**
   - Speedup increases with file size (better amortization)
   - Best results on files > 100 MB
   - I/O becomes dominant factor for very large files (> 10 GB)

## Future Optimizations

Potential areas for improvement:

1. **Runtime SIMD dispatch**
   - Auto-detect and select best available SIMD implementation
   - Priority: PSHUFB (x86_64) → NEON (ARM64) → portable_simd → scalar
   - Currently uses compile-time selection

2. **AVX-512 support (investigate)**
   - 64-byte processing (vs 32-byte AVX2, 16-byte NEON)
   - Unknown if it would provide benefits for this workload
   - Requires hardware testing and validation

3. **I/O optimization for very large files**
   - 25GB repair currently reads ~128GB and writes ~26GB (5x read amplification)
   - Optimize recovery slice loading strategy
   - Implement streaming reconstruction to minimize memory-to-disk round trips
   - Better parallelization for NVMe SSDs

4. **Additional platform support**
   - RISC-V vector extensions
   - WebAssembly SIMD
   - Power/POWER9 VSX

5. **Algorithm improvements**
   - Further cache optimization for Galois Field tables
   - Explore alternative Reed-Solomon implementations (FFT-based?)
   - Optimize Vandermonde matrix inversion

## References

1. [PAR2 Specification](https://parchive.sourceforge.net/docs/specifications/parity-volume-spec/article-spec.html)
2. [Screaming Fast Galois Field Arithmetic (Plank)](http://web.eecs.utk.edu/~plank/plank/papers/FAST-2013-GF.html)
3. [galois_2p8 crate](https://github.com/djsweet/galois_2p8) - Original nibble-based technique for GF(2^8)
4. [par2cmdline](https://github.com/Parchive/par2cmdline) - Reference implementation
5. [Intel Intrinsics Guide](https://www.intel.com/content/www/us/en/docs/intrinsics-guide/index.html) - PSHUFB and AVX2 documentation
6. [ARM NEON Intrinsics Reference](https://developer.arm.com/architectures/instruction-sets/intrinsics/) - NEON documentation

