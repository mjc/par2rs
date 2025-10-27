use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::fs::File;
use std::hint::black_box;
use std::io::Write;

// Old approach: compute MD5 twice (16k + full)
fn compute_md5_old_way(file_path: &str) -> (par2rs::domain::Md5Hash, par2rs::domain::Md5Hash) {
    use md5::{Digest, Md5};
    use std::io::Read;

    // First pass: 16k MD5
    let mut file = File::open(file_path).unwrap();
    let mut hasher = Md5::new();
    let mut buffer = [0u8; 16384];
    let bytes_read = file.read(&mut buffer).unwrap();
    hasher.update(&buffer[..bytes_read]);
    let hash_16k = par2rs::domain::Md5Hash::new(hasher.finalize().into());

    // Second pass: full file MD5
    let mut file = File::open(file_path).unwrap();
    let mut hasher = Md5::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let bytes_read = file.read(&mut buffer).unwrap();
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    let hash_full = par2rs::domain::Md5Hash::new(hasher.finalize().into());

    (hash_16k, hash_full)
}

// New approach: single pass
fn compute_md5_new_way(file_path: &str) -> (par2rs::domain::Md5Hash, par2rs::domain::Md5Hash) {
    use par2rs::file_checksummer::FileCheckSummer;

    let checksummer = FileCheckSummer::new(file_path.to_string(), 1024).unwrap();
    let results = checksummer.compute_file_hashes().unwrap();

    (results.hash_16k, results.hash_full)
}

fn bench_verification_approaches(c: &mut Criterion) {
    let sizes = vec![
        ("10MB", 10 * 1024 * 1024),
        ("100MB", 100 * 1024 * 1024),
        ("1GB", 1024 * 1024 * 1024),
    ];

    for (name, size) in sizes {
        let test_file = format!("/tmp/verify_test_{}.bin", name);

        // Create test file
        {
            let mut file = std::fs::File::create(&test_file).unwrap();
            let chunk_size = 1024 * 1024; // Write in 1MB chunks
            let chunk = vec![0xAB_u8; chunk_size];
            for _ in 0..(size / chunk_size) {
                file.write_all(&chunk).unwrap();
            }
            file.sync_all().unwrap();
        }

        let mut group = c.benchmark_group(format!("verify_{}", name));
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_function("old_two_pass", |b| {
            b.iter(|| black_box(compute_md5_old_way(&test_file)))
        });

        group.bench_function("new_single_pass", |b| {
            b.iter(|| black_box(compute_md5_new_way(&test_file)))
        });

        group.finish();

        // Cleanup
        std::fs::remove_file(&test_file).ok();
    }
}

criterion_group!(benches, bench_verification_approaches);
criterion_main!(benches);
