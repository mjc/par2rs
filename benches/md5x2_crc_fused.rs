//! End-to-end fused-hashing comparison for the par2 create path.
//!
//! Each variant computes the three outputs the create path needs:
//!   - file-MD5    (rolls across all blocks of a file)
//!   - block-MD5   (per source block)
//!   - block-CRC32 (per source block)
//!
//! 5 variants × 2 sizes (16 KiB sub-slice and 4 MiB block).
//!
//! All variants are sanity-checked at startup against the naive
//! reference: any divergence aborts the run.
//!
//! Variants:
//!   1. naive_seq            — three independent passes (md-5 + md-5 + crc32fast).
//!   2. tier1_subslice       — current shipped helper:
//!                             `update_file_md5_block_md5_crc32_fused`
//!                             interleaves all three at 16 KiB sub-slice
//!                             granularity (single-lane MD5s + crc32fast).
//!   3. md5x2_crc32fast_64b  — MD5x2 absorbs block+file MD5; crc32fast
//!                             streamed in 64 B chunks.
//!   4. md5x2_crcfast_64b    — same, but `crc_fast::Digest` for CRC.
//!   5. md5x2_only           — lower bound: MD5x2 with no CRC.
//!
//! The MD5x2 lanes are fed the same 64 B for harness simplicity; this is
//! what the real `HasherInput` will do when the file is 64 B-aligned and
//! the staggered position offset is 0. (Stagger overhead is a separate
//! concern for T2.c, not the backend choice here.)

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use md5::Digest as _;
use md5::Md5;
use par2rs::checksum::update_file_md5_block_md5_crc32_fused;
use par2rs::parpar_hasher::md5x2_scalar;
use std::hint::black_box;

type Out = ([u8; 16], [u8; 16], u32); // (file_md5, block_md5, crc32)

fn make_data(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(7))
        .collect()
}

// ---------- variants ----------

fn v1_naive_seq(data: &[u8]) -> Out {
    let mut f = Md5::new();
    f.update(data);
    let mut b = Md5::new();
    b.update(data);
    let mut c = crc32fast::Hasher::new();
    c.update(data);
    (f.finalize().into(), b.finalize().into(), c.finalize())
}

fn v2_tier1_subslice(data: &[u8]) -> Out {
    let mut f = Md5::new();
    let mut b = Md5::new();
    let mut c = crc32fast::Hasher::new();
    update_file_md5_block_md5_crc32_fused(&mut f, &mut b, &mut c, data);
    (f.finalize().into(), b.finalize().into(), c.finalize())
}

/// MD5x2 produces *two* MD5 results from one walk. We use lane 1 for
/// file-MD5 and lane 2 for block-MD5; this bench feeds the same 64 B
/// to both lanes so the two lanes' final digests are identical, but
/// the cost profile (two MD5s + CRC in a single pass) matches what
/// HasherInput will pay.
fn md5x2_finalise(mut state: [u32; 8], total_len: usize) -> ([u8; 16], [u8; 16]) {
    // Bench harness only feeds chunks_exact(64), so the partial buffer is
    // empty at finalise time and one tail block suffices: 0x80, then zero
    // pad to 56 mod 64, then 8-byte LE bit-length.
    let bit_len = (total_len as u64).wrapping_mul(8);
    let mut tail = [0u8; 64];
    tail[0] = 0x80;
    tail[56..64].copy_from_slice(&bit_len.to_le_bytes());
    // Both lanes finalise in one MD5x2 step: same tail in both lanes,
    // independent state per lane.
    unsafe {
        md5x2_scalar::process_block_x2_scalar(&mut state, tail.as_ptr(), tail.as_ptr());
    }
    let mut out1 = [0u8; 16];
    let mut out2 = [0u8; 16];
    for i in 0..4 {
        out1[i * 4..i * 4 + 4].copy_from_slice(&state[i].to_le_bytes());
        out2[i * 4..i * 4 + 4].copy_from_slice(&state[i + 4].to_le_bytes());
    }
    (out1, out2)
}

