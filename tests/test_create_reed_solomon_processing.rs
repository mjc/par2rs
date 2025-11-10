//! Comprehensive tests for Reed-Solomon processing in PAR2 creation
//!
//! These tests verify that sequential and parallel Reed-Solomon processing
//! produce identical results across various scenarios.

use par2rs::create::{CreateContextBuilder, ConsoleCreateReporter};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

/// Helper to create a test file with specific content
fn create_test_file(path: &Path, size: usize, pattern: u8) -> std::io::Result<()> {
    let data = vec![pattern; size];
    fs::write(path, data)
}

/// Helper to create a test file with varying content (non-uniform)
fn create_varied_test_file(path: &Path, size: usize) -> std::io::Result<()> {
    let mut data = Vec::with_capacity(size);
    for i in 0..size {
        data.push((i % 256) as u8);
    }
    fs::write(path, data)
}

/// Helper to read all PAR2 files created in a directory
fn read_par2_files(dir: &Path, base_name: &str) -> std::io::Result<Vec<Vec<u8>>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with(base_name) && name.ends_with(".par2") {
                let content = fs::read(&path)?;
                files.push(content);
            }
        }
    }
    files.sort(); // Ensure consistent ordering
    Ok(files)
}

#[test]
fn test_single_block_sequential_vs_parallel() {
    let temp = tempdir().unwrap();
    
    // Create a small file that fits in one block (< 512 bytes minimum block size)
    let test_file = temp.path().join("test.dat");
    create_test_file(&test_file, 256, 0xAB).unwrap();

    // Create PAR2 with sequential processing (thread_count = 1)
    let seq_dir = temp.path().join("sequential");
    fs::create_dir(&seq_dir).unwrap();
    let seq_test_file = seq_dir.join("test.dat");
    fs::copy(&test_file, &seq_test_file).unwrap();
    
    let reporter = Box::new(ConsoleCreateReporter::new(true));
    let mut seq_context = CreateContextBuilder::new()
        .output_name(seq_dir.join("test.par2").to_str().unwrap())
        .source_files(vec![seq_test_file])
        .redundancy_percentage(10)
        .thread_count(1) // Force sequential
        .reporter(reporter)
        .build()
        .unwrap();
    
    seq_context.create().unwrap();

    // Create PAR2 with parallel processing (thread_count > 1)
    let par_dir = temp.path().join("parallel");
    fs::create_dir(&par_dir).unwrap();
    let par_test_file = par_dir.join("test.dat");
    fs::copy(&test_file, &par_test_file).unwrap();
    
    let reporter = Box::new(ConsoleCreateReporter::new(true));
    let mut par_context = CreateContextBuilder::new()
        .output_name(par_dir.join("test.par2").to_str().unwrap())
        .source_files(vec![par_test_file])
        .redundancy_percentage(10)
        .thread_count(4) // Force parallel
        .reporter(reporter)
        .build()
        .unwrap();
    
    par_context.create().unwrap();

    // Compare output files - they should be identical
    let seq_files = read_par2_files(&seq_dir, "test").unwrap();
    let par_files = read_par2_files(&par_dir, "test").unwrap();

    assert_eq!(seq_files.len(), par_files.len(), "Different number of output files");
    
    for (seq, par) in seq_files.iter().zip(par_files.iter()) {
        assert_eq!(
            seq.len(),
            par.len(),
            "File sizes differ between sequential and parallel"
        );
        assert_eq!(
            seq, par,
            "File content differs between sequential and parallel processing"
        );
    }
}

