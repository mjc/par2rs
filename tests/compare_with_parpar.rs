#![cfg(all(
    feature = "parpar-compare",
    target_arch = "x86_64",
    parpar_compare_embedded
))]

use par2rs::ffi::{crc32, hasher_input::ParParHasherInput, HasherInputMethod};
use par2rs::parpar_hasher::hasher_input::HasherInput;
use par2rs::parpar_hasher::md5x2_avx512::Avx512;
use par2rs::parpar_hasher::md5x2_bmi1::Bmi1;
use par2rs::parpar_hasher::md5x2_scalar::Scalar;
use par2rs::parpar_hasher::md5x2_sse2::Sse2;

fn make_data(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(7))
        .collect()
}

fn par2rs_hash_scalar(data: &[u8]) -> [u8; 16] {
    let mut hasher = HasherInput::<Scalar>::new();
    hasher.update(data);
    hasher.end()
}

fn par2rs_hash_sse2(data: &[u8]) -> [u8; 16] {
    let mut hasher = HasherInput::<Sse2>::new();
    hasher.update(data);
    hasher.end()
}

fn par2rs_hash_bmi1(data: &[u8]) -> [u8; 16] {
    let mut hasher = HasherInput::<Bmi1>::new();
    hasher.update(data);
    hasher.end()
}

fn par2rs_hash_avx512(data: &[u8]) -> Option<[u8; 16]> {
    if !is_x86_feature_detected!("avx512f")
        || !is_x86_feature_detected!("avx512vl")
        || !is_x86_feature_detected!("avx512bw")
    {
        return None;
    }

    let mut hasher = HasherInput::<Avx512>::new();
    hasher.update(data);
    Some(hasher.end())
}

fn par2rs_crc32(data: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

fn parpar_hash(method: HasherInputMethod, data: &[u8]) -> [u8; 16] {
    assert!(
        method.is_available(),
        "ParPar method {:?} is unavailable",
        method
    );
    let mut hasher = ParParHasherInput::new(method).expect("ParPar hasher unavailable");
    hasher.update(data);
    *hasher.finalize().as_bytes()
}

#[test]
fn parpar_matches_par2rs_on_fixed_buffers() {
    for size in [0usize, 1, 63, 64, 65, 1024, 16 * 1024, 4 * 1024 * 1024] {
        let data = make_data(size);

        assert_eq!(
            par2rs_hash_scalar(&data),
            parpar_hash(HasherInputMethod::Scalar, &data)
        );
        assert_eq!(
            par2rs_hash_sse2(&data),
            parpar_hash(HasherInputMethod::Simd, &data)
        );
        assert_eq!(
            par2rs_hash_scalar(&data),
            parpar_hash(HasherInputMethod::Crc, &data)
        );
        assert_eq!(
            par2rs_hash_sse2(&data),
            parpar_hash(HasherInputMethod::SimdCrc, &data)
        );
        assert_eq!(
            par2rs_hash_bmi1(&data),
            parpar_hash(HasherInputMethod::Bmi1, &data)
        );
        if let Some(rust) = par2rs_hash_avx512(&data) {
            assert_eq!(rust, parpar_hash(HasherInputMethod::Avx512, &data));
        }
        assert_eq!(par2rs_crc32(&data), crc32::crc32_compute(&data));
    }
}

#[test]
fn parpar_chunked_updates_match_par2rs() {
    for size in [0usize, 1, 63, 64, 65, 1024, 16 * 1024] {
        let data = make_data(size);

        let mut par2rs = HasherInput::<Scalar>::new();
        let mut parpar =
            ParParHasherInput::new(HasherInputMethod::Crc).expect("ParPar hasher unavailable");

        for chunk in data.chunks(17) {
            par2rs.update(chunk);
            parpar.update(chunk);
        }

        assert_eq!(par2rs.end(), parpar.finalize());
        assert_eq!(par2rs_crc32(&data), crc32::crc32_compute(&data));
    }
}
