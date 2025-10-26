use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use par2rs::reed_solomon::{
    ReedSolomon as OriginalRS, ReedSolomonBuilder, TypeSafeReedSolomon as TypeSafeRS,
    TypeSafeReedSolomonBuilder,
};
use std::hint::black_box;

/// Benchmark the original ReedSolomon implementation
fn bench_original_reed_solomon(c: &mut Criterion) {
    let mut group = c.benchmark_group("reed_solomon_original");

    group.bench_function("setup_compute_process", |b| {
        b.iter(|| {
            let mut rs = OriginalRS::new();
            rs.set_input(&[true, true, false, true]).unwrap(); // 1 missing data block
            rs.set_output(true, 0).unwrap(); // 1 present recovery
            rs.set_output(false, 1).unwrap(); // 1 missing recovery
            rs.compute().unwrap();

            let input = vec![0xAAu8; 528]; // PAR2 block size
            let mut output = vec![0x55u8; 528];

            // Process multiple times (simulates real usage)
            for i in 0..4 {
                rs.process(i % 2, black_box(&input), 0, black_box(&mut output))
                    .unwrap();
            }
        })
    });

    group.bench_function("builder_pattern", |b| {
        b.iter(|| {
            let mut rs = ReedSolomonBuilder::new()
                .with_input_status(&[true, true, false, true]) // 1 missing data block
                .with_recovery_block(true, 0) // 1 present recovery
                .with_recovery_block(false, 1) // 1 missing recovery
                .build()
                .unwrap();

            rs.compute().unwrap();

            let input = vec![0xAAu8; 528];
            let mut output = vec![0x55u8; 528];

            for i in 0..4 {
                rs.process(i % 2, black_box(&input), 0, black_box(&mut output))
                    .unwrap();
            }
        })
    });
}

/// Benchmark the type-safe ReedSolomon implementation
fn bench_typestate_reed_solomon(c: &mut Criterion) {
    let mut group = c.benchmark_group("reed_solomon_typestate");

    group.bench_function("setup_compute_process", |b| {
        b.iter(|| {
            let rs = TypeSafeRS::new()
                .set_input(&[true, true, false, true])
                .unwrap() // 1 missing data block
                .set_output(true, 0)
                .unwrap() // 1 present recovery
                .set_output(false, 1)
                .unwrap() // 1 missing recovery
                .compute()
                .unwrap();

            let input = vec![0xAAu8; 528]; // PAR2 block size
            let mut output = vec![0x55u8; 528];

            // Process multiple times (simulates real usage)
            for i in 0..4 {
                rs.process(i % 2, black_box(&input), 0, black_box(&mut output))
                    .unwrap();
            }
        })
    });

    group.bench_function("builder_pattern", |b| {
        b.iter(|| {
            let rs = TypeSafeReedSolomonBuilder::new()
                .with_input_status(&[true, true, false, true]) // 1 missing data block
                .with_recovery_block(true, 0) // 1 present recovery
                .with_recovery_block(false, 1) // 1 missing recovery
                .build()
                .unwrap()
                .compute()
                .unwrap();

            let input = vec![0xAAu8; 528];
            let mut output = vec![0x55u8; 528];

            for i in 0..4 {
                rs.process(i % 2, black_box(&input), 0, black_box(&mut output))
                    .unwrap();
            }
        })
    });
}

/// Benchmark process() call specifically (the hot path)
fn bench_process_only(c: &mut Criterion) {
    let mut group = c.benchmark_group("reed_solomon_process_only");

    // Setup original RS
    let mut original_rs = OriginalRS::new();
    original_rs.set_input(&[true, true, false, true]).unwrap(); // 1 missing data block
    original_rs.set_output(true, 0).unwrap(); // 1 present recovery
    original_rs.set_output(false, 1).unwrap(); // 1 missing recovery
    original_rs.compute().unwrap();

    // Setup type-safe RS
    let typestate_rs = TypeSafeRS::new()
        .set_input(&[true, true, false, true])
        .unwrap() // 1 missing data block
        .set_output(true, 0)
        .unwrap() // 1 present recovery
        .set_output(false, 1)
        .unwrap() // 1 missing recovery
        .compute()
        .unwrap();

    let input = vec![0xAAu8; 528];

    group.bench_function("original", |b| {
        let mut output = vec![0x55u8; 528];
        b.iter(|| {
            original_rs
                .process(0, black_box(&input), 0, black_box(&mut output))
                .unwrap();
        })
    });

    group.bench_function("typestate", |b| {
        let mut output = vec![0x55u8; 528];
        b.iter(|| {
            typestate_rs
                .process(0, black_box(&input), 0, black_box(&mut output))
                .unwrap();
        })
    });
}

/// Benchmark different block sizes to ensure consistent performance
fn bench_different_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("reed_solomon_sizes");

    for size in [64, 128, 256, 528, 1024, 2048].iter() {
        // Setup original RS
        let mut original_rs = OriginalRS::new();
        original_rs.set_input(&[true, true, false]).unwrap(); // 1 missing data block
        original_rs.set_output(true, 0).unwrap(); // 1 present recovery
        original_rs.compute().unwrap();

        // Setup type-safe RS
        let typestate_rs = TypeSafeRS::new()
            .set_input(&[true, true, false])
            .unwrap() // 1 missing data block
            .set_output(true, 0)
            .unwrap() // 1 present recovery
            .compute()
            .unwrap();

        let input = vec![0xAAu8; *size];

        group.bench_with_input(BenchmarkId::new("original", size), size, |b, _| {
            let mut output = vec![0x55u8; *size];
            b.iter(|| {
                original_rs
                    .process(0, black_box(&input), 0, black_box(&mut output))
                    .unwrap();
            })
        });

        group.bench_with_input(BenchmarkId::new("typestate", size), size, |b, _| {
            let mut output = vec![0x55u8; *size];
            b.iter(|| {
                typestate_rs
                    .process(0, black_box(&input), 0, black_box(&mut output))
                    .unwrap();
            })
        });
    }
}

criterion_group!(
    benches,
    bench_original_reed_solomon,
    bench_typestate_reed_solomon,
    bench_process_only,
    bench_different_sizes
);
criterion_main!(benches);