#[test]
fn test_multiple_blocks_sequential_vs_parallel() {
    let temp = tempdir().unwrap();
    
    // Create a file with multiple blocks (8KB, will be split into blocks)
    let test_file = temp.path().join("test.dat");
    create_test_file(&test_file, 8192, 0xCD).unwrap();

    // Sequential processing
    let seq_dir = temp.path().join("sequential");
    fs::create_dir(&seq_dir).unwrap();
    let seq_test_file = seq_dir.join("test.dat");
    fs::copy(&test_file, &seq_test_file).unwrap();
    
    let reporter = Box::new(ConsoleCreateReporter::new(true));
    let mut seq_context = CreateContextBuilder::new()
        .output_name(seq_dir.join("multi.par2").to_str().unwrap())
        .source_files(vec![seq_test_file])
        .block_size(2048) // Explicit 2KB blocks = 4 blocks total
        .redundancy_percentage(20)
        .thread_count(1)
        .reporter(reporter)
        .build()
        .unwrap();
    
    seq_context.create().unwrap();

    // Parallel processing
    let par_dir = temp.path().join("parallel");
    fs::create_dir(&par_dir).unwrap();
    let par_test_file = par_dir.join("test.dat");
    fs::copy(&test_file, &par_test_file).unwrap();
    
    let reporter = Box::new(ConsoleCreateReporter::new(true));
    let mut par_context = CreateContextBuilder::new()
        .output_name(par_dir.join("multi.par2").to_str().unwrap())
        .source_files(vec![par_test_file])
        .block_size(2048)
        .redundancy_percentage(20)
        .thread_count(8)
        .reporter(reporter)
        .build()
        .unwrap();
    
    par_context.create().unwrap();

    // Compare outputs
    let seq_files = read_par2_files(&seq_dir, "multi").unwrap();
    let par_files = read_par2_files(&par_dir, "multi").unwrap();

    assert_eq!(seq_files.len(), par_files.len());
    for (seq, par) in seq_files.iter().zip(par_files.iter()) {
        assert_eq!(seq, par, "Files differ between sequential and parallel");
    }
}

#[test]
fn test_partial_last_block_sequential_vs_parallel() {
    let temp = tempdir().unwrap();
    
    // Create a file that doesn't align to block boundaries
    // With 2048 byte blocks, 5000 bytes = 2 full blocks + 904 byte partial block
    let test_file = temp.path().join("test.dat");
    create_varied_test_file(&test_file, 5000).unwrap();

    // Sequential
    let seq_dir = temp.path().join("sequential");
    fs::create_dir(&seq_dir).unwrap();
    let seq_test_file = seq_dir.join("test.dat");
    fs::copy(&test_file, &seq_test_file).unwrap();
    
    let reporter = Box::new(ConsoleCreateReporter::new(true));
    let mut seq_context = CreateContextBuilder::new()
        .output_name(seq_dir.join("partial.par2").to_str().unwrap())
        .source_files(vec![seq_test_file])
        .block_size(2048)
        .recovery_block_count(2)
        .thread_count(1)
        .reporter(reporter)
        .build()
        .unwrap();
    
    seq_context.create().unwrap();

    // Parallel
    let par_dir = temp.path().join("parallel");
    fs::create_dir(&par_dir).unwrap();
    let par_test_file = par_dir.join("test.dat");
    fs::copy(&test_file, &par_test_file).unwrap();
    
    let reporter = Box::new(ConsoleCreateReporter::new(true));
    let mut par_context = CreateContextBuilder::new()
        .output_name(par_dir.join("partial.par2").to_str().unwrap())
        .source_files(vec![par_test_file])
        .block_size(2048)
        .recovery_block_count(2)
        .thread_count(6)
        .reporter(reporter)
        .build()
        .unwrap();
    
    par_context.create().unwrap();

    // Compare
    let seq_files = read_par2_files(&seq_dir, "partial").unwrap();
    let par_files = read_par2_files(&par_dir, "partial").unwrap();

    assert_eq!(seq_files.len(), par_files.len());
    for (seq, par) in seq_files.iter().zip(par_files.iter()) {
        assert_eq!(seq, par, "Partial block handling differs");
    }
}

