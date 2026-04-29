//! Instruction-count comparison for par2rs vs embedded ParPar hasher sources.

use iai_callgrind::{library_benchmark, library_benchmark_group, main};
use std::hint::black_box;

fn make_data(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(7))
        .collect()
}

#[cfg(target_arch = "x86_64")]
fn par2rs_md5_scalar(data: &[u8]) -> [u8; 16] {
    use par2rs::parpar_hasher::hasher_input::HasherInput;
    use par2rs::parpar_hasher::md5x2_scalar::Scalar;

    let mut hasher = HasherInput::<Scalar>::new();
    hasher.update(data);
    hasher.end()
}

#[cfg(target_arch = "x86_64")]
fn par2rs_md5_sse2(data: &[u8]) -> [u8; 16] {
    use par2rs::parpar_hasher::hasher_input::HasherInput;
    use par2rs::parpar_hasher::md5x2_sse2::Sse2;

    let mut hasher = HasherInput::<Sse2>::new();
    hasher.update(data);
    hasher.end()
}

#[cfg(target_arch = "x86_64")]
fn par2rs_md5_bmi1(data: &[u8]) -> [u8; 16] {
    use par2rs::parpar_hasher::hasher_input::HasherInput;
    use par2rs::parpar_hasher::md5x2_bmi1::Bmi1;

    let mut hasher = HasherInput::<Bmi1>::new();
    hasher.update(data);
    hasher.end()
}

fn par2rs_crc32(data: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

#[cfg(all(
    feature = "parpar-compare",
    target_arch = "x86_64",
    parpar_compare_embedded
))]
fn parpar_hasher_input(data: &[u8], method: par2rs::ffi::HasherInputMethod) -> [u8; 16] {
    use par2rs::ffi::hasher_input::ParParHasherInput;

    let mut hasher = ParParHasherInput::new(method).expect("ParPar hasher unavailable");
    hasher.update(data);
    *hasher.finalize().as_bytes()
}

#[cfg(all(
    feature = "parpar-compare",
    target_arch = "x86_64",
    parpar_compare_embedded
))]
fn parpar_crc32(data: &[u8]) -> u32 {
    par2rs::ffi::crc32::crc32_compute(data)
}

#[cfg(target_arch = "x86_64")]
#[library_benchmark]
#[bench::par2rs_scalar_16k(make_data(16 * 1024))]
fn bench_par2rs_scalar_16k(data: Vec<u8>) -> [u8; 16] {
    black_box(par2rs_md5_scalar(black_box(&data)))
}

#[cfg(target_arch = "x86_64")]
#[library_benchmark]
#[bench::par2rs_sse2_16k(make_data(16 * 1024))]
fn bench_par2rs_sse2_16k(data: Vec<u8>) -> [u8; 16] {
    black_box(par2rs_md5_sse2(black_box(&data)))
}

#[cfg(target_arch = "x86_64")]
#[library_benchmark]
#[bench::par2rs_bmi1_16k(make_data(16 * 1024))]
fn bench_par2rs_bmi1_16k(data: Vec<u8>) -> [u8; 16] {
    black_box(par2rs_md5_bmi1(black_box(&data)))
}

#[cfg(all(
    feature = "parpar-compare",
    target_arch = "x86_64",
    parpar_compare_embedded
))]
#[library_benchmark]
#[bench::parpar_scalar_16k(make_data(16 * 1024))]
fn bench_parpar_scalar_16k(data: Vec<u8>) -> [u8; 16] {
    black_box(parpar_hasher_input(
        black_box(&data),
        par2rs::ffi::HasherInputMethod::Scalar,
    ))
}

#[cfg(all(
    feature = "parpar-compare",
    target_arch = "x86_64",
    parpar_compare_embedded
))]
#[library_benchmark]
#[bench::parpar_sse_16k(make_data(16 * 1024))]
fn bench_parpar_sse_16k(data: Vec<u8>) -> [u8; 16] {
    black_box(parpar_hasher_input(
        black_box(&data),
        par2rs::ffi::HasherInputMethod::Simd,
    ))
}

#[cfg(all(
    feature = "parpar-compare",
    target_arch = "x86_64",
    parpar_compare_embedded
))]
#[library_benchmark]
#[bench::parpar_bmi1_16k(make_data(16 * 1024))]
fn bench_parpar_bmi1_16k(data: Vec<u8>) -> [u8; 16] {
    black_box(parpar_hasher_input(
        black_box(&data),
        par2rs::ffi::HasherInputMethod::Bmi1,
    ))
}

#[library_benchmark]
#[bench::crc32_1m(make_data(1024 * 1024))]
fn bench_crc32_1m(data: Vec<u8>) -> u32 {
    black_box(par2rs_crc32(black_box(&data)))
}

#[cfg(all(
    feature = "parpar-compare",
    target_arch = "x86_64",
    parpar_compare_embedded
))]
#[library_benchmark]
#[bench::parpar_crc32_1m(make_data(1024 * 1024))]
fn bench_parpar_crc32_1m(data: Vec<u8>) -> u32 {
    black_box(parpar_crc32(black_box(&data)))
}

#[cfg(target_arch = "x86_64")]
library_benchmark_group!(
    name = md5_group;
    benchmarks =
        bench_par2rs_scalar_16k,
        bench_par2rs_sse2_16k,
        bench_par2rs_bmi1_16k
);

#[cfg(all(
    feature = "parpar-compare",
    target_arch = "x86_64",
    parpar_compare_embedded
))]
library_benchmark_group!(
    name = md5_parpar_group;
    benchmarks =
        bench_parpar_scalar_16k,
        bench_parpar_sse_16k,
        bench_parpar_bmi1_16k
);

library_benchmark_group!(
    name = crc32_group;
    benchmarks =
        bench_crc32_1m
);

#[cfg(all(
    feature = "parpar-compare",
    target_arch = "x86_64",
    parpar_compare_embedded
))]
library_benchmark_group!(
    name = crc32_parpar_group;
    benchmarks =
        bench_parpar_crc32_1m
);

#[cfg(all(
    feature = "parpar-compare",
    target_arch = "x86_64",
    parpar_compare_embedded
))]
main!(
    library_benchmark_groups = md5_group,
    md5_parpar_group,
    crc32_group,
    crc32_parpar_group
);

#[cfg(any(
    all(not(feature = "parpar-compare"), target_arch = "x86_64"),
    all(
        feature = "parpar-compare",
        target_arch = "x86_64",
        not(parpar_compare_embedded)
    )
))]
main!(library_benchmark_groups = md5_group, crc32_group);

#[cfg(not(target_arch = "x86_64"))]
main!(library_benchmark_groups = crc32_group);
