use iai_callgrind::{library_benchmark, library_benchmark_group, main};
use par2rs::reed_solomon::codec::build_split_mul_table;
use par2rs::reed_solomon::galois::Galois16;
use par2rs::reed_solomon::simd::process_slice_multiply_add_portable_simd;
#[cfg(target_arch = "x86_64")]
use par2rs::reed_solomon::simd::{process_slice_multiply_add_simd, SimdLevel};
use std::hint::black_box;

#[library_benchmark]
#[bench::small(vec![0xAAu8; 528])]
#[bench::medium(vec![0xAAu8; 4096])]
#[bench::large(vec![0xAAu8; 65536])]
fn bench_pshufb_simd(input: Vec<u8>) -> Vec<u8> {
    let coefficient = 0x1234u16;
    let gf = Galois16::new(coefficient);
    let tables = build_split_mul_table(gf);
    let mut output = vec![0x55u8; input.len()];

    #[cfg(target_arch = "x86_64")]
    process_slice_multiply_add_simd(
        black_box(&input),
        black_box(&mut output),
        black_box(&tables),
        SimdLevel::Avx2,
    );

    output
}

#[library_benchmark]
#[bench::small(vec![0xAAu8; 528])]
#[bench::medium(vec![0xAAu8; 4096])]
#[bench::large(vec![0xAAu8; 65536])]
fn bench_portable_simd(input: Vec<u8>) -> Vec<u8> {
    let coefficient = 0x1234u16;
    let gf = Galois16::new(coefficient);
    let tables = build_split_mul_table(gf);
    let mut output = vec![0x55u8; input.len()];

    unsafe {
        process_slice_multiply_add_portable_simd(
            black_box(&input),
            black_box(&mut output),
            black_box(&tables),
        );
    }

    output
}

#[library_benchmark]
#[bench::small(vec![0xAAu8; 528])]
#[bench::medium(vec![0xAAu8; 4096])]
#[bench::large(vec![0xAAu8; 65536])]
fn bench_scalar_baseline(input: Vec<u8>) -> Vec<u8> {
    let coefficient = 0x1234u16;
    let gf = Galois16::new(coefficient);
    let tables = build_split_mul_table(gf);
    let mut output = vec![0x55u8; input.len()];

    unsafe {
        par2rs::reed_solomon::simd::process_slice_multiply_add_scalar(
            black_box(&input),
            black_box(&mut output),
            black_box(&tables),
        );
    }

    output
}

library_benchmark_group!(
    name = simd_group;
    benchmarks = bench_pshufb_simd, bench_portable_simd, bench_scalar_baseline
);

main!(library_benchmark_groups = simd_group);