#[test]
fn test_multifile_sequential_vs_parallel() {
    let temp = tempdir().unwrap();
    
    // Create multiple source files
    let file1 = temp.path().join("file1.dat");
    let file2 = temp.path().join("file2.dat");
    let file3 = temp.path().join("file3.dat");
    
    create_test_file(&file1, 3000, 0x11).unwrap();
    create_varied_test_file(&file2, 5500).unwrap();
    create_test_file(&file3, 2000, 0x33).unwrap();

    // Sequential
    let seq_dir = temp.path().join("sequential");
    fs::create_dir(&seq_dir).unwrap();
    let seq_file1 = seq_dir.join("file1.dat");
    let seq_file2 = seq_dir.join("file2.dat");
    let seq_file3 = seq_dir.join("file3.dat");
    fs::copy(&file1, &seq_file1).unwrap();
    fs::copy(&file2, &seq_file2).unwrap();
    fs::copy(&file3, &seq_file3).unwrap();
    
    let reporter = Box::new(ConsoleCreateReporter::new(true));
    let mut seq_context = CreateContextBuilder::new()
        .output_name(seq_dir.join("multi.par2").to_str().unwrap())
        .source_files(vec![seq_file1, seq_file2, seq_file3])
        .redundancy_percentage(15)
        .thread_count(1)
        .reporter(reporter)
        .build()
        .unwrap();
    
    seq_context.create().unwrap();

    // Parallel
    let par_dir = temp.path().join("parallel");
    fs::create_dir(&par_dir).unwrap();
    let par_file1 = par_dir.join("file1.dat");
    let par_file2 = par_dir.join("file2.dat");
    let par_file3 = par_dir.join("file3.dat");
    fs::copy(&file1, &par_file1).unwrap();
    fs::copy(&file2, &par_file2).unwrap();
    fs::copy(&file3, &par_file3).unwrap();
    
    let reporter = Box::new(ConsoleCreateReporter::new(true));
    let mut par_context = CreateContextBuilder::new()
        .output_name(par_dir.join("multi.par2").to_str().unwrap())
        .source_files(vec![par_file1, par_file2, par_file3])
        .redundancy_percentage(15)
        .thread_count(4)
        .reporter(reporter)
        .build()
        .unwrap();
    
    par_context.create().unwrap();

    // Compare
    let seq_files = read_par2_files(&seq_dir, "multi").unwrap();
    let par_files = read_par2_files(&par_dir, "multi").unwrap();

    assert_eq!(seq_files.len(), par_files.len());
    for (seq, par) in seq_files.iter().zip(par_files.iter()) {
        assert_eq!(seq, par, "Multifile processing differs");
    }
}

#[test]
fn test_high_redundancy_sequential_vs_parallel() {
    let temp = tempdir().unwrap();
    
    // Test with high redundancy (50%) to stress Reed-Solomon more
    let test_file = temp.path().join("test.dat");
    create_varied_test_file(&test_file, 10240).unwrap();

    // Sequential
    let seq_dir = temp.path().join("sequential");
    fs::create_dir(&seq_dir).unwrap();
    let seq_test_file = seq_dir.join("test.dat");
    fs::copy(&test_file, &seq_test_file).unwrap();
    
    let reporter = Box::new(ConsoleCreateReporter::new(true));
    let mut seq_context = CreateContextBuilder::new()
        .output_name(seq_dir.join("high.par2").to_str().unwrap())
        .source_files(vec![seq_test_file])
        .redundancy_percentage(50)
        .thread_count(1)
        .reporter(reporter)
        .build()
        .unwrap();
    
    seq_context.create().unwrap();

    // Parallel
    let par_dir = temp.path().join("parallel");
    fs::create_dir(&par_dir).unwrap();
    let par_test_file = par_dir.join("test.dat");
    fs::copy(&test_file, &par_test_file).unwrap();
    
    let reporter = Box::new(ConsoleCreateReporter::new(true));
    let mut par_context = CreateContextBuilder::new()
        .output_name(par_dir.join("high.par2").to_str().unwrap())
        .source_files(vec![par_test_file])
        .redundancy_percentage(50)
        .thread_count(8)
        .reporter(reporter)
        .build()
        .unwrap();
    
    par_context.create().unwrap();

    // Compare
    let seq_files = read_par2_files(&seq_dir, "high").unwrap();
    let par_files = read_par2_files(&par_dir, "high").unwrap();

    assert_eq!(seq_files.len(), par_files.len());
    for (seq, par) in seq_files.iter().zip(par_files.iter()) {
        assert_eq!(seq, par, "High redundancy processing differs");
    }
}

#[test]
fn test_large_file_sequential_vs_parallel() {
    let temp = tempdir().unwrap();
    
    // Test with a larger file (1MB) to ensure chunking works correctly
    let test_file = temp.path().join("large.dat");
    create_varied_test_file(&test_file, 1024 * 1024).unwrap();

    // Sequential
    let seq_dir = temp.path().join("sequential");
    fs::create_dir(&seq_dir).unwrap();
    let seq_test_file = seq_dir.join("large.dat");
    fs::copy(&test_file, &seq_test_file).unwrap();
    
    let reporter = Box::new(ConsoleCreateReporter::new(true));
    let mut seq_context = CreateContextBuilder::new()
        .output_name(seq_dir.join("large.par2").to_str().unwrap())
        .source_files(vec![seq_test_file])
        .redundancy_percentage(10)
        .thread_count(1)
        .reporter(reporter)
        .build()
        .unwrap();
    
    seq_context.create().unwrap();

    // Parallel
    let par_dir = temp.path().join("parallel");
    fs::create_dir(&par_dir).unwrap();
    let par_test_file = par_dir.join("large.dat");
    fs::copy(&test_file, &par_test_file).unwrap();
    
    let reporter = Box::new(ConsoleCreateReporter::new(true));
    let mut par_context = CreateContextBuilder::new()
        .output_name(par_dir.join("large.par2").to_str().unwrap())
        .source_files(vec![par_test_file])
        .redundancy_percentage(10)
        .thread_count(4)
        .reporter(reporter)
        .build()
        .unwrap();
    
    par_context.create().unwrap();

    // Compare
    let seq_files = read_par2_files(&seq_dir, "large").unwrap();
    let par_files = read_par2_files(&par_dir, "large").unwrap();

    assert_eq!(seq_files.len(), par_files.len());
    for (seq, par) in seq_files.iter().zip(par_files.iter()) {
        assert_eq!(seq, par, "Large file processing differs");
    }
}

