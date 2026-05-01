use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::process::Command;
use std::time::Duration;

#[path = "common/par2verify_fixture.rs"]
mod par2verify_fixture;

fn run_invocation(invocation: &par2verify_fixture::Invocation) {
    let output = Command::new(&invocation.program)
        .current_dir(&invocation.current_dir)
        .args(&invocation.args)
        .output()
        .unwrap_or_else(|err| {
            panic!(
                "failed to run {}: {err}",
                invocation.program.to_string_lossy()
            )
        });

    assert!(
        output.status.success(),
        "verify command failed: program={} status={:?} stderr={}",
        invocation.program.display(),
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn bench_par2verify_compare(c: &mut Criterion) {
    let fixture = par2verify_fixture::criterion_fixture();
    let mut group = c.benchmark_group("par2verify_compare");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(20));
    group.throughput(Throughput::Bytes(fixture.protected_bytes));

    let par2rs = par2verify_fixture::par2rs_verify_invocation(fixture);
    group.bench_function(BenchmarkId::new("par2rs", fixture.case_name), |b| {
        b.iter(|| run_invocation(&par2rs))
    });

    let turbo = par2verify_fixture::turbo_verify_invocation(fixture);
    group.bench_function(BenchmarkId::new("turbo", fixture.case_name), |b| {
        b.iter(|| run_invocation(&turbo))
    });

    group.finish();
}

criterion_group!(benches, bench_par2verify_compare);
criterion_main!(benches);
