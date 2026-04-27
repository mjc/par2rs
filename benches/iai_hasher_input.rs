//! iai-callgrind instruction-count benchmark for the ParPar HasherInput
//! port (T2.c). Complement to `parpar_hasher_input.rs` (criterion
//! wall-clock): this one measures deterministic instruction / branch /
//! cache counts so regressions show up without timing noise.
//!
//! Compares the same three implementations:
//!   1. naive 3-pass    (md-5 + crc32fast)
//!   2. tier-1 helper   (`update_file_md5_block_md5_crc32_fused`)
//!   3. HasherInput     (the new ParPar fused 64 B driver)
//!
//! at two single-block sizes (16 KiB sub-slice, 4 MiB block).

use iai_callgrind::{library_benchmark, library_benchmark_group, main};
use md5::Digest as _;
use md5::Md5;
use par2rs::checksum::update_file_md5_block_md5_crc32_fused;
#[cfg(target_arch = "x86_64")]
use par2rs::parpar_hasher::hasher_input::HasherInput;
use std::hint::black_box;

type Out = ([u8; 16], [u8; 16], u32);

fn make_data(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(7))
        .collect()
}

// Naive 3-pass: file-MD5 + block-MD5 + block-CRC, three independent walks.
fn naive_seq(data: &[u8]) -> Out {
    let mut f = Md5::new();
    f.update(data);
    let mut b = Md5::new();
    b.update(data);
    let mut c = crc32fast::Hasher::new();
    c.update(data);
    (f.finalize().into(), b.finalize().into(), c.finalize())
}

// Tier-1 helper: one walk, three single-lane hashers.
fn tier1_helper(data: &[u8]) -> Out {
    let mut f = Md5::new();
    let mut b = Md5::new();
    let mut c = crc32fast::Hasher::new();
    update_file_md5_block_md5_crc32_fused(&mut f, &mut b, &mut c, data);
    (f.finalize().into(), b.finalize().into(), c.finalize())
}

// HasherInput: MD5x2 + CLMul CRC32 fused at 64 B granularity.
// Single-block scenario: one update + one get_block(0) + one end().
#[cfg(target_arch = "x86_64")]
fn hasher_input(data: &[u8]) -> Out {
    let mut h = HasherInput::new();
    h.update(data);
    let bh = h.get_block(0);
    let file = h.end();
    (file, bh.md5, bh.crc32)
}

// ----------------------- 16 KiB single block -----------------------

#[library_benchmark]
#[bench::sixteen_kib(make_data(16 * 1024))]
fn bench_naive_seq_16k(data: Vec<u8>) -> Out {
    black_box(naive_seq(black_box(&data)))
}

#[library_benchmark]
#[bench::sixteen_kib(make_data(16 * 1024))]
fn bench_tier1_helper_16k(data: Vec<u8>) -> Out {
    black_box(tier1_helper(black_box(&data)))
}

#[cfg(target_arch = "x86_64")]
#[library_benchmark]
#[bench::sixteen_kib(make_data(16 * 1024))]
fn bench_hasher_input_16k(data: Vec<u8>) -> Out {
    black_box(hasher_input(black_box(&data)))
}

// ----------------------- 4 MiB single block ------------------------

#[library_benchmark]
#[bench::four_mib(make_data(4 * 1024 * 1024))]
fn bench_naive_seq_4m(data: Vec<u8>) -> Out {
    black_box(naive_seq(black_box(&data)))
}

#[library_benchmark]
#[bench::four_mib(make_data(4 * 1024 * 1024))]
fn bench_tier1_helper_4m(data: Vec<u8>) -> Out {
    black_box(tier1_helper(black_box(&data)))
}

#[cfg(target_arch = "x86_64")]
#[library_benchmark]
#[bench::four_mib(make_data(4 * 1024 * 1024))]
fn bench_hasher_input_4m(data: Vec<u8>) -> Out {
    black_box(hasher_input(black_box(&data)))
}

// ----------------------- groups + main -----------------------------

#[cfg(target_arch = "x86_64")]
library_benchmark_group!(
    name = hasher_input_group;
    benchmarks =
        bench_naive_seq_16k,
        bench_tier1_helper_16k,
        bench_hasher_input_16k,
        bench_naive_seq_4m,
        bench_tier1_helper_4m,
        bench_hasher_input_4m
);

#[cfg(not(target_arch = "x86_64"))]
library_benchmark_group!(
    name = hasher_input_group;
    benchmarks =
        bench_naive_seq_16k,
        bench_tier1_helper_16k,
        bench_naive_seq_4m,
        bench_tier1_helper_4m
);

main!(library_benchmark_groups = hasher_input_group);
