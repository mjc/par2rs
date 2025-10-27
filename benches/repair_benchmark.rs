use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use par2rs::reed_solomon::codec::{build_split_mul_table, ReconstructionEngine, SplitMulTable};
use par2rs::reed_solomon::galois::Galois16;
use par2rs::reed_solomon::simd::process_slice_multiply_add_portable_simd;
#[cfg(target_arch = "x86_64")]
use par2rs::reed_solomon::simd::{process_slice_multiply_add_simd, SimdLevel};
use par2rs::RecoverySlicePacket;
use std::collections::HashMap;
use std::hint::black_box;

/// Pure scalar implementation (no SIMD) - for benchmark baseline
fn scalar_baseline(input: &[u8], output: &mut [u8], tables: &SplitMulTable) {
    let min_len = input.len().min(output.len());
    let num_words = min_len / 2;
    if num_words == 0 {
        return;
    }

    unsafe {
        let in_ptr = input.as_ptr() as *const u16;
        let out_ptr = output.as_mut_ptr() as *mut u16;
        let low_ptr = tables.low.as_ptr();
        let high_ptr = tables.high.as_ptr();

        // No unrolling - truly scalar baseline
        for idx in 0..num_words {
            let in_word = *in_ptr.add(idx);
            let out_word = *out_ptr.add(idx);
            let mul_result =
                *low_ptr.add((in_word & 0xFF) as usize) ^ *high_ptr.add((in_word >> 8) as usize);
            *out_ptr.add(idx) = out_word ^ mul_result;
        }
    }

    // Handle odd trailing byte
    if min_len % 2 == 1 {
        let last_idx = num_words * 2;
        let in_byte = input[last_idx];
        output[last_idx] ^= tables.low[in_byte as usize].to_le_bytes()[0];
    }
}

/// Benchmark SIMD multiply-add with different implementations
fn bench_simd_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_multiply_add_comparison");
    group.measurement_time(std::time::Duration::from_secs(30));

    let size = 528; // PAR2 block size
    let coefficient = 0x1234u16;
    let gf = Galois16::new(coefficient);
    let tables = build_split_mul_table(gf);

    let input = vec![0xAAu8; size];

    // Benchmark with PSHUFB SIMD (AVX2) - x86_64 only
    #[cfg(target_arch = "x86_64")]
    group.bench_function("with_pshufb", |b| {
        let mut output = vec![0x55u8; size];
        b.iter(|| {
            process_slice_multiply_add_simd(
                black_box(&input),
                black_box(&mut output),
                black_box(&tables),
                SimdLevel::Avx2,
            );
        });
    });

    // Benchmark the library's scalar fallback function
    group.bench_function("lib_scalar", |b| {
        let mut output = vec![0x55u8; size];
        b.iter(|| unsafe {
            par2rs::reed_solomon::simd::process_slice_multiply_add_scalar(
                black_box(&input),
                black_box(&mut output),
                black_box(&tables),
            );
        });
    });

    // Benchmark without SIMD (pure scalar baseline)
    group.bench_function("scalar_fallback", |b| {
        let mut output = vec![0x55u8; size];
        b.iter(|| {
            scalar_baseline(
                black_box(&input),
                black_box(&mut output),
                black_box(&tables),
            );
        });
    });

    // Benchmark with portable_simd (cross-platform SIMD)
    group.bench_function("portable_simd", |b| {
        let mut output = vec![0x55u8; size];
        b.iter(|| unsafe {
            process_slice_multiply_add_portable_simd(
                black_box(&input),
                black_box(&mut output),
                black_box(&tables),
            );
        });
    });

    group.finish();
}

