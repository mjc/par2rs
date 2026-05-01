use iai_callgrind::{binary_benchmark, binary_benchmark_group, main, Command};

#[path = "common/par2verify_fixture.rs"]
mod par2verify_fixture;

fn into_iai_command(invocation: par2verify_fixture::Invocation) -> Command {
    let mut command = Command::new(invocation.program);
    command.current_dir(invocation.current_dir);
    for arg in invocation.args {
        command.arg(arg);
    }
    command.build()
}

#[binary_benchmark]
#[bench::intact_8m(par2verify_fixture::iai_fixture_arg())]
fn bench_par2rs_verify(par2_file: String) -> Command {
    into_iai_command(par2verify_fixture::par2rs_verify_invocation_for_path(
        par2_file,
    ))
}

#[binary_benchmark]
#[bench::intact_8m(par2verify_fixture::iai_fixture_arg())]
fn bench_turbo_verify(par2_file: String) -> Command {
    into_iai_command(par2verify_fixture::turbo_verify_invocation_for_path(
        par2_file,
    ))
}

binary_benchmark_group!(
    name = par2verify_group;
    benchmarks = bench_par2rs_verify, bench_turbo_verify
);

main!(binary_benchmark_groups = par2verify_group);
