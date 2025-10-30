use par2rs::verify::VerificationConfig;

#[test]
fn test_thread_configuration_values() {
    // Test various thread configuration values
    let configs = [
        (0, true),  // Auto-detect threads, parallel
        (1, true),  // Single thread, parallel (effectively sequential)
        (2, true),  // Two threads
        (4, true),  // Four threads
        (8, true),  // Eight threads
        (0, false), // Auto-detect threads, sequential
        (4, false), // Four threads, sequential
    ];

    for (threads, parallel) in configs.iter() {
        let config = VerificationConfig::new(*threads, *parallel);
        assert_eq!(config.threads, *threads);
        assert_eq!(config.parallel, *parallel);

        // Basic validation
        if !config.parallel {
            // In sequential mode, effective threads should be 1
            assert_eq!(config.effective_threads(), 1);
        } else if config.threads == 1 {
            // Single thread parallel is effectively sequential
            assert_eq!(config.effective_threads(), 1);
        }
    }
}

#[test]
fn test_thread_pool_configuration() {
    // Test that thread pool configuration works (integration test)
    // This test verifies the thread pool can be configured without errors

    let test_configs = [
        VerificationConfig::new(1, true),
        VerificationConfig::new(2, true),
        VerificationConfig::new(4, true),
    ];

    for config in test_configs.iter() {
        // Test that configuring rayon thread pool doesn't panic
        let effective_threads = config.effective_threads();
        if effective_threads > 0 {
            let result = rayon::ThreadPoolBuilder::new()
                .num_threads(effective_threads)
                .build_global();

            // It's ok if this fails (thread pool already configured)
            // We just want to ensure it doesn't panic
            match result {
                Ok(_) => println!("Successfully configured {} threads", effective_threads),
                Err(_) => println!("Thread pool already configured (expected in tests)"),
            }
        }
    }
}

#[test]
fn test_parallel_vs_sequential_flag() {
    // Test that parallel flag correctly affects behavior
    let parallel_config = VerificationConfig::new(4, true);
    let sequential_config = VerificationConfig::new(4, false);

    assert!(parallel_config.parallel);
    assert!(!sequential_config.parallel);

    // Both should have same thread counts but different parallel flag
    assert_eq!(parallel_config.threads, sequential_config.threads);
    assert_ne!(parallel_config.parallel, sequential_config.parallel);
    
    // Effective threads should differ
    assert_eq!(parallel_config.effective_threads(), 4);
    assert_eq!(sequential_config.effective_threads(), 1);
}

#[test]
fn test_config_edge_cases() {
    // Test edge cases for configuration

    // Zero threads with parallel (should auto-detect)
    let config = VerificationConfig::new(0, true);
    assert_eq!(config.threads, 0);
    assert!(config.parallel);
    assert!(config.effective_threads() > 0);

    // Large thread counts
    let config = VerificationConfig::new(64, true);
    assert_eq!(config.threads, 64);
    assert!(config.parallel);
    assert_eq!(config.effective_threads(), 64);

    // Sequential mode forces single thread regardless of configuration
    let config = VerificationConfig::new(100, false);
    assert_eq!(config.threads, 100);
    assert!(!config.parallel);
    assert_eq!(config.effective_threads(), 1);
}
