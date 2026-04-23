# Benchmark Results Summary

## April 22, 2026 Local `cargo bench` Rerun

This run was requested after an earlier benchmark attempt was interrupted
because the system was busy. The interrupted run was discarded. The fresh run
completed with:

```sh
nix develop -c cargo bench
```

Run metadata:

- Date: 2026-04-22, America/Denver local time.
- Host: `tina`.
- CPU: AMD Ryzen 9 5950X 16-Core Processor, 16 cores / 32 threads.
- RAM: 62 GiB total, 32 GiB available at metadata capture.
- OS: NixOS Linux `6.18.22`, x86_64.
- Git commit tested: `875ad6d` (`Support PAR1 renamed repair and purge`).
- Turbo baseline available from Nix:
  `/nix/store/02pgbh7v6h2ysrs1l8sqv09izk2swplq-par2cmdline-turbo-1.4.0/bin/par2`.
- Raw run log: `/tmp/par2rs-cargo-bench-20260422-rerun.log` on the machine
  where the run was performed.

Completed benchmark files:

- `benches/aligned_benchmark.rs` completed with no measured tests.
- `benches/create_benchmark.rs` completed.
- `benches/iai_simd.rs` completed: 9 Iai-Callgrind benchmarks, 0 regressions.
- `benches/md5_optimized.rs` completed.
- `benches/md5_throughput.rs` completed.
- `benches/repair_benchmark.rs` completed.
- `benches/simd_size_variants.rs` completed with no measured tests.
- `benches/verify_performance.rs` completed.

Failures and skips:

- `cargo bench` exited successfully.
- Criterion emitted two Gnuplot plot-generation errors during
  `create_benchmark`; measurement continued and the benchmark command did not
  fail.
- The release test harness reported normal ignored unit tests before running
  benchmarks.

Selected median samples from the fresh run:

| Group | Benchmark | Median |
| --- | --- | ---: |
| Create | `create_file/1KB` | 2.9197 ms |
| Create | `create_file/10KB` | 57.557 ms |
| Create | `create_file/100KB` | 141.42 ms |
| Create | `create_file/1MB` | 152.80 ms |
| Create | `create_file/10MB` | 179.99 ms |
| MD5 throughput | `calculate_file_md5/1GB` | 1.6612 s, 601.97 MiB/s |
| Repair SIMD | `simd_multiply_add_comparison/with_pshufb` | 53.255 ns |
| Repair SIMD | `simd_multiply_add_comparison/lib_scalar` | 123.54 ns |
| Repair SIMD | `simd_variants_by_size/pshufb/1MB` | 52.557 us |
| Repair SIMD | `simd_variants_by_size/scalar/1MB` | 258.79 us |
| Reed-Solomon | `reconstruct/1` | 72.482 us |
| Reed-Solomon | `reconstruct/5` | 8.4893 ms |
| Reed-Solomon | `reconstruct/10` | 16.169 ms |
| GF16 matrix | `invert_4x4` | 33.970 ns |
| GF16 matrix | `invert_16x16` | 1.8845 us |
| GF16 matrix | `invert_32x32` | 13.965 us |
| Verify | `verify_1GB/old_two_pass` | 1.2716 s, 805.28 MiB/s |
| Verify | `verify_1GB/new_single_pass` | 1.2778 s, 801.36 MiB/s |

Summary:

- The fresh rerun was substantially cleaner than the interrupted busy-system
  attempt and completed all benchmark files.
- The Criterion "Performance has improved" annotations in create benchmarks are
  relative to local stored baselines and are not treated as turbo comparison
  claims.
- PSHUFB SIMD measured faster than scalar in the repair microbenchmarks on this
  AVX2-capable host.
- The verify single-pass and old two-pass samples were close on this run, with
  the 1 GiB single-pass sample slightly slower than the old two-pass sample.
- No optimization changes were made based on this benchmark run.

## April 2026 Parity-Branch Status

The `codex/par2-turbo-parity` branch expands benchmark coverage for the current
par2cmdline-turbo parity work. In addition to the existing create, verify, MD5,
SIMD, and reconstruction benchmarks, `benches/repair_benchmark.rs` now includes
focused GF16 matrix inversion and scalar GF16 arithmetic benchmarks.

