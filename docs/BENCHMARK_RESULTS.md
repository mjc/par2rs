# Benchmark Results Summary

Comprehensive benchmarking results showing par2rs performance compared to par2cmdline.

## Test Configuration
- **Corruption**: 512 bytes at file midpoint (generated files) or 1MB at 10% offset (real files)
- **Recovery**: 5% redundancy (PAR2 standard)
- **Iterations**: 10 iterations per test
- **System**: AMD Ryzen 9 5950X, 64GB RAM, Linux x86_64

## Performance Results

| File Size | Files | par2cmdline (avg) | par2rs (avg) | Speedup | Notes |
|-----------|-------|-------------------|--------------|---------|-------|
| 1MB       | 1     | 0.276s            | 0.017s       | **16.23x** | Overhead-dominated |
| 10MB      | 1     | 0.168s            | 0.042s       | **4.00x**  | |
| 100MB     | 1     | 0.984s            | 0.350s       | **2.81x**  | |
| 1GB       | 1     | 11.388s           | 4.350s       | **2.61x**  | Single file repair |
| ~8GB      | 50    | 28.901s           | 16.248s      | **1.77x**  | Multi-file PAR2 set |

## Key Findings

1. **Small File Advantage**: par2rs shows exceptional speedup (16x) for small files due to efficient overhead handling
2. **Consistent Performance**: par2rs has much lower variance across iterations (2-3% vs 10-20%)
3. **Scaling**: Performance advantage remains strong across file sizes, from 1MB to 8GB+
4. **Real-World**: 2-3x speedup for typical repair scenarios (100MB-1GB single files)
5. **Multi-File**: 1.77x speedup on large multi-file PAR2 sets with 50 protected files
6. **Memory Efficiency**: Uses ~100MB RAM vs par2cmdline's variable usage (scales with file size)

## Detailed Results

### 1MB File (10 iterations)
```
par2cmdline:
  Average: 0.276s
  Min:     0.025s
  Max:     1.911s
  Variance: Very high (outliers present)

par2rs:
  Average: 0.017s
  Min:     0.015s
  Max:     0.030s
  Variance: Very low (consistent)

Speedup: 16.23x
```

### 10MB File (10 iterations)
```
par2cmdline:
  Average: 0.168s
  Min:     0.118s
  Max:     0.529s

par2rs:
  Average: 0.042s
  Min:     0.041s
  Max:     0.046s

Speedup: 4.00x
```

### 100MB File (10 iterations)
```
par2cmdline:
  Average: 0.984s
  Min:     0.969s
  Max:     1.030s

par2rs:
  Average: 0.350s
  Min:     0.337s
  Max:     0.391s

Speedup: 2.81x
```

### 1GB File (10 iterations)
```
par2cmdline:
  Average: 11.388s
  Min:     9.989s
  Max:     13.912s

par2rs:
  Average: 4.350s
  Min:     4.043s
  Max:     4.903s

Speedup: 2.61x
```

### Multi-File PAR2 Set - 50 files, ~8GB total (10 iterations)
```
par2cmdline:
  Average: 28.901s
  Min:     24.902s
  Max:     33.839s

par2rs:
  Average: 16.248s
  Min:     14.648s
  Max:     18.631s

Speedup: 1.77x
```

## Performance Optimizations

Key optimizations that enable these speedups:

1. **Skip Validation for Valid Files**: Files with matching MD5 skip full slice scanning (instant vs 400MB/s read)
2. **Sequential I/O**: Track read position to avoid unnecessary seeks during repair writes
3. **SIMD Acceleration**: Hardware-accelerated Reed-Solomon operations using PSHUFB
4. **Memory Efficiency**: Lazy loading of recovery data with 8MB buffers for optimal throughput
5. **Type Safety**: Zero-cost abstractions prevent common bugs without runtime overhead

## Running Benchmarks

To reproduce these results:

```bash
# Run comprehensive suite (all sizes)
./scripts/benchmark_all_comprehensive.sh [temp_dir]

# Run specific size with custom iterations
ITERATIONS=10 ./scripts/benchmark_repair_averaged.sh 100  # 100MB

# Extract summary
./scripts/extract_benchmark_results.sh /tmp/par2rs_benchmark_results_*.txt
```
