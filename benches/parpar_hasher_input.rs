//! Benchmark for the ported ParPar `HasherInput` fused driver (T2.c).
//!
//! Compares three implementations of the create-path workload —
//! producing per-file MD5 + per-block MD5 + per-block CRC32 over a
//! sequence of source bytes:
//!
//!   1. `naive_seq`    — three independent passes (file MD5, block MD5,
//!                       block CRC32) using `md-5` + `crc32fast`. The
//!                       upper bound on naive throughput.
//!   2. `tier1_helper` — the currently-shipped Tier-1 sub-slice helper
//!                       `update_file_md5_block_md5_crc32_fused` driving
//!                       three single-lane hashers from one walk.
//!   3. `hasher_input` — the new ParPar HasherInput port: MD5x2
//!                       (block-MD5 + file-MD5 in one walk via ILP) +
//!                       PCLMULQDQ CRC32, fused at 64 B granularity.
//!
//! Two scenarios:
//!
//!   * `single_block_<size>` — exactly one PAR2 block, no zero-pad.
//!     Driver is reset per iteration. Sizes: 16 KiB, 4 MiB.
//!   * `multi_block_<count>x<size>` — N blocks of `block_size` bytes
//!     plus a `block_size/2` short tail. Exercises the steady-state
//!     loop AND the staggered-offset get_block path. Default: 4×4 MiB.
//!
//! Correctness: every variant is sanity-checked once at startup
//! against the naive reference; any mismatch panics before timing.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use md5::Digest as _;
use md5::Md5;
use par2rs::checksum::update_file_md5_block_md5_crc32_fused;
use par2rs::parpar_hasher::hasher_input::HasherInput;
use par2rs::parpar_hasher::md5x2::Md5x2;
use par2rs::parpar_hasher::md5x2_scalar::Scalar;
use par2rs::parpar_hasher::md5x2_sse2::Sse2;
use std::hint::black_box;

/// (file_md5, Vec<(block_md5, block_crc32)>)
type MultiBlockOut = ([u8; 16], Vec<([u8; 16], u32)>);

fn make_data(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(7))
        .collect()
}

// ----------------------------- Variants -----------------------------

fn v1_naive_seq_blocks(data: &[u8], block_size: usize) -> MultiBlockOut {
    let mut file = Md5::new();
    file.update(data);
    let file_md5: [u8; 16] = file.finalize().into();

    let mut blocks = Vec::new();
    let mut off = 0;
    while off < data.len() {
        let end = (off + block_size).min(data.len());
        let real = &data[off..end];
        let pad = block_size - real.len();

        let mut bm = Md5::new();
        bm.update(real);
        if pad > 0 {
            let zeros = vec![0u8; pad];
            bm.update(&zeros);
        }
        let bmd5: [u8; 16] = bm.finalize().into();

        let mut bc = crc32fast::Hasher::new();
        bc.update(real);
        if pad > 0 {
            let zeros = vec![0u8; pad];
            bc.update(&zeros);
        }
        blocks.push((bmd5, bc.finalize()));
        off = end;
    }
    (file_md5, blocks)
}

fn v2_tier1_helper_blocks(data: &[u8], block_size: usize) -> MultiBlockOut {
    let mut file = Md5::new();
    let mut blocks = Vec::new();
    let mut off = 0;
    while off < data.len() {
        let end = (off + block_size).min(data.len());
        let real = &data[off..end];
        let pad = block_size - real.len();

        let mut bm = Md5::new();
        let mut bc = crc32fast::Hasher::new();
        update_file_md5_block_md5_crc32_fused(&mut file, &mut bm, &mut bc, real);
        if pad > 0 {
            // Pad block-only (file MD5 already absorbed real bytes only,
            // matching naive: file is over data, NOT over zero-padded blocks).
            let zeros = vec![0u8; pad];
            bm.update(&zeros);
            bc.update(&zeros);
        }
        let bmd5: [u8; 16] = bm.finalize().into();
        blocks.push((bmd5, bc.finalize()));
        off = end;
    }
    let file_md5: [u8; 16] = file.finalize().into();
    (file_md5, blocks)
}

fn hasher_input_blocks<B: Md5x2>(data: &[u8], block_size: usize) -> MultiBlockOut {
    let mut h: HasherInput<B> = HasherInput::new();
    let mut blocks = Vec::new();
    let mut off = 0;
    let mut written_in_block = 0usize;

    while off < data.len() {
        let block_remaining = block_size - written_in_block;
        let take = block_remaining.min(data.len() - off);
        h.update(&data[off..off + take]);
        off += take;
        written_in_block += take;
        if written_in_block == block_size {
            let bh = h.get_block(0);
            blocks.push((bh.md5, bh.crc32));
            written_in_block = 0;
        }
    }
    if written_in_block > 0 {
        let pad = (block_size - written_in_block) as u64;
        let bh = h.get_block(pad);
        blocks.push((bh.md5, bh.crc32));
    }
    let file_md5 = h.end();
    (file_md5, blocks)
}