No new timing numbers are recorded here yet. Run `cargo bench` on the target
machine before making performance claims, and use `cargo bench --no-run` as the
fast compile-only acceptance check.

⚠️ **Performance Regression Note:** These results (November 2025) show degraded performance compared to previous benchmarks (October 2025) which demonstrated 2-200× speedups. The current implementation maintains correctness and par2cmdline compatibility but has lost most of its performance advantages on Linux x86_64. This regression is under investigation.

Comprehensive end-to-end benchmarking results showing par2rs performance compared to par2cmdline across different platforms.

## Test Platforms

### Linux x86_64
- **CPU**: AMD Ryzen 9 5950X 16-Core Processor (32 threads, up to 5.27 GHz)
- **RAM**: 64 GB DDR4
- **OS**: Linux x86_64
- **SIMD**: AVX2 + SSSE3 (PSHUFB optimizations)

### macOS Apple Silicon
- **CPU**: Apple M1 (ARM64/AArch64, 8 cores)
- **RAM**: 16 GB unified memory
- **OS**: macOS (aarch64-darwin)
- **SIMD**: ARM NEON + portable_simd

## Linux x86_64 Performance Results

### Test Configuration
- **Corruption**: 512 bytes at file midpoint
- **Recovery**: 5% redundancy (PAR2 standard)
- **Iterations**: 10 iterations per test
- **System**: AMD Ryzen 9 5950X, 64GB RAM, Linux x86_64
- **Date**: November 7, 2025
- **Raw data**: [par2rs_benchmark_results_20251107_002703.txt](par2rs_benchmark_results_20251107_002703.txt)

### Results Summary

| File Size | par2cmdline (avg) | par2rs (avg) | Speedup | Notes |
|-----------|-------------------|--------------|---------|-------|
| 1MB       | 0.032s            | 0.026s       | **1.23x** | Minimal overhead |
| 10MB      | 0.074s            | 0.048s       | **1.54x** | I/O + SIMD benefits |
| 100MB     | 0.386s            | 0.321s       | **1.20x** | Consistent gains |
| 1GB       | 3.743s            | 3.366s       | **1.11x** | Memory bandwidth bound |
| 10GB      | 58.804s           | 38.320s      | **1.53x** | I/O optimization shines |

### Key Findings (Linux x86_64)

**⚠️ REGRESSION:** These results represent a significant performance regression from previous benchmarks:
- **Previous results (Oct 2025)**: 2-212× speedup across file sizes
- **Current results (Nov 2025)**: 1.11-1.54× speedup - **most performance gains lost**
- **Suspected causes**: Recent changes for par2cmdline compatibility may have introduced inefficiencies
- **Status**: Under investigation - correctness maintained but performance degraded

Current observations:
1. **Consistent Performance**: 1.11-1.54x speedup across all file sizes shows reliable but modest improvements
2. **Best at Medium-Large Files**: 10GB files show optimal 1.53x speedup with I/O optimization
3. **Minimal Overhead**: Even tiny 1MB files show 1.23x speedup (vs previous large overhead issues)
4. **Memory Bandwidth Scaling**: 1GB files show lowest speedup (1.11x) as both implementations become memory-bound
5. **Low Variance**: par2rs shows consistent performance with <5% variance
6. **I/O Optimization Impact**: Larger files (10GB) benefit most from optimized read patterns (1.53x)

### Detailed Results

#### 1MB File (10 iterations)
```
par2cmdline:
  Average: 0.032s
  Min:     0.031s
  Max:     0.033s

par2rs:
  Average: 0.026s
  Min:     0.025s
  Max:     0.028s

Speedup: 1.23x
```

#### 10MB File (10 iterations)
```
par2cmdline:
  Average: 0.074s
  Min:     0.071s
  Max:     0.077s

par2rs:
  Average: 0.048s
  Min:     0.046s
  Max:     0.051s

Speedup: 1.54x
```

#### 100MB File (10 iterations)
```
par2cmdline:
  Average: 0.386s
  Min:     0.379s
  Max:     0.396s

par2rs:
  Average: 0.321s
  Min:     0.316s
  Max:     0.326s

Speedup: 1.20x
```

