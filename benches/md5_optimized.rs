use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use par2rs::checksum::{calculate_file_md5, calculate_file_md5_16k};
use std::fs::File;
use std::hint::black_box;
use std::io::Write;
use tempfile::TempDir;

fn create_test_file(size: usize) -> (TempDir, std::path::PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test_file.bin");

    let mut file = File::create(&file_path).unwrap();
    let chunk = vec![0x42u8; 1024 * 1024]; // 1MB chunks

    for _ in 0..(size / (1024 * 1024)) {
        file.write_all(&chunk).unwrap();
    }

    // Handle remainder
    let remainder = size % (1024 * 1024);
    if remainder > 0 {
        file.write_all(&chunk[..remainder]).unwrap();
    }

    file.sync_all().unwrap();
    drop(file);

    (temp_dir, file_path)
}

fn bench_md5_full_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("md5_full_file");

    let sizes = [
        (10 * 1024 * 1024, "10MB"),
        (100 * 1024 * 1024, "100MB"),
        (500 * 1024 * 1024, "500MB"),
        (1000 * 1024 * 1024, "1GB"),
    ];

    for (size, name) in sizes.iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        let (_temp_dir, file_path) = create_test_file(*size);

        group.bench_with_input(
            BenchmarkId::new("calculate_file_md5", name),
            size,
            |b, _| {
                b.iter(|| {
                    let result = calculate_file_md5(black_box(&file_path));
                    black_box(result.unwrap());
                });
            },
        );
    }

    group.finish();
}

fn bench_md5_16k(c: &mut Criterion) {
    let mut group = c.benchmark_group("md5_16k_fast_check");

    let sizes = [
        (1 * 1024 * 1024, "1MB"),
        (100 * 1024 * 1024, "100MB"),
        (1000 * 1024 * 1024, "1GB"),
    ];

    for (size, name) in sizes.iter() {
        // Throughput is always 16KB regardless of file size
        group.throughput(Throughput::Bytes(16384));

        let (_temp_dir, file_path) = create_test_file(*size);

        group.bench_with_input(
            BenchmarkId::new("calculate_file_md5_16k", name),
            size,
            |b, _| {
                b.iter(|| {
                    let result = calculate_file_md5_16k(black_box(&file_path));
                    black_box(result.unwrap());
                });
            },
        );
    }

    group.finish();
}

fn bench_verification_workflow(c: &mut Criterion) {
    let mut group = c.benchmark_group("verification_workflow");

    // Simulate typical PAR2 verification: 16KB check first, then full if needed
    let size = 100 * 1024 * 1024; // 100MB file
    group.throughput(Throughput::Bytes(size as u64));

    let (_temp_dir, file_path) = create_test_file(size);

    group.bench_function("fast_check_then_full", |b| {
        b.iter(|| {
            // First: fast 16KB check (this would normally be compared against expected)
            let fast_hash = calculate_file_md5_16k(black_box(&file_path)).unwrap();
            black_box(fast_hash);

            // Then: full file hash (in real scenario, only if 16KB matched)
            let full_hash = calculate_file_md5(black_box(&file_path)).unwrap();
            black_box(full_hash);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_md5_full_file,
    bench_md5_16k,
    bench_verification_workflow
);
criterion_main!(benches);
