// src/resilience/tests/exponential_backoff_tests.rs

use std::time::Duration;
use crate::resilience::{ExponentialBackoff, RetryConfig};

#[test]
fn test_backoff_increases_exponentially() {
    let config = RetryConfig {
        max_attempts: 5,
        initial_backoff: Duration::from_millis(100),
        max_backoff: Duration::from_secs(10),
        backoff_multiplier: 2.0,
        use_jitter: false,  // Disable jitter for deterministic testing
    };
    
    let mut backoff = ExponentialBackoff::new(config);
    
    // First backoff should be initial_backoff (100ms)
    let first = backoff.next_backoff().unwrap();
    assert_eq!(first, Duration::from_millis(100));
    
    // Second backoff should be initial_backoff * multiplier = 100ms * 2 = 200ms
    let second = backoff.next_backoff().unwrap();
    assert_eq!(second, Duration::from_millis(200));
    
    // Third backoff should be previous * multiplier = 200ms * 2 = 400ms
    let third = backoff.next_backoff().unwrap();
    assert_eq!(third, Duration::from_millis(400));
    
    // Fourth backoff = 400ms * 2 = 800ms
    let fourth = backoff.next_backoff().unwrap();
    assert_eq!(fourth, Duration::from_millis(800));
    
    // Fifth backoff = 800ms * 2 = 1600ms
    let fifth = backoff.next_backoff().unwrap();
    assert_eq!(fifth, Duration::from_millis(1600));
    
    // Sixth attempt should return None (exceeds max_attempts)
    assert_eq!(backoff.next_backoff(), None);
}

#[test]
fn test_backoff_respects_max_backoff() {
    let config = RetryConfig {
        max_attempts: 5,
        initial_backoff: Duration::from_millis(100),
        max_backoff: Duration::from_millis(300),  // Low max to test capping
        backoff_multiplier: 2.0,
        use_jitter: false,
    };
    
    let mut backoff = ExponentialBackoff::new(config);
    
    // First backoff = 100ms
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(100));
    
    // Second backoff = 200ms
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(200));
    
    // Third backoff would be 400ms but should be capped at 300ms
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(300));
    
    // Fourth backoff still capped at 300ms
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(300));
    
    // Fifth backoff still capped at 300ms
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(300));
}

#[test]
fn test_jitter_adds_randomness() {
    let config = RetryConfig {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(100),
        max_backoff: Duration::from_secs(10),
        backoff_multiplier: 2.0,
        use_jitter: true,  // Enable jitter
    };
    
    let mut backoff = ExponentialBackoff::new(config);
    
    // With jitter enabled, we can only verify values are within expected ranges
    
    // First backoff should be between 50ms (50% of 100ms) and 100ms
    let first = backoff.next_backoff().unwrap();
    assert!(first >= Duration::from_millis(50) && first <= Duration::from_millis(100),
           "First backoff with jitter should be between 50ms and 100ms");
    
    // Second backoff should be between 100ms (50% of 200ms) and 200ms
    let second = backoff.next_backoff().unwrap();
    assert!(second >= Duration::from_millis(100) && second <= Duration::from_millis(200),
           "Second backoff with jitter should be between 100ms and 200ms");
    
    // Third backoff should be between 200ms (50% of 400ms) and 400ms
    let third = backoff.next_backoff().unwrap();
    assert!(third >= Duration::from_millis(200) && third <= Duration::from_millis(400),
           "Third backoff with jitter should be between 200ms and 400ms");
}

#[test]
fn test_reset_restarts_backoff_sequence() {
    let config = RetryConfig {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(100),
        max_backoff: Duration::from_secs(10),
        backoff_multiplier: 2.0,
        use_jitter: false,
    };
    
    let mut backoff = ExponentialBackoff::new(config);
    
    // First cycle
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(100));
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(200));
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(400));
    assert_eq!(backoff.next_backoff(), None);  // Exceeded max attempts
    
    // Reset
    backoff.reset();
    
    // Second cycle should start from the beginning
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(100));
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(200));
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(400));
    assert_eq!(backoff.next_backoff(), None);
}

#[test]
fn test_custom_backoff_configurations() {
    // Test with high initial backoff
    let config = RetryConfig {
        max_attempts: 2,
        initial_backoff: Duration::from_secs(1),  // 1 second
        max_backoff: Duration::from_secs(10),
        backoff_multiplier: 2.0,
        use_jitter: false,
    };
    
    let mut backoff = ExponentialBackoff::new(config);
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_secs(1));
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_secs(2));
    
    // Test with high multiplier
    let config = RetryConfig {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(100),
        max_backoff: Duration::from_secs(10),
        backoff_multiplier: 5.0,  // 5x multiplier
        use_jitter: false,
    };
    
    let mut backoff = ExponentialBackoff::new(config);
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(100));
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(500));  // 100 * 5
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(2500)); // 500 * 5
    
    // Test with fractional multiplier
    let config = RetryConfig {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(1000),
        max_backoff: Duration::from_secs(10),
        backoff_multiplier: 0.5,  // 0.5x multiplier (decreasing)
        use_jitter: false,
    };
    
    let mut backoff = ExponentialBackoff::new(config);
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(1000));
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(500));  // 1000 * 0.5
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(250));  // 500 * 0.5
}

#[test]
fn test_edge_cases() {
    // Test with zero max_attempts
    let config = RetryConfig {
        max_attempts: 0,
        initial_backoff: Duration::from_millis(100),
        max_backoff: Duration::from_secs(10),
        backoff_multiplier: 2.0,
        use_jitter: false,
    };
    
    let mut backoff = ExponentialBackoff::new(config);
    assert_eq!(backoff.next_backoff(), None, "Zero max_attempts should immediately return None");
    
    // Test with zero initial_backoff
    let config = RetryConfig {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(0),
        max_backoff: Duration::from_secs(10),
        backoff_multiplier: 2.0,
        use_jitter: false,
    };
    
    let mut backoff = ExponentialBackoff::new(config);
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(0));
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(0));
    
    // Test with multiplier = 1.0 (constant backoff)
    let config = RetryConfig {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(100),
        max_backoff: Duration::from_secs(10),
        backoff_multiplier: 1.0,
        use_jitter: false,
    };
    
    let mut backoff = ExponentialBackoff::new(config);
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(100));
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(100));
    assert_eq!(backoff.next_backoff().unwrap(), Duration::from_millis(100));
}