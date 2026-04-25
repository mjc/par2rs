use iai_callgrind::{library_benchmark, library_benchmark_group, main};
use par2rs::reed_solomon::codec::build_split_mul_table;
use par2rs::reed_solomon::galois::Galois16;
use par2rs::reed_solomon::simd::process_slice_multiply_add_portable_simd;
#[cfg(target_arch = "x86_64")]
use par2rs::reed_solomon::simd::{
    prepare_xor_jit_bitplane_chunks, XorJitBitplaneKernel, XorJitPreparedCoeff,
};
#[cfg(target_arch = "x86_64")]
use par2rs::reed_solomon::simd::{process_slice_multiply_add_simd, SimdLevel};
use std::hint::black_box;

#[cfg(target_arch = "x86_64")]
const BITPLANE_BLOCK_BYTES: usize = 512;

#[cfg(target_arch = "x86_64")]
struct BitplaneFixture {
    inputs: Vec<Vec<u8>>,
    output: Vec<u8>,
    kernels: Vec<XorJitBitplaneKernel>,
}

#[cfg(target_arch = "x86_64")]
fn bitplane_fixture(batch_len: usize, segment_len: usize) -> BitplaneFixture {
    let mut inputs = Vec::with_capacity(batch_len);
    let mut kernels = Vec::with_capacity(batch_len);

    for input_idx in 0..batch_len {
        let source = (0..segment_len)
            .map(|byte_idx| (input_idx * 31 + byte_idx * 17 + byte_idx / 7) as u8)
            .collect::<Vec<_>>();
        let mut prepared = vec![0u8; segment_len.next_multiple_of(BITPLANE_BLOCK_BYTES)];
        prepare_xor_jit_bitplane_chunks(&mut prepared, &source);
        inputs.push(prepared);

        let coeff = XorJitPreparedCoeff::new((0x100b + input_idx * 37) as u16);
        kernels.push(XorJitBitplaneKernel::new(&coeff).expect("bitplane kernel"));
    }

    BitplaneFixture {
        inputs,
        output: vec![0u8; segment_len.next_multiple_of(BITPLANE_BLOCK_BYTES)],
        kernels,
    }
}

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

#[cfg(target_arch = "x86_64")]
#[library_benchmark]
#[bench::batch12_128k(bitplane_fixture(12, 128 * 1024))]
fn bench_xor_jit_bitplane_batch_first(mut fixture: BitplaneFixture) -> Vec<u8> {
    for (input, kernel) in fixture.inputs.iter().zip(fixture.kernels.iter()) {
        kernel.multiply_add_chunks(black_box(input), black_box(&mut fixture.output));
    }

    fixture.output
}

#[cfg(target_arch = "x86_64")]
#[library_benchmark]
#[bench::batch12_128k(bitplane_fixture(12, 128 * 1024))]
fn bench_xor_jit_bitplane_block_first(mut fixture: BitplaneFixture) -> Vec<u8> {
    for block_offset in (0..fixture.output.len()).step_by(BITPLANE_BLOCK_BYTES) {
        let output_block = &mut fixture.output[block_offset..block_offset + BITPLANE_BLOCK_BYTES];
        for (input, kernel) in fixture.inputs.iter().zip(fixture.kernels.iter()) {
            let input_block = &input[block_offset..block_offset + BITPLANE_BLOCK_BYTES];
            kernel.multiply_add_block(black_box(input_block), black_box(output_block));
        }
    }

    fixture.output
}

library_benchmark_group!(
    name = simd_group;
    benchmarks = bench_pshufb_simd, bench_portable_simd, bench_scalar_baseline
);

#[cfg(target_arch = "x86_64")]
library_benchmark_group!(
    name = xor_jit_bitplane_group;
    benchmarks = bench_xor_jit_bitplane_batch_first, bench_xor_jit_bitplane_block_first
);

#[cfg(target_arch = "x86_64")]
main!(
    library_benchmark_groups = simd_group,
    xor_jit_bitplane_group
);

#[cfg(not(target_arch = "x86_64"))]
main!(library_benchmark_groups = simd_group);
