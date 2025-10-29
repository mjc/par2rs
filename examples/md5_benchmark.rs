use par2rs::checksum::calculate_file_md5;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

fn main() {
    // Create a test file of various sizes
    let sizes = vec![
        (1 * 1024 * 1024, "1MB"),
        (10 * 1024 * 1024, "10MB"),
        (100 * 1024 * 1024, "100MB"),
        (500 * 1024 * 1024, "500MB"),
    ];

    for (size, label) in sizes {
        // Create test file
        let test_file = format!("/tmp/par2rs_perf_test_{}.bin", label);
        {
            let mut file = File::create(&test_file).unwrap();
            let chunk = vec![0x42u8; 1024 * 1024]; // 1MB chunks
            for _ in 0..(size / (1024 * 1024)) {
                file.write_all(&chunk).unwrap();
            }
            file.sync_all().unwrap();
        }

        // Benchmark MD5 calculation
        let start = Instant::now();
        let _hash = calculate_file_md5(std::path::Path::new(&test_file)).unwrap();
        let duration = start.elapsed();

        let throughput = (size as f64) / duration.as_secs_f64() / (1024.0 * 1024.0);

        println!(
            "{}: {:.2} MB/s ({:.3}s for {} bytes)",
            label,
            throughput,
            duration.as_secs_f64(),
            size
        );

        // Clean up
        std::fs::remove_file(&test_file).ok();
    }
}
