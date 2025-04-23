// src/tests/rate_limiter_tests.rs

use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

use crate::algorithms::{FixedWindow, SlidingWindow, TokenBucket};
use crate::config::{FixedWindowConfig, SlidingWindowConfig, TokenBucketConfig};
use crate::test_utils::{create_test_rate_limiter, test_rate_limit_scenario, MockStorage};
use crate::RateLimiter;

// Test algorithm/storage coordination
#[tokio::test]
async fn test_algorithm_storage_coordination() {
    // Use fixed window algorithm for this test
    let algorithm_config = FixedWindowConfig {
        max_requests: 5,
        window: Duration::from_secs(60),
    };

    // Create algorithm using mock storage
    let storage = MockStorage::new();
    let algorithm = FixedWindow::new(storage.clone(), algorithm_config);

    // Create rate limiter
    let rate_limiter = create_test_rate_limiter(algorithm).await;

    // Test key
    let test_key = "coordination_test";

    // First check should be allowed and initialize storage
    let result = rate_limiter.check_and_record(test_key).await.unwrap();
    assert!(result.allowed, "First request should be allowed");
    assert_eq!(
        result.remaining, 4,
        "Should have 4 remaining after first request"
    );

    // Instead of directly checking storage data, we'll verify through behavior
    // Make more requests and verify counts
    for i in 1..5 {
        let result = rate_limiter.check_and_record(test_key).await.unwrap();
        assert!(result.allowed, "Request {} should be allowed", i);
        assert_eq!(
            result.remaining,
            5 - i - 1,
            "Should have {} remaining after request {}",
            5 - i - 1,
            i
        );
    }

    // 6th request should be denied
    let result = rate_limiter.check_and_record(test_key).await.unwrap();
    assert!(!result.allowed, "6th request should be denied");
    assert_eq!(
        result.remaining, 0,
        "Should have 0 remaining after limit reached"
    );

    // Reset should clear the limit
    rate_limiter.reset(test_key).await.unwrap();

    // After reset, request should be allowed again
    let result = rate_limiter.check_and_record(test_key).await.unwrap();
    assert!(result.allowed, "Request after reset should be allowed");
    assert_eq!(
        result.remaining, 4,
        "Should have 4 remaining after reset and one request"
    );
}

// Test separate rate limits for different keys
#[tokio::test]
async fn test_separate_rate_limits() {
    // Create fixed window algorithm with a small limit
    let algorithm_config = FixedWindowConfig {
        max_requests: 3,
        window: Duration::from_secs(60),
    };

    // Use the helper to create a test rate limiter
    let rate_limiter =
        create_test_rate_limiter(FixedWindow::new(MockStorage::new(), algorithm_config)).await;

    // Test with two different users
    let user1 = "rate_limit_user1";
    let user2 = "rate_limit_user2";

    // Use up all requests for user1
    for i in 0..3 {
        let result = rate_limiter.check_and_record(user1).await.unwrap();
        assert!(result.allowed, "Request {} for user1 should be allowed", i);
        assert_eq!(
            result.remaining,
            2 - i,
            "User1 should have {} requests remaining",
            2 - i
        );
    }

    // Additional request for user1 should be denied
    let result = rate_limiter.check_and_record(user1).await.unwrap();
    assert!(
        !result.allowed,
        "User1 should be rate limited after 3 requests"
    );
    assert_eq!(
        result.remaining, 0,
        "User1 should have 0 requests remaining"
    );

    // But user2 should still have their full quota
    for i in 0..3 {
        let result = rate_limiter.check_and_record(user2).await.unwrap();
        assert!(result.allowed, "Request {} for user2 should be allowed", i);
        assert_eq!(
            result.remaining,
            2 - i,
            "User2 should have {} requests remaining",
            2 - i
        );
    }

    // Additional request for user2 should be denied
    let result = rate_limiter.check_and_record(user2).await.unwrap();
    assert!(
        !result.allowed,
        "User2 should be rate limited after 3 requests"
    );
    assert_eq!(
        result.remaining, 0,
        "User2 should have 0 requests remaining"
    );
}