/// Benchmark different SIMD variants across multiple data sizes
///
/// See docs/SIMD_OPTIMIZATION.md for detailed performance results and analysis.
fn bench_simd_variants_by_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_variants_by_size");
    group.measurement_time(std::time::Duration::from_secs(20));

    let coefficient = 0x1234u16;
    let gf = Galois16::new(coefficient);
    let tables = build_split_mul_table(gf);

    // Test different sizes: small (cache-friendly) to large (memory-bound)
    for &size in &[528, 4096, 65536, 1_048_576] {
        let input = vec![0xAAu8; size];
        let size_label = if size < 1024 {
            format!("{}B", size)
        } else if size < 1024 * 1024 {
            format!("{}KB", size / 1024)
        } else {
            format!("{}MB", size / (1024 * 1024))
        };

        // Scalar baseline
        group.bench_with_input(
            BenchmarkId::new("scalar", &size_label),
            &size,
            |b, &size| {
                let mut output = vec![0x55u8; size];
                b.iter(|| {
                    scalar_baseline(
                        black_box(&input),
                        black_box(&mut output),
                        black_box(&tables),
                    );
                });
            },
        );

        // portable_simd
        group.bench_with_input(
            BenchmarkId::new("portable_simd", &size_label),
            &size,
            |b, &size| {
                let mut output = vec![0x55u8; size];
                b.iter(|| unsafe {
                    process_slice_multiply_add_portable_simd(
                        black_box(&input),
                        black_box(&mut output),
                        black_box(&tables),
                    );
                });
            },
        );

        // x86_64 PSHUFB (for comparison on x86 systems)
        #[cfg(target_arch = "x86_64")]
        group.bench_with_input(
            BenchmarkId::new("pshufb", &size_label),
            &size,
            |b, &size| {
                let mut output = vec![0x55u8; size];
                b.iter(|| {
                    process_slice_multiply_add_simd(
                        black_box(&input),
                        black_box(&mut output),
                        black_box(&tables),
                        SimdLevel::Avx2,
                    );
                });
            },
        );
    }

    group.finish();
}

/// Benchmark complete Reed-Solomon reconstruction (the actual hotspot from flamegraph)
///
/// This tests reconstruct_missing_slices_global which performs:
/// 1. Matrix inversion in GF(2^16)
/// 2. Multiple SIMD multiply-add operations per slice
fn bench_reed_solomon_reconstruct(c: &mut Criterion) {
    let mut group = c.benchmark_group("reed_solomon_reconstruct");
    group.measurement_time(std::time::Duration::from_secs(30));

    // Test with realistic PAR2 parameters
    let slice_size = 528; // Typical PAR2 block size
    let total_slices = 1986; // Number of slices in testfile

    // Test different numbers of missing slices (recovery complexity)
    for &missing_count in &[1, 5, 10] {
        // Create recovery slices
        let mut recovery_slices = Vec::new();
        for i in 0..99 {
            recovery_slices.push(RecoverySlicePacket {
                length: 0,
                md5: par2rs::domain::Md5Hash::new([0; 16]),
                set_id: par2rs::domain::RecoverySetId::new([0; 16]),
                type_of_packet: [0; 16],
                exponent: i,
                recovery_data: vec![0xAAu8; slice_size],
            });
        }

        group.bench_with_input(
            BenchmarkId::new("reconstruct", missing_count),
            &missing_count,
            |b, &missing_count| {
                let engine =
                    ReconstructionEngine::new(slice_size, total_slices, recovery_slices.clone());

                // Create existing slices (all except the missing ones)
                let mut all_slices = HashMap::default();
                for i in missing_count..total_slices {
                    all_slices.insert(i, vec![0x55u8; slice_size]);
                }

                let global_missing_indices: Vec<usize> = (0..missing_count).collect();

                b.iter(|| {
                    let result = engine.reconstruct_missing_slices_global(
                        black_box(&all_slices),
                        black_box(&global_missing_indices),
                        black_box(total_slices),
                    );
                    assert!(result.success);
                    assert_eq!(result.reconstructed_slices.len(), missing_count);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_simd_comparison,
    bench_simd_variants_by_size,
    bench_reed_solomon_reconstruct
);
criterion_main!(benches);
