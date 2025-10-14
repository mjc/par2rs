use criterion::{black_box, criterion_group, criterion_main, Criterion};
use par2rs::repair::repair_files_quiet;
use std::fs;

/// Setup function to ensure test files are in the correct corrupted state
fn setup_corrupted_testfile() {
    let testfile = "tests/fixtures/testfile";
    let backup = "tests/fixtures/testfile_corrupted";
    
    // Copy corrupted version over the testfile
    if std::path::Path::new(backup).exists() {
        fs::copy(backup, testfile).expect("Failed to setup corrupted testfile");
    }
}

/// Benchmark the complete repair workflow
/// 
/// This is the main benchmark - it tests the entire repair process end-to-end
/// including file I/O, Reed-Solomon reconstruction, and verification.
fn bench_repair_workflow(c: &mut Criterion) {
    let par2_file = "tests/fixtures/testfile.par2";
    
    let mut group = c.benchmark_group("repair");
    // Use smaller sample size since each iteration is expensive
    group.sample_size(20);
    // Set longer measurement time for more stable results  
    group.measurement_time(std::time::Duration::from_secs(10));
    
    group.bench_function("complete_workflow", |b| {
        b.iter(|| {
            // Setup corrupted file before each iteration
            setup_corrupted_testfile();
            
            // Run the repair (quiet mode to avoid print overhead in benchmark)
            let result = repair_files_quiet(black_box(par2_file), black_box(&[]));
            assert!(result.is_ok());
        });
    });
    
    group.finish();
}

criterion_group!(benches, bench_repair_workflow);
criterion_main!(benches);