fn v3_md5x2_crc32fast(data: &[u8]) -> Out {
    let mut state = md5x2_scalar::init_state();
    let mut crc = crc32fast::Hasher::new();
    for chunk in data.chunks_exact(64) {
        unsafe {
            md5x2_scalar::process_block_x2_scalar(&mut state, chunk.as_ptr(), chunk.as_ptr());
        }
        crc.update(chunk);
    }
    let (lane1, lane2) = md5x2_finalise(state, data.len());
    (lane1, lane2, crc.finalize())
}

fn v4_md5x2_crcfast(data: &[u8]) -> Out {
    use crc_fast::{CrcAlgorithm, Digest};
    let mut state = md5x2_scalar::init_state();
    let mut crc = Digest::new(CrcAlgorithm::Crc32IsoHdlc);
    for chunk in data.chunks_exact(64) {
        unsafe {
            md5x2_scalar::process_block_x2_scalar(&mut state, chunk.as_ptr(), chunk.as_ptr());
        }
        crc.update(chunk);
    }
    let (lane1, lane2) = md5x2_finalise(state, data.len());
    (lane1, lane2, crc.finalize() as u32)
}

fn v5_md5x2_only(data: &[u8]) -> ([u8; 16], [u8; 16]) {
    let mut state = md5x2_scalar::init_state();
    for chunk in data.chunks_exact(64) {
        unsafe {
            md5x2_scalar::process_block_x2_scalar(&mut state, chunk.as_ptr(), chunk.as_ptr());
        }
    }
    md5x2_finalise(state, data.len())
}

// ---------- bench ----------

fn bench(c: &mut Criterion) {
    let sizes = [16 * 1024usize, 4 * 1024 * 1024];
    let mut group = c.benchmark_group("create_path_hashing");

    // Sanity-check correctness at every size before benching.
    for &len in &sizes {
        assert!(
            len % 64 == 0,
            "bench sizes must be 64-aligned for MD5x2 harness"
        );
        let d = make_data(len);
        let r1 = v1_naive_seq(&d);
        let r2 = v2_tier1_subslice(&d);
        let r3 = v3_md5x2_crc32fast(&d);
        let r4 = v4_md5x2_crcfast(&d);
        let r5 = v5_md5x2_only(&d);

        // CRC: variants 1, 2, 3 must match (all crc32fast); variant 4 same value.
        assert_eq!(r1.2, r2.2);
        assert_eq!(r1.2, r3.2);
        assert_eq!(r1.2, r4.2, "crc-fast disagrees with crc32fast at len={len}");

        // Naive single-lane MD5 == MD5x2 lane (since both lanes get the
        // same input, MD5x2 lane1 == MD5x2 lane2 == single-lane MD5).
        assert_eq!(
            r1.0, r3.0,
            "MD5x2 lane1 != single-lane MD5 at len={len} (v3)"
        );
        assert_eq!(
            r1.0, r3.1,
            "MD5x2 lane2 != single-lane MD5 at len={len} (v3)"
        );
        assert_eq!(r3.0, r4.0);
        assert_eq!(r3.0, r5.0);
        assert_eq!(r3.1, r5.1);
    }

    for &len in &sizes {
        let data = make_data(len);
        group.throughput(Throughput::Bytes(len as u64));

        group.bench_with_input(BenchmarkId::new("1_naive_seq", len), &data, |b, d| {
            b.iter(|| black_box(v1_naive_seq(black_box(d))));
        });
        group.bench_with_input(BenchmarkId::new("2_tier1_subslice", len), &data, |b, d| {
            b.iter(|| black_box(v2_tier1_subslice(black_box(d))));
        });
        group.bench_with_input(BenchmarkId::new("3_md5x2+crc32fast", len), &data, |b, d| {
            b.iter(|| black_box(v3_md5x2_crc32fast(black_box(d))))
        });
        group.bench_with_input(BenchmarkId::new("4_md5x2+crc-fast", len), &data, |b, d| {
            b.iter(|| black_box(v4_md5x2_crcfast(black_box(d))));
        });
        group.bench_with_input(BenchmarkId::new("5_md5x2_only", len), &data, |b, d| {
            b.iter(|| black_box(v5_md5x2_only(black_box(d))));
        });
    }

    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
