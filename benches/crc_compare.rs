//! Bench: `crc32fast` vs `crc-fast` over realistic par2 block sizes.
//!
//! Decision driver for whether to swap the CRC backend. We care about:
//!   - 64 B: the fused HasherInput inner-loop granularity.
//!   - 16 KiB: the Tier-1 sub-slice size used by `update_md5_crc32_fused`.
//!   - 4 MiB & 64 MiB: full-buffer one-shots representative of large par2 blocks.
//!
//! Both crates produce IsoHdlc/zlib CRC-32. Each bench also asserts equality
//! on the first iteration so we don't ship a result that doesn't match.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

fn make_data(len: usize) -> Vec<u8> {
    // Deterministic non-zero pattern so optimizers can't constant-fold.
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(7))
        .collect()
}

fn crc32fast_one_shot(data: &[u8]) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(data);
    h.finalize()
}

fn crc_fast_one_shot(data: &[u8]) -> u32 {
    use crc_fast::{CrcAlgorithm, Digest};
    let mut d = Digest::new(CrcAlgorithm::Crc32IsoHdlc);
    d.update(data);
    d.finalize() as u32
}

fn bench_oneshot(c: &mut Criterion) {
    let sizes = [64usize, 16 * 1024, 4 * 1024 * 1024, 64 * 1024 * 1024];
    let mut group = c.benchmark_group("crc_oneshot");

    for &len in &sizes {
        let data = make_data(len);

        // Sanity: both backends must agree.
        assert_eq!(
            crc32fast_one_shot(&data),
            crc_fast_one_shot(&data),
            "crc32fast and crc-fast disagree at len={len}"
        );

        group.throughput(Throughput::Bytes(len as u64));

        group.bench_with_input(BenchmarkId::new("crc32fast", len), &data, |b, d| {
            b.iter(|| black_box(crc32fast_one_shot(black_box(d))));
        });
        group.bench_with_input(BenchmarkId::new("crc-fast", len), &data, |b, d| {
            b.iter(|| black_box(crc_fast_one_shot(black_box(d))));
        });
    }

    group.finish();
}

/// Streaming bench: feed 64 B at a time. This is the actual fused-HasherInput
/// access pattern — what matters for the T2.c port decision.
fn bench_streaming_64b(c: &mut Criterion) {
    let total_sizes = [16 * 1024usize, 4 * 1024 * 1024];
    let mut group = c.benchmark_group("crc_stream_64B");

    for &total in &total_sizes {
        let data = make_data(total);
        group.throughput(Throughput::Bytes(total as u64));

        group.bench_with_input(BenchmarkId::new("crc32fast", total), &data, |b, d| {
            b.iter(|| {
                let mut h = crc32fast::Hasher::new();
                for chunk in d.chunks_exact(64) {
                    h.update(black_box(chunk));
                }
                black_box(h.finalize())
            });
        });
        group.bench_with_input(BenchmarkId::new("crc-fast", total), &data, |b, d| {
            b.iter(|| {
                use crc_fast::{CrcAlgorithm, Digest};
                let mut h = Digest::new(CrcAlgorithm::Crc32IsoHdlc);
                for chunk in d.chunks_exact(64) {
                    h.update(black_box(chunk));
                }
                black_box(h.finalize() as u32)
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_oneshot, bench_streaming_64b);
criterion_main!(benches);
