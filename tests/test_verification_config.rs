use par2rs::verify::VerificationConfig;

#[test]
fn test_verification_config_default() {
    let config = VerificationConfig::default();
    assert_eq!(config.threads, 0); // 0 means use default thread count
    assert!(config.parallel); // Default should be parallel mode
}

#[test]
fn test_verification_config_creation() {
    let config = VerificationConfig::new(4, true);
    assert_eq!(config.threads, 4);
    assert!(config.parallel);

    let config = VerificationConfig::new(0, false);
    assert_eq!(config.threads, 0);
    assert!(!config.parallel);

    let config = VerificationConfig::new(8, false);
    assert_eq!(config.threads, 8);
    assert!(!config.parallel);
}

#[test]
fn test_verification_config_thread_bounds() {
    // Test edge cases for thread counts
    let config = VerificationConfig {
        threads: 1,
        parallel: true,
    };
    assert_eq!(config.threads, 1);

    let config = VerificationConfig {
        threads: 32,
        parallel: true,
    };
    assert_eq!(config.threads, 32);
}

#[test]
fn test_verification_config_parallel_combinations() {
    // Test all combinations of parallel and thread settings
    let combinations = [
        (0, true),  // Default threads, parallel
        (0, false), // Default threads, sequential
        (1, true),  // Single thread, parallel (effectively sequential)
        (1, false), // Single thread, sequential
        (4, true),  // Multi-thread, parallel
        (4, false), // Multi-thread, sequential (threads ignored)
    ];

    for (threads, parallel) in combinations.iter() {
        let config = VerificationConfig {
            threads: *threads,
            parallel: *parallel,
        };
        assert_eq!(config.threads, *threads);
        assert_eq!(config.parallel, *parallel);
    }
}