// Test different algorithms with the same pattern
#[tokio::test]
async fn test_different_algorithms() {
    // Define test parameters
    let request_count = 10;
    let rate_limit = 5;
    let expected_allowed = rate_limit; // Only first 5 requests should be allowed
    // Test each algorithm separately to avoid type issues with closures
    // 1. Test Fixed Window
    let fw_config = FixedWindowConfig {
        max_requests: rate_limit as u64,
        window: Duration::from_secs(60),
    };
    let fw_algorithm = FixedWindow::new(MockStorage::new(), fw_config);

    let fw_success = test_rate_limit_scenario(
        fw_algorithm,
        request_count,
        expected_allowed,
        None::<fn(usize) -> futures::future::BoxFuture<'static, ()>>,
    )
    .await;

    assert!(
        fw_success,
        "Fixed Window algorithm should allow exactly {} requests",
        expected_allowed
    );

    // 2. Test Sliding Window
    let sw_config = SlidingWindowConfig {
        max_requests: rate_limit as u64,
        window: Duration::from_secs(60),
        precision: 6,
    };
    let sw_algorithm = SlidingWindow::new(MockStorage::new(), sw_config);

    let sw_success = test_rate_limit_scenario(
        sw_algorithm,
        request_count,
        expected_allowed,
        None::<fn(usize) -> futures::future::BoxFuture<'static, ()>>,
    )
    .await;

    assert!(
        sw_success,
        "Sliding Window algorithm should allow exactly {} requests",
        expected_allowed
    );

    // 3. Test Token Bucket
    let tb_config = TokenBucketConfig {
        capacity: rate_limit as u64,
        refill_rate: 0.0, // No refill for this test
        initial_tokens: Some(rate_limit as u64),
    };
    let tb_algorithm = TokenBucket::new(MockStorage::new(), tb_config);

    let tb_success = test_rate_limit_scenario(
        tb_algorithm,
        request_count,
        expected_allowed,
        None::<fn(usize) -> futures::future::BoxFuture<'static, ()>>,
    )
    .await;

    assert!(
        tb_success,
        "Token Bucket algorithm should allow exactly {} requests",
        expected_allowed
    );
}
// Test logging integration
// Note: This test depends on how your codebase integrates with logging
#[tokio::test]
async fn test_logging_integration() {
    // For a basic test, we'll just make sure that RateLimiter operations
    // with logging don't crash. In a real test, we'd capture and verify logs.

    // Create a simple rate limiter
    let algorithm_config = FixedWindowConfig {
        max_requests: 2,
        window: Duration::from_secs(60),
    };

    let algorithm = FixedWindow::new(MockStorage::new(), algorithm_config);
    let rate_limiter = create_test_rate_limiter(algorithm).await;

    // Test key that will appear in logs
    let test_key = "logging_test_user";

    // First request (allowed)
    info!("About to check first request for {}", test_key);
    let result1 = rate_limiter.check_and_record(test_key).await.unwrap();
    assert!(result1.allowed, "First request should be allowed");

    // Second request (allowed)
    info!("About to check second request for {}", test_key);
    let result2 = rate_limiter.check_and_record(test_key).await.unwrap();
    assert!(result2.allowed, "Second request should be allowed");

    // Third request (denied)
    warn!(
        "About to check third request for {} (should be denied)",
        test_key
    );
    let result3 = rate_limiter.check_and_record(test_key).await.unwrap();
    assert!(!result3.allowed, "Third request should be denied");

    // Log results
    info!(
        "Rate limit test results: allowed1={}, allowed2={}, allowed3={}",
        result1.allowed, result2.allowed, result3.allowed
    );

    // The test passes if no panics occur and assertions are met
}

// Test timeouts
#[tokio::test]
async fn test_timeouts() {
    // Create storage
    let storage = MockStorage::new();

    // Create algorithm
    let algorithm_config = FixedWindowConfig {
        max_requests: 5,
        window: Duration::from_secs(60),
    };
    let algorithm = FixedWindow::new(storage.clone(), algorithm_config);

    // Create rate limiter with default config since new_with_config doesn't exist
    let rate_limiter = RateLimiter::new(algorithm, storage.clone(), "timeout_test".to_string());

    // Basic test with timing
    let start = std::time::Instant::now();
    let result = rate_limiter.check_and_record("timeout_test_user").await;
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "Operation should succeed");
    assert!(
        elapsed < Duration::from_secs(1),
        "Operation should complete well within timeout"
    );
}

// Test reset functionality
#[tokio::test]
async fn test_reset_functionality() {
    // Create fixed window algorithm with a small limit
    let algorithm_config = FixedWindowConfig {
        max_requests: 3,
        window: Duration::from_secs(60),
    };

    // Use the helper to create a test rate limiter
    let rate_limiter =
        create_test_rate_limiter(FixedWindow::new(MockStorage::new(), algorithm_config)).await;

    // Test key
    let test_key = "reset_test_user";

    // Use up all requests
    for i in 0..3 {
        let result = rate_limiter.check_and_record(test_key).await.unwrap();
        assert!(result.allowed, "Request {} should be allowed", i);
    }

    // Verify limit is reached
    let result = rate_limiter.check_and_record(test_key).await.unwrap();
    assert!(!result.allowed, "Should be rate limited after 3 requests");

    // Reset the rate limiter
    rate_limiter.reset(test_key).await.unwrap();

    // Should be able to make requests again
    let result = rate_limiter.check_and_record(test_key).await.unwrap();
    assert!(result.allowed, "Should be allowed after reset");
    assert_eq!(
        result.remaining, 2,
        "Should have 2 remaining after reset and one request"
    );
}

// Test with high concurrency
#[tokio::test]
async fn test_concurrent_requests() {
    // Create fixed window algorithm with a small limit
    let algorithm_config = FixedWindowConfig {
        max_requests: 5,
        window: Duration::from_secs(60),
    };

    // Use the helper to create a test rate limiter
    let rate_limiter = Arc::new(
        create_test_rate_limiter(FixedWindow::new(MockStorage::new(), algorithm_config)).await,
    );

    // Test key
    let test_key = "concurrent_test";

    // Create multiple tasks to hit the rate limiter simultaneously
    let mut handles = Vec::with_capacity(10);
    let barrier = Arc::new(tokio::sync::Barrier::new(10));

    for i in 0..10 {
        let rate_limiter_clone = rate_limiter.clone();
        let barrier_clone = barrier.clone();
        let key = test_key.to_string();

        let handle = tokio::spawn(async move {
            // Wait for all tasks to be ready
            barrier_clone.wait().await;

            // All try to get a request at the same time
            let result = rate_limiter_clone.check_and_record(&key).await.unwrap();
            (i, result.allowed)
        });

        handles.push(handle);
    }

    // Wait for all tasks to complete
    let results = futures::future::join_all(handles).await;

    // Collect results
    let allowed_results: Vec<_> = results.into_iter().map(|r| r.unwrap()).collect();

    // Count how many were allowed
    let allowed_count = allowed_results
        .iter()
        .filter(|(_, allowed)| *allowed)
        .count();

    // Exactly 5 should have been allowed (the limit)
    assert_eq!(
        allowed_count, 5,
        "Exactly 5 concurrent requests should be allowed"
    );

    // Now all additional requests should be denied
    let result = rate_limiter.check_and_record(test_key).await.unwrap();
    assert!(
        !result.allowed,
        "Additional request should be denied after concurrent burst"
    );
}