fn v3_hasher_input_scalar(data: &[u8], block_size: usize) -> MultiBlockOut {
    hasher_input_blocks::<Scalar>(data, block_size)
}

fn v4_hasher_input_sse2(data: &[u8], block_size: usize) -> MultiBlockOut {
    hasher_input_blocks::<Sse2>(data, block_size)
}

// ----------------------------- Bench -----------------------------

fn bench(c: &mut Criterion) {
    // Scenario A: a single full block. Highlights steady-state cost
    // without the get_block / cross-block plumbing.
    let single_sizes = [16 * 1024usize, 4 * 1024 * 1024];

    // Scenario B: a multi-block "file": four full blocks + one short
    // tail (half a block) that exercises zero-pad + carry between
    // get_block calls.
    let multi_block_size = 4 * 1024 * 1024;
    let multi_blocks = 4usize;
    let multi_total = multi_block_size * multi_blocks + multi_block_size / 2;

    // ---- Correctness sanity check (panics on divergence) ----
    for &len in &single_sizes {
        let data = make_data(len);
        let r1 = v1_naive_seq_blocks(&data, len);
        let r2 = v2_tier1_helper_blocks(&data, len);
        let r3 = v3_hasher_input_scalar(&data, len);
        let r4 = v4_hasher_input_sse2(&data, len);
        assert_eq!(r1, r2, "tier1 helper diverges from naive at len={len}");
        assert_eq!(
            r1, r3,
            "HasherInput<Scalar> diverges from naive at len={len}"
        );
        assert_eq!(r1, r4, "HasherInput<Sse2> diverges from naive at len={len}");
    }
    {
        let data = make_data(multi_total);
        let r1 = v1_naive_seq_blocks(&data, multi_block_size);
        let r2 = v2_tier1_helper_blocks(&data, multi_block_size);
        let r3 = v3_hasher_input_scalar(&data, multi_block_size);
        let r4 = v4_hasher_input_sse2(&data, multi_block_size);
        assert_eq!(r1, r2, "tier1 helper multi-block diverges");
        assert_eq!(r1, r3, "HasherInput<Scalar> multi-block diverges");
        assert_eq!(r1, r4, "HasherInput<Sse2> multi-block diverges");
    }

    // ---- Single-block group ----
    let mut g = c.benchmark_group("hasher_input_single_block");
    for &len in &single_sizes {
        let data = make_data(len);
        g.throughput(Throughput::Bytes(len as u64));
        g.bench_with_input(BenchmarkId::new("1_naive_seq", len), &data, |b, d| {
            b.iter(|| black_box(v1_naive_seq_blocks(black_box(d), len)))
        });
        g.bench_with_input(BenchmarkId::new("2_tier1_helper", len), &data, |b, d| {
            b.iter(|| black_box(v2_tier1_helper_blocks(black_box(d), len)))
        });
        g.bench_with_input(BenchmarkId::new("3_hasher_scalar", len), &data, |b, d| {
            b.iter(|| black_box(v3_hasher_input_scalar(black_box(d), len)))
        });
        g.bench_with_input(BenchmarkId::new("4_hasher_sse2", len), &data, |b, d| {
            b.iter(|| black_box(v4_hasher_input_sse2(black_box(d), len)))
        });
    }
    g.finish();

    // ---- Multi-block group ----
    let mut g = c.benchmark_group("hasher_input_multi_block");
    let data = make_data(multi_total);
    let label = format!("{multi_blocks}x{multi_block_size}+half");
    g.throughput(Throughput::Bytes(multi_total as u64));
    g.bench_with_input(BenchmarkId::new("1_naive_seq", &label), &data, |b, d| {
        b.iter(|| black_box(v1_naive_seq_blocks(black_box(d), multi_block_size)))
    });
    g.bench_with_input(BenchmarkId::new("2_tier1_helper", &label), &data, |b, d| {
        b.iter(|| black_box(v2_tier1_helper_blocks(black_box(d), multi_block_size)))
    });
    g.bench_with_input(
        BenchmarkId::new("3_hasher_scalar", &label),
        &data,
        |b, d| b.iter(|| black_box(v3_hasher_input_scalar(black_box(d), multi_block_size))),
    );
    g.bench_with_input(BenchmarkId::new("4_hasher_sse2", &label), &data, |b, d| {
        b.iter(|| black_box(v4_hasher_input_sse2(black_box(d), multi_block_size)))
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
