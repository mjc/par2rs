# SIMD Optimization for PAR2 Reed-Solomon Operations

## Overview

This document describes the SIMD (Single Instruction, Multiple Data) optimizations implemented in par2rs for Reed-Solomon error correction operations. Combined with I/O optimizations, par2rs achieves **2.4-3x speedup** over par2cmdline for real-world repair workloads.

**Performance breakdown:**
- **I/O optimization** (primary factor): Full slice-size chunks eliminate 32x redundant reads
- **SIMD acceleration**: 2.2-2.8x speedup for GF(2^16) multiply-add operations
- **Parallel reconstruction**: Multi-threaded chunk processing with Rayon
- **Smart caching**: LRU cache with dynamic sizing

**Measured performance (M1 MacBook Air 16GB):**
- 100MB: 2.77x speedup (2.26s → 0.81s)
- 1GB: 2.99x speedup (22.7s → 7.6s)
- 10GB: 2.46x speedup (104.8s → 42.6s)
- 25GB: 2.36x speedup (349.6s → 147.8s)

For end-to-end benchmark results, see [BENCHMARK_RESULTS.md](BENCHMARK_RESULTS.md).

## SIMD Implementations

par2rs includes two SIMD implementations, all using the same nibble-based table lookup strategy:

### 1. PSHUFB (x86_64 - AVX2/SSSE3)

Platform-specific implementation for x86_64 CPUs with AVX2 or SSSE3 support.

**Instructions Used:**
- `_mm256_shuffle_epi8` (PSHUFB) - 16-byte table lookups
- `_mm256_xor_si256` - XOR operations
- `_mm256_and_si256` / `_mm256_srli_epi16` - Nibble extraction

**Performance:** 2.76x faster than scalar (54.7ns vs 150.9ns for 528B blocks)

### 2. portable_simd (ARM64 and cross-platform)

Cross-platform implementation using `std::simd` that compiles to optimal SIMD instructions:
- **ARM64**: Compiles to NEON instructions (vqtbl1q_u8, vuzpq_u8, vzipq_u8)
- **Other platforms**: Falls back to available SIMD or scalar

**Instructions Used (ARM64):**
- `swizzle_dyn()` compiles to `vqtbl1q_u8` - Table lookup
- `simd_swizzle!()` for de-interleaving and re-interleaving bytes
- XOR operations via `^` operator

**Performance:** 2.2-2.4x faster than scalar on ARM64 (identical to hand-written NEON)

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

- **`src/reed_solomon/simd/pshufb.rs`**: PSHUFB implementation (x86_64)
  - `process_slice_multiply_add_pshufb()`: Main AVX2 SIMD loop (32 bytes/iteration)

- **`src/reed_solomon/simd/portable.rs`**: portable_simd implementation
  - `process_slice_multiply_add_portable_simd()`: Cross-platform SIMD using swizzle_dyn
  - Compiles to NEON on ARM64, optimal instructions on other platforms

- **`src/reed_solomon/simd/common.rs`**: Shared utilities and scalar fallback
  - `build_nibble_tables()`: Convert 256-entry tables to nibble tables
  - `process_slice_multiply_add_scalar()`: Scalar fallback implementation

- **`src/reed_solomon/simd/mod.rs`**: SIMD dispatcher
  - Runtime CPU feature detection (AVX2/SSSE3 on x86_64)
  - `detect_simd_support()`: Returns best available SIMD level
  - `process_slice_multiply_add_simd()`: Dispatches to optimal implementation

- **`src/reed_solomon/codec.rs`**: Core Reed-Solomon operations
  - `build_split_mul_table()`: Creates split low/high byte tables
  - `reconstruct_missing_slices_global()`: Main reconstruction function

- **`benches/repair_benchmark.rs`**: Performance benchmarks
  - Comparison benchmarks: PSHUFB/portable_simd vs scalar
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
### Cross-Platform SIMD Status

✅ **x86_64 (PSHUFB)**: Fully implemented and tested (2.76x speedup)  
✅ **ARM64 (portable_simd → NEON)**: Compiles to NEON instructions (2.2-2.4x speedup)  
✅ **Cross-platform (portable_simd)**: Works on all platforms with std::simd support  
✅ **Conditional compilation**: Correct cfg guards for platform-specific code  
✅ **Runtime dispatch**: Automatically selects best SIMD level at runtime

### Platform-Specific Notes

**x86_64:**
- Prefers PSHUFB (AVX2) when available (detected at runtime)
- Falls back to SSSE3 PSHUFB if no AVX2
- Falls back to scalar if no SIMD support (portable_simd is slower than scalar on x86_64)

**ARM64:**
- Uses portable_simd which compiles to NEON instructions
- NEON is always available on ARM64, provides 2.2-2.4x speedup
- No need for separate hand-written NEON implementation

**Other platforms (via portable_simd):**
- RISC-V with vector extensions
- WebAssembly with SIMD
- Future x86 platforms
- Performance depends on backend SIMD support

### Future Improvements

1. **Profile-guided optimization for dispatch**
   - Benchmark at startup to choose fastest implementation
   - Cache results across runs
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