#[test]
fn test_varying_thread_counts_produce_same_output() {
    let temp = tempdir().unwrap();
    
    let test_file = temp.path().join("test.dat");
    create_varied_test_file(&test_file, 16384).unwrap();

    let mut all_outputs = Vec::new();

    // Test with different thread counts: 1, 2, 4, 8
    for thread_count in [1, 2, 4, 8] {
        let dir = temp.path().join(format!("threads_{}", thread_count));
        fs::create_dir(&dir).unwrap();
        let test_copy = dir.join("test.dat");
        fs::copy(&test_file, &test_copy).unwrap();
        
        let reporter = Box::new(ConsoleCreateReporter::new(true));
        let mut context = CreateContextBuilder::new()
            .output_name(dir.join("test.par2").to_str().unwrap())
            .source_files(vec![test_copy])
            .redundancy_percentage(15)
            .thread_count(thread_count)
            .reporter(reporter)
            .build()
            .unwrap();
        
        context.create().unwrap();
        
        let files = read_par2_files(&dir, "test").unwrap();
        all_outputs.push((thread_count, files));
    }

    // All outputs should be identical
    let (_, reference) = &all_outputs[0];
    for (thread_count, output) in &all_outputs[1..] {
        assert_eq!(
            reference.len(),
            output.len(),
            "Thread count {} produced different number of files",
            thread_count
        );
        for (ref_file, test_file) in reference.iter().zip(output.iter()) {
            assert_eq!(
                ref_file, test_file,
                "Thread count {} produced different output",
                thread_count
            );
        }
    }
}

#[test]
fn test_explicit_vs_calculated_block_size_consistency() {
    let temp = tempdir().unwrap();
    
    let test_file = temp.path().join("test.dat");
    create_test_file(&test_file, 8192, 0xEF).unwrap();

    // Create with calculated block size
    let calc_dir = temp.path().join("calculated");
    fs::create_dir(&calc_dir).unwrap();
    let calc_test = calc_dir.join("test.dat");
    fs::copy(&test_file, &calc_test).unwrap();
    
    let reporter = Box::new(ConsoleCreateReporter::new(true));
    let mut calc_context = CreateContextBuilder::new()
        .output_name(calc_dir.join("test.par2").to_str().unwrap())
        .source_files(vec![calc_test])
        .redundancy_percentage(10)
        .thread_count(1)
        .reporter(reporter)
        .build()
        .unwrap();
    
    let calculated_block_size = calc_context.block_size();
    calc_context.create().unwrap();

    // Create with explicit block size (same as calculated)
    let expl_dir = temp.path().join("explicit");
    fs::create_dir(&expl_dir).unwrap();
    let expl_test = expl_dir.join("test.dat");
    fs::copy(&test_file, &expl_test).unwrap();
    
    let reporter = Box::new(ConsoleCreateReporter::new(true));
    let mut expl_context = CreateContextBuilder::new()
        .output_name(expl_dir.join("test.par2").to_str().unwrap())
        .source_files(vec![expl_test])
        .block_size(calculated_block_size)
        .redundancy_percentage(10)
        .thread_count(4) // Different thread count to test both paths
        .reporter(reporter)
        .build()
        .unwrap();
    
    expl_context.create().unwrap();

    // Outputs should be identical
    let calc_files = read_par2_files(&calc_dir, "test").unwrap();
    let expl_files = read_par2_files(&expl_dir, "test").unwrap();

    assert_eq!(calc_files.len(), expl_files.len());
    for (calc, expl) in calc_files.iter().zip(expl_files.iter()) {
        assert_eq!(calc, expl);
    }
}