#### 1GB File (10 iterations)
```
par2cmdline:
  Average: 3.743s
  Min:     3.572s
  Max:     3.912s

par2rs:
  Average: 3.366s
  Min:     3.280s
  Max:     3.480s

Speedup: 1.11x
```

#### 10GB File (10 iterations)
```
par2cmdline:
  Average: 58.804s
  Min:     54.972s
  Max:     61.845s

par2rs:
  Average: 38.320s
  Min:     37.122s
  Max:     39.554s

Speedup: 1.53x
```

## macOS Apple Silicon Performance Results

**⚠️ OUTDATED DATA:** These results are from October 2025 and have not been re-tested with the current codebase. New benchmarks needed to confirm whether the Linux regression affects macOS as well.

### Test Configuration
- **Corruption**: Similar corruption patterns as Linux tests
- **Recovery**: 5% redundancy (PAR2 standard)
- **Iterations**: 10 iterations (10MB-10GB), 5 iterations (25GB)
- **System**: Apple M1 MacBook Air, 16GB RAM, macOS

### Results Summary

| File Size | par2cmdline (avg) | par2rs (avg) | Speedup | Notes |
|-----------|-------------------|--------------|---------|-------|
| 100 MB    | 2.260s            | 0.814s       | **2.77x** | I/O optimized |
| 1 GB      | 22.678s           | 7.569s       | **2.99x** | I/O optimized |
| 10 GB     | 104.775s          | 42.563s      | **2.46x** | I/O optimized |
| 25 GB     | 349.621s          | 147.751s     | **2.36x** | I/O optimized |

### Key Findings (macOS M1)

1. **I/O Optimization Breakthrough**: 2.36x-2.99x speedup from using full slice-size chunks (eliminates 32x redundant reads)
2. **Best Performance at 1GB**: 2.99x speedup is the sweet spot - nearly 3x faster than par2cmdline
3. **Consistent Speedup**: 2.36x-2.99x across entire 100MB-25GB range shows optimization effectiveness
4. **Scales to Large Files**: Even at 25GB, maintains 2.36x speedup (40% improvement over previous 1.68x)
5. **Exceptional Consistency**: Very low variance (~2-7%) across all file sizes
6. **Verified Correctness**: All repairs verified to produce bit-identical results

### Detailed Results

#### 100MB File (10 iterations, I/O optimized)
```
par2cmdline:
  Average: 2.260s
  Min:     2.183s
  Max:     2.353s
  Variance: ~3.7%

par2rs:
  Average: 0.814s
  Min:     0.757s
  Max:     1.174s
  Variance: ~23% (first iteration outlier: 1.17s, rest: 0.76-0.84s)

Speedup: 2.77x

All repairs verified correct
```

#### 1GB File (10 iterations, I/O optimized)
```
par2cmdline:
  Average: 22.678s
  Min:     21.559s
  Max:     25.845s
  Variance: ~8.6%

par2rs:
  Average: 7.569s
  Min:     7.246s
  Max:     8.305s
  Variance: ~6.6%

Speedup: 2.99x (best speedup across all sizes)

All repairs verified correct
```

#### 10GB File (10 iterations, I/O optimized)
```
10MB:
  par2cmdline avg: 0.069s
  par2rs avg:      0.044s
  Speedup:         1.57x

100MB:
  par2cmdline avg: 0.636s
  par2rs avg:      0.350s
  Speedup:         1.82x

1GB:
  par2cmdline avg: 6.358s
  par2rs avg:      3.196s
  Speedup:         1.99x
```

