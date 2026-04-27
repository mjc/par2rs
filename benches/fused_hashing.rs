//! Microbenchmark for the Tier 1 fused-hashing helpers in `src/checksum.rs`.
//!
//! Compares four strategies over the same input buffer:
//!
//!   * `naive_2pass`     — `Md5::update(whole)` then `Crc32::update(whole)`.
//!     This is what `par2cmdline-turbo` would call the "double scan" baseline:
//!     for any buffer larger than L1d, the second pass eats cold cache lines.
//!
//!   * `fused_2hash`     — `compute_md5_crc32_simultaneous`, which walks the
//!     buffer in 16 KiB sub-slices feeding both hashers back-to-back so the
//!     CRC reads what MD5 just brought into L1.
//!
//!   * `naive_3pass`     — file-MD5 + block-MD5 + CRC32 each as full-buffer
//!     passes. Mirrors the pre-Tier-1 create() loop where the chunk was
//!     scanned three times.
//!
//!   * `fused_3hash`     — `update_file_md5_block_md5_crc32_fused`, the
//!     three-hasher variant that backs `encode_and_hash_files` post-Tier-1.
//!
//! The interesting buffer sizes are the ones that span L1 / L2 / L3 / DRAM:
//! 16 KiB sits exactly in L1, 256 KiB falls out of L1, 4 MiB falls out of L2,
//! 64 MiB falls out of L3 on most desktops. The fused helpers should shine
//! once the buffer is bigger than L1.
//!
//! For the cache-miss number itself, run under `perf stat`:
//!
//! ```text
//! perf stat -e cache-misses,cache-references,L1-dcache-load-misses,\
//!   LLC-load-misses,instructions,cycles,task-clock \
//!   cargo bench --bench fused_hashing -- --profile-time 5 fused_2hash/4MiB
//! ```

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

use crc32fast::Hasher as Crc32Hasher;
use md5::{Digest, Md5};

use par2rs::checksum::{compute_md5_crc32_simultaneous, update_file_md5_block_md5_crc32_fused};

/// Buffer sizes, chosen to span L1 / L2 / L3 / DRAM on a typical desktop.
const SIZES: &[(&str, usize)] = &[
    ("16KiB", 16 * 1024),        // fits in L1d (~32-48 KiB)
    ("256KiB", 256 * 1024),      // fits in L2 (~256 KiB-1 MiB)
    ("4MiB", 4 * 1024 * 1024),   // fits in L3
    ("64MiB", 64 * 1024 * 1024), // exceeds typical L3
];

fn make_buffer(size: usize) -> Vec<u8> {
    // Repeating low-entropy pattern is fine: we're measuring memory traffic +
    // hash compress throughput, not anything data-dependent.
    (0..size).map(|i| (i & 0xff) as u8).collect()
}

fn bench_two_hashes(c: &mut Criterion) {
    let mut group = c.benchmark_group("two_hashes_md5_crc32");

    for &(label, size) in SIZES {
        let data = make_buffer(size);
        group.throughput(Throughput::Bytes(size as u64));

        // Pre-Tier-1 baseline: two full-buffer scans.
        group.bench_with_input(BenchmarkId::new("naive_2pass", label), &data, |b, data| {
            b.iter(|| {
                let mut md5 = Md5::new();
                let mut crc = Crc32Hasher::new();
                md5.update(black_box(data));
                crc.update(black_box(data));
                black_box((md5.finalize(), crc.finalize()))
            });
        });

        // Tier 1: cache-resident sub-slice walker.
        group.bench_with_input(BenchmarkId::new("fused_2hash", label), &data, |b, data| {
            b.iter(|| black_box(compute_md5_crc32_simultaneous(black_box(data))));
        });
    }

    group.finish();
}

fn bench_three_hashes(c: &mut Criterion) {
    let mut group = c.benchmark_group("three_hashes_file_block_crc");

    for &(label, size) in SIZES {
        let data = make_buffer(size);
        group.throughput(Throughput::Bytes(size as u64));

        // Pre-Tier-1: three full-buffer scans (the original create() loop).
        group.bench_with_input(BenchmarkId::new("naive_3pass", label), &data, |b, data| {
            b.iter(|| {
                let mut file_md5 = Md5::new();
                let mut block_md5 = Md5::new();
                let mut crc = Crc32Hasher::new();
                file_md5.update(black_box(data));
                block_md5.update(black_box(data));
                crc.update(black_box(data));
                black_box((file_md5.finalize(), block_md5.finalize(), crc.finalize()))
            });
        });

        // Tier 1: cache-resident three-hasher walker.
        group.bench_with_input(BenchmarkId::new("fused_3hash", label), &data, |b, data| {
            b.iter(|| {
                let mut file_md5 = Md5::new();
                let mut block_md5 = Md5::new();
                let mut crc = Crc32Hasher::new();
                update_file_md5_block_md5_crc32_fused(
                    &mut file_md5,
                    &mut block_md5,
                    &mut crc,
                    black_box(data),
                );
                black_box((file_md5.finalize(), block_md5.finalize(), crc.finalize()))
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_two_hashes, bench_three_hashes);
criterion_main!(benches);
