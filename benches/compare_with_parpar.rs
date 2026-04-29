//! Wall-clock comparison between par2rs and embedded ParPar hasher sources.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
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

#[cfg(target_arch = "x86_64")]
fn par2rs_md5_avx512(data: &[u8]) -> Option<[u8; 16]> {
    if !is_x86_feature_detected!("avx512f")
        || !is_x86_feature_detected!("avx512vl")
        || !is_x86_feature_detected!("avx512bw")
    {
        return None;
    }

    use par2rs::parpar_hasher::hasher_input::HasherInput;
    use par2rs::parpar_hasher::md5x2_avx512::Avx512;

    let mut hasher = HasherInput::<Avx512>::new();
    hasher.update(data);
    Some(hasher.end())
}

fn par2rs_crc32_fast(data: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

#[cfg(target_arch = "x86_64")]
fn avx512_runtime_available() -> bool {
    is_x86_feature_detected!("avx512f")
        && is_x86_feature_detected!("avx512vl")
        && is_x86_feature_detected!("avx512bw")
}

#[cfg(all(
    feature = "parpar-compare",
    target_arch = "x86_64",
    parpar_compare_embedded
))]
fn parpar_hasher_input(data: &[u8], method: par2rs::ffi::HasherInputMethod) -> [u8; 16] {
    use par2rs::ffi::hasher_input::ParParHasherInput;

    if !method.is_available() {
        panic!("ParPar method {:?} is unavailable", method);
    }
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
fn bench_md5_variants(c: &mut Criterion) {
    let sizes = [(16 * 1024, "16k"), (4 * 1024 * 1024, "4m")];

    for (size, label) in &sizes {
        let data = black_box(make_data(*size));
        let mut group = c.benchmark_group(format!("md5_variants_{label}"));
        group.throughput(Throughput::Bytes(*size as u64));

        let rust_variants: &[(&str, fn(&[u8]) -> [u8; 16])] = &[
            ("par2rs_scalar", par2rs_md5_scalar),
            ("par2rs_sse2", par2rs_md5_sse2),
            ("par2rs_bmi1", par2rs_md5_bmi1),
        ];
        for (name, func) in rust_variants {
            group.bench_with_input(BenchmarkId::new(*name, label), &data, |b, data| {
                b.iter(|| func(black_box(data)))
            });
        }
        if avx512_runtime_available() && par2rs_md5_avx512(&data).is_some() {
            group.bench_with_input(
                BenchmarkId::new("par2rs_avx512", label),
                &data,
                |b, data| b.iter(|| par2rs_md5_avx512(black_box(data)).unwrap()),
            );
        }

        #[cfg(all(
            feature = "parpar-compare",
            target_arch = "x86_64",
            parpar_compare_embedded
        ))]
        {
            let parpar_variants: &[(&str, par2rs::ffi::HasherInputMethod)] = &[
                ("parpar_scalar", par2rs::ffi::HasherInputMethod::Scalar),
                ("parpar_simd", par2rs::ffi::HasherInputMethod::Simd),
                ("parpar_crc", par2rs::ffi::HasherInputMethod::Crc),
                ("parpar_simd_crc", par2rs::ffi::HasherInputMethod::SimdCrc),
                ("parpar_bmi1", par2rs::ffi::HasherInputMethod::Bmi1),
                ("parpar_avx512", par2rs::ffi::HasherInputMethod::Avx512),
            ];

            for (name, method) in parpar_variants {
                if method.is_available()
                    && (method != &par2rs::ffi::HasherInputMethod::Avx512
                        || avx512_runtime_available())
                {
                    group.bench_with_input(BenchmarkId::new(*name, label), &data, |b, data| {
                        b.iter(|| parpar_hasher_input(black_box(data), *method))
                    });
                }
            }
        }

        group.finish();
    }
}

fn bench_crc32_variants(c: &mut Criterion) {
    let data = black_box(make_data(4 * 1024 * 1024));
    let mut group = c.benchmark_group("crc32_4m");
    group.throughput(Throughput::Bytes(data.len() as u64));

    group.bench_function("par2rs_crc32fast", |b| {
        b.iter(|| par2rs_crc32_fast(black_box(&data)))
    });

    #[cfg(all(
        feature = "parpar-compare",
        target_arch = "x86_64",
        parpar_compare_embedded
    ))]
    group.bench_function("parpar_crc32", |b| {
        b.iter(|| parpar_crc32(black_box(&data)))
    });

    group.finish();
}

#[cfg(target_arch = "x86_64")]
criterion_group!(benches, bench_md5_variants, bench_crc32_variants);

#[cfg(not(target_arch = "x86_64"))]
criterion_group!(benches, bench_crc32_variants);

criterion_main!(benches);
