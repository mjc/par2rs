use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use par2rs::create::{CreateContextBuilder, SilentCreateReporter};
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use tempfile::tempdir;

fn create_test_file(size: usize) -> (tempfile::TempDir, PathBuf) {
    let temp_dir = tempdir().unwrap();
    let file_path = temp_dir.path().join("test.dat");

    // Create file with pattern to avoid compression optimizations
    let pattern = (0..256)
        .cycle()
        .take(size)
        .map(|i| i as u8)
        .collect::<Vec<_>>();
    fs::write(&file_path, pattern).unwrap();

    (temp_dir, file_path)
}

fn bench_par2_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("par2_creation");

    // Test different file sizes
    let sizes = vec![
        ("1KB", 1024),
        ("10KB", 10 * 1024),
        ("100KB", 100 * 1024),
        ("1MB", 1024 * 1024),
        ("10MB", 10 * 1024 * 1024),
    ];

    for (size_name, size) in sizes {
        group.bench_with_input(
            BenchmarkId::new("create_file", size_name),
            &size,
            |b, &size| {
                b.iter_batched(
                    || create_test_file(size),
                    |(temp_dir, test_file)| {
                        let par2_file = temp_dir.path().join("test.par2");
                        let reporter = Box::new(SilentCreateReporter);

                        let mut context = CreateContextBuilder::new()
                            .output_name(par2_file.to_str().unwrap())
                            .source_files(vec![test_file])
                            .redundancy_percentage(5)
                            .reporter(reporter)
                            .build()
                            .unwrap();

                        context.create().unwrap();
                        black_box(());
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_par2_creation_redundancy(c: &mut Criterion) {
    let mut group = c.benchmark_group("par2_creation_redundancy");

    // Test different redundancy levels with 1MB file
    let redundancy_levels = vec![5, 10, 20, 50];

    for redundancy in redundancy_levels {
        group.bench_with_input(
            BenchmarkId::new("redundancy", format!("{}%", redundancy)),
            &redundancy,
            |b, &redundancy| {
                b.iter_batched(
                    || create_test_file(1024 * 1024), // 1MB
                    |(temp_dir, test_file)| {
                        let par2_file = temp_dir.path().join("test.par2");
                        let reporter = Box::new(SilentCreateReporter);

                        let mut context = CreateContextBuilder::new()
                            .output_name(par2_file.to_str().unwrap())
                            .source_files(vec![test_file])
                            .redundancy_percentage(redundancy)
                            .reporter(reporter)
                            .build()
                            .unwrap();

                        context.create().unwrap();
                        black_box(());
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_par2_creation_multifile(c: &mut Criterion) {
    let mut group = c.benchmark_group("par2_creation_multifile");

    // Test different numbers of files (each 100KB)
    let file_counts = vec![1, 3, 5, 10];

    for file_count in file_counts {
        group.bench_with_input(
            BenchmarkId::new("files", format!("{}_files", file_count)),
            &file_count,
            |b, &file_count| {
                b.iter_batched(
                    || {
                        let temp_dir = tempdir().unwrap();
                        let mut files = Vec::new();

                        for i in 0..file_count {
                            let file_path = temp_dir.path().join(format!("test{}.dat", i));
                            let pattern = (0..256)
                                .cycle()
                                .take(100 * 1024)
                                .map(|x| (x + i) as u8)
                                .collect::<Vec<_>>();
                            fs::write(&file_path, pattern).unwrap();
                            files.push(file_path);
                        }

                        (temp_dir, files)
                    },
                    |(temp_dir, test_files)| {
                        let par2_file = temp_dir.path().join("test.par2");
                        let reporter = Box::new(SilentCreateReporter);

                        let mut context = CreateContextBuilder::new()
                            .output_name(par2_file.to_str().unwrap())
                            .source_files(test_files)
                            .redundancy_percentage(5)
                            .reporter(reporter)
                            .build()
                            .unwrap();

                        context.create().unwrap();
                        black_box(());
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_block_size_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_size_calculation");

    // Test block size calculation performance
    let sizes = vec![
        ("1MB", 1024 * 1024),
        ("10MB", 10 * 1024 * 1024),
        ("100MB", 100 * 1024 * 1024),
    ];

    for (size_name, size) in sizes {
        group.bench_with_input(
            BenchmarkId::new("calculate", size_name),
            &size,
            |b, &size| {
                b.iter_batched(
                    || create_test_file(size),
                    |(temp_dir, test_file)| {
                        let par2_file = temp_dir.path().join("test.par2");
                        let reporter = Box::new(SilentCreateReporter);

                        // This will trigger block size calculation
                        let context = CreateContextBuilder::new()
                            .output_name(par2_file.to_str().unwrap())
                            .source_files(vec![test_file])
                            .redundancy_percentage(10)
                            .reporter(reporter)
                            .build()
                            .unwrap();

                        black_box(context.block_size());
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_par2_creation,
    bench_par2_creation_redundancy,
    bench_par2_creation_multifile,
    bench_block_size_calculation
);
criterion_main!(benches);
