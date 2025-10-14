use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use par2rs::reed_solomon::reedsolomon::{ReconstructionEngine, build_split_mul_table, SplitMulTable};
use par2rs::reed_solomon::galois::Galois16;
use par2rs::reed_solomon::simd::{SimdLevel, process_slice_multiply_add_simd, process_slice_multiply_add_avx2_unrolled};
use par2rs::RecoverySlicePacket;
use std::collections::HashMap;

/// Pure scalar implementation (no SIMD)
fn process_slice_multiply_add_scalar(input: &[u8], output: &mut [u8], tables: &SplitMulTable) {
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
    
    // Benchmark with PSHUFB SIMD (AVX2)
    group.bench_function("with_pshufb", |b| {
        let mut output = vec![0x55u8; size];
        b.iter(|| {
            unsafe {
                process_slice_multiply_add_simd(
                    black_box(&input),
                    black_box(&mut output),
                    black_box(&tables),
                    SimdLevel::Avx2
                );
            }
        });
    });
    
    // Benchmark with unrolled SIMD (no PSHUFB)
    group.bench_function("unrolled_only", |b| {
        let mut output = vec![0x55u8; size];
        b.iter(|| {
            unsafe {
                process_slice_multiply_add_avx2_unrolled(
                    black_box(&input),
                    black_box(&mut output),
                    black_box(&tables)
                );
            }
        });
    });
    
    // Benchmark without SIMD (pure scalar)
    group.bench_function("scalar_fallback", |b| {
        let mut output = vec![0x55u8; size];
        b.iter(|| {
            process_slice_multiply_add_scalar(
                black_box(&input),
                black_box(&mut output),
                black_box(&tables)
            );
        });
    });
    
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
                md5: [0; 16],
                set_id: [0; 16],
                type_of_packet: [0; 16],
                exponent: i,
                recovery_data: vec![0xAAu8; slice_size],
            });
        }
        
        group.bench_with_input(
            BenchmarkId::new("with_pshufb", missing_count),
            &missing_count,
            |b, &missing_count| {
                let engine = ReconstructionEngine::new(slice_size, total_slices, recovery_slices.clone());
                
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
                        black_box(total_slices)
                    );
                    assert!(result.success);
                    assert_eq!(result.reconstructed_slices.len(), missing_count);
                });
            }
        );
    }
    
    group.finish();
}

criterion_group!(benches, bench_simd_comparison, bench_reed_solomon_reconstruct);
criterion_main!(benches);