#### 10GB File Repair (10 iterations, I/O optimized)
```
par2cmdline:
  Average: 104.775s
  Min:     101.378s
  Max:     109.831s
  Variance: ~4%

par2rs:
  Average: 42.563s
  Min:     40.481s
  Max:     44.333s
  Variance: ~4.4%

Speedup: 2.46x

Individual iteration results:
Iteration | par2cmdline | par2rs    | Improvement
----------|-------------|-----------|------------
        1 | 109.831s    | 44.333s   | 2.48x
        2 | 103.722s    | 43.820s   | 2.37x
        3 | 105.955s    | 42.690s   | 2.48x
        4 | 101.855s    | 40.481s   | 2.52x (best)
        5 | 101.378s    | 42.874s   | 2.36x
        6 | 108.817s    | 41.609s   | 2.61x
        7 | 104.016s    | 42.703s   | 2.44x
        8 | 103.004s    | 42.204s   | 2.44x
        9 | 105.455s    | 41.977s   | 2.51x
       10 | 103.721s    | 42.941s   | 2.42x

All repairs verified correct
```

#### 25GB File (3 iterations, I/O optimized)
```
par2cmdline:
  Average: 349.621s
  Min:     347.891s
  Max:     350.665s
  Variance: ~0.4%

par2rs:
  Average: 147.751s
  Min:     146.058s
  Max:     149.053s
  Variance: ~1.0%

Speedup: 2.36x

All repairs verified correct

Individual times:
Iteration | par2cmdline   | par2rs
----------|---------------|-------------
        1 | 350.665s      | 148.144s
        2 | 350.307s      | 146.058s
        3 | 347.891s      | 149.053s
```
```
25GB:
  par2cmdline avg: 355.425s
  par2rs avg:      211.160s
  Speedup:         1.68x
  Note:            I/O intensive (reads ~128GB, writes ~26GB)
                   Should be re-benchmarked with I/O optimization
```

## Cross-Platform Comparison

**⚠️ Note:** macOS M1 data is from October 2025 and may not reflect current performance.

| Metric | Linux x86_64 (Ryzen 9) | macOS M1 (OUTDATED) |
|--------|------------------------|----------|
| **Best Speedup** | 1.54x (10MB) | 2.99x (1GB) |
| **1MB Speedup** | 1.23x | N/A |
| **10MB Speedup** | 1.54x | N/A |
| **100MB Speedup** | 1.20x | 2.77x |
| **1GB Speedup** | 1.11x | 2.99x |
| **10GB Speedup** | 1.53x | 2.46x |
| **25GB Speedup** | N/A | 2.36x |
| **SIMD Technique** | PSHUFB (AVX2) | NEON + portable_simd |
| **SIMD Speedup** | 2.76x | 2.2-2.4x |
| **Primary Factor** | I/O + Reed-Solomon | I/O + Reed-Solomon |
| **Variance** | Very Low (<5%) | Very Low (<2%) |
| **Benchmark Date** | November 2025 | October 2025 |

**Note:** The more modest Linux speedups (1.1-1.5x) compared to macOS (2.3-3.0x) may indicate the Linux regression hasn't affected macOS, or macOS data is outdated and needs re-testing.

## Performance Factors

The speedups come from par2rs's optimized implementation:

1. **Reed-Solomon & Galois Field Operations** (primary factor)
   - SIMD-accelerated GF(2^16) multiply-add operations
   - x86_64: PSHUFB-based nibble lookup (AVX2)
   - ARM64: NEON vtbl + portable_simd swizzle operations
   - 2.2-2.8x speedup at the operation level
   - See [SIMD_OPTIMIZATION.md](SIMD_OPTIMIZATION.md) for details

2. **Optimized I/O Patterns**
   - Use full slice-size chunks instead of 64KB blocks
   - LRU cache with dynamic sizing based on slice size
   - Sequential read patterns with position tracking
   - Reduces redundant reads and improves cache efficiency

3. **Parallel Reconstruction**
   - Rayon-based parallel chunk processing
   - Multi-threaded Reed-Solomon reconstruction
   - Scales well with core count

4. **Memory Efficiency**
   - Lazy loading of recovery data
   - Constant memory usage regardless of file size
   - Efficient caching strategy

5. **Smart Validation**
   - Skip slice validation for files with matching MD5
   - Conditional buffer zeroing only for partial slices
   - HashMap lookup hoisting in hot loops

## Technical Details

For detailed information about SIMD implementations, microbenchmarks, and technical approach, see:
- [SIMD_OPTIMIZATION.md](SIMD_OPTIMIZATION.md) - Technical implementation details
- [README.md](../README.md) - Project overview and quick start
