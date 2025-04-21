#[cfg(test)]
mod tests {
    use futures::future::join_all;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Barrier;
    use tokio::time;

    use crate::{
        algorithms::{RateLimitAlgorithm, TokenBucket},
        config::TokenBucketConfig,
        test_utils::{create_test_rate_limiter, test_rate_limit_scenario, MockStorage, TestClock},
    };
    /// Test token consumption and token depletion
    #[tokio::test]
    async fn test_token_consumption_and_depletion() {
        let storage = MockStorage::new();
        let config = TokenBucketConfig {
            capacity: 5,
            refill_rate: 1.0,
            initial_tokens: Some(5),
        };

        let algorithm = TokenBucket::new(storage, config);
        let rate_limiter = create_test_rate_limiter(algorithm).await;

        // Test token consumption
        for i in 0..5 {
            let result = rate_limiter
                .check_and_record("consumption_test_user")
                .await
                .unwrap();
            assert!(result.allowed, "Request {} should be allowed", i);
            assert_eq!(
                result.remaining,
                5 - i - 1,
                "Should have {} tokens remaining",
                5 - i - 1
            );
        }

        // Test token depletion
        let result = rate_limiter
            .check_and_record("consumption_test_user")
            .await
            .unwrap();
        assert!(
            !result.allowed,
            "Request when tokens depleted should be denied"
        );
        assert_eq!(result.remaining, 0, "Should have 0 tokens remaining");

        // Test multiple users (keys)
        let result = rate_limiter
            .check_and_record("consumption_different_user")
            .await
            .unwrap();
        assert!(result.allowed, "Different user should be allowed");
        assert_eq!(
            result.remaining, 4,
            "Different user should have 4 tokens remaining"
        );
    }

    /// Test token regeneration over time using TestClock
    /// Test token regeneration over time
    #[tokio::test]
    async fn test_token_regeneration_over_time() {
        // For the refill test, we need to have smaller values and wait longer
        let storage = MockStorage::new();
        let config = TokenBucketConfig {
            capacity: 5,
            refill_rate: 10.0, // Faster refill for tests
            initial_tokens: Some(1),
        };

        let algorithm = TokenBucket::new(storage, config);
        let rate_limiter = create_test_rate_limiter(algorithm).await;

        // Use the only token
        let result = rate_limiter
            .check_and_record("regen_test_user")
            .await
            .unwrap();
        assert!(result.allowed, "First request should be allowed");
        assert_eq!(result.remaining, 0, "Should have 0 tokens remaining");

        // Next request should be denied
        let result = rate_limiter
            .check_and_record("regen_test_user")
            .await
            .unwrap();
        assert!(!result.allowed, "Request with no tokens should be denied");

        // Wait for refill - with a rate of 10 tokens/second, we should get 1 token in 100ms
        time::sleep(Duration::from_millis(150)).await;

        // Should be able to make one request now
        let result = rate_limiter
            .check_and_record("regen_test_user")
            .await
            .unwrap();
        assert!(
            result.allowed,
            "Request after token regeneration should be allowed"
        );

        // Wait for more tokens
        time::sleep(Duration::from_millis(400)).await;

        // Should be able to make more requests now
        for i in 0..4 {
            let result = rate_limiter
                .check_and_record("regen_test_user")
                .await
                .unwrap();
            assert!(
                result.allowed,
                "Request {} after more regeneration should be allowed",
                i
            );
        }
    }
    /// Test token regeneration with real time pauses (alternative approach)
    #[tokio::test]
    async fn test_token_regeneration_with_real_time() {
        let storage = MockStorage::new();
        let config = TokenBucketConfig {
            capacity: 3,
            refill_rate: 2.0, // 2 tokens per second
            initial_tokens: Some(1),
        };

        let algorithm = TokenBucket::new(storage, config);
        let rate_limiter = create_test_rate_limiter(algorithm).await;

        // Use the initial token
        let result = rate_limiter
            .check_and_record("realtime_test_user")
            .await
            .unwrap();
        assert!(result.allowed, "First request should be allowed");

        // Next request should be denied
        let result = rate_limiter
            .check_and_record("realtime_test_user")
            .await
            .unwrap();
        assert!(!result.allowed, "Second request should be denied");

        // Wait for token regeneration (1 second should give us 2 tokens)
        time::sleep(Duration::from_secs(1)).await;

        // Should be able to make 2 requests now
        for i in 0..2 {
            let result = rate_limiter
                .check_and_record("realtime_test_user")
                .await
                .unwrap();
            assert!(
                result.allowed,
                "Request {} after waiting should be allowed",
                i
            );
        }

        // Third request should be denied
        let result = rate_limiter
            .check_and_record("realtime_test_user")
            .await
            .unwrap();
        assert!(!result.allowed, "Third request should be denied");
    }

    /// Test concurrent access patterns
    #[tokio::test]
    async fn test_concurrent_access_patterns() {
        let storage = MockStorage::new();
        let config = TokenBucketConfig {
            capacity: 10,
            refill_rate: 0.0, // No refill during test
            initial_tokens: Some(5),
        };

        let algorithm = TokenBucket::new(storage, config);
        let rate_limiter = Arc::new(create_test_rate_limiter(algorithm).await);
        let key = "concurrent_test_user_unique";

        // Create a barrier to synchronize all tasks
        let barrier = Arc::new(Barrier::new(20));
        let mut handles = Vec::with_capacity(20);

        // Launch 20 concurrent tasks
        for i in 0..20 {
            let rate_limiter_clone = rate_limiter.clone();
            let barrier_clone = barrier.clone();
            let key = key.to_string();

            let handle = tokio::spawn(async move {
                // Wait for all tasks to be ready
                barrier_clone.wait().await;

                // All try to get a token at the same time
                let result = rate_limiter_clone.check_and_record(&key).await.unwrap();
                (i, result.allowed)
            });

            handles.push(handle);
        }

        // Wait for all tasks to complete
        let results = join_all(handles).await;

        // Extract results and count how many succeeded
        let results: Vec<_> = results.into_iter().map(|r| r.unwrap()).collect();
        let allowed_count = results.iter().filter(|(_, allowed)| *allowed).count();

        // Exactly 5 tasks should have succeeded (we had 5 tokens)
        assert_eq!(
            allowed_count, 5,
            "Expected exactly 5 out of 20 concurrent requests to be allowed"
        );
    }

    /// Test edge cases (zero capacity, zero refill rate)
    /// Test edge cases (zero capacity, zero refill rate)
    #[tokio::test]
    async fn test_edge_cases() {
        // Test with zero capacity
        let storage1 = MockStorage::new();
        let config = TokenBucketConfig {
            capacity: 0,
            refill_rate: 1.0,
            initial_tokens: None,
        };

        let algorithm = TokenBucket::new(storage1, config);
        let rate_limiter = create_test_rate_limiter(algorithm).await;

        let result = rate_limiter
            .check_and_record("edge_test_zero_capacity")
            .await
            .unwrap();
        assert!(
            !result.allowed,
            "Request with zero capacity should be denied"
        );

        // Test with zero refill rate
        let storage2 = MockStorage::new();
        let config = TokenBucketConfig {
            capacity: 3,
            refill_rate: 0.0,
            initial_tokens: Some(3),
        };

        let algorithm = TokenBucket::new(storage2, config);
        let rate_limiter = create_test_rate_limiter(algorithm).await;

        // Should allow 3 requests
        for i in 0..3 {
            let result = rate_limiter
                .check_and_record("edge_test_zero_refill")
                .await
                .unwrap();
            assert!(result.allowed, "Request {} should be allowed", i);
        }

        // 4th request should be denied
        let result = rate_limiter
            .check_and_record("edge_test_zero_refill")
            .await
            .unwrap();
        assert!(!result.allowed, "Request 4 should be denied");

        // Wait a bit, but since refill_rate is 0, no tokens should regenerate
        time::sleep(Duration::from_millis(100)).await;

        let result = rate_limiter
            .check_and_record("edge_test_zero_refill")
            .await
            .unwrap();
        assert!(
            !result.allowed,
            "Request after waiting with zero refill rate should still be denied"
        );

        // Test with very high refill rate - need to wait longer
        let storage3 = MockStorage::new();
        let config = TokenBucketConfig {
            capacity: 10,
            refill_rate: 50.0,       // 50 tokens per second (1 token every 20ms)
            initial_tokens: Some(1), // Start with 1 token to verify it works
        };

        let algorithm = TokenBucket::new(storage3, config);
        let rate_limiter = create_test_rate_limiter(algorithm).await;

        // Use the initial token
        let result = rate_limiter
            .check_and_record("edge_test_high_refill")
            .await
            .unwrap();
        assert!(result.allowed, "First request should be allowed");

        // Wait longer for token regeneration (50ms should give us ~2-3 tokens)
        time::sleep(Duration::from_millis(50)).await;

        // Should have gotten at least 1 token in 50ms
        let result = rate_limiter
            .check_and_record("edge_test_high_refill")
            .await
            .unwrap();
        assert!(
            result.allowed,
            "Request with high refill rate should be allowed after waiting"
        );
    }
    /// Test reset functionality
    #[tokio::test]
    async fn test_reset_functionality() {
        let storage = MockStorage::new();
        let config = TokenBucketConfig {
            capacity: 5,
            refill_rate: 1.0,
            initial_tokens: Some(5),
        };

        let algorithm = TokenBucket::new(storage, config);
        let rate_limiter = create_test_rate_limiter(algorithm).await;

        // Use all tokens
        for _ in 0..5 {
            rate_limiter
                .check_and_record("reset_test_user")
                .await
                .unwrap();
        }

        // Verify tokens are depleted
        let result = rate_limiter
            .check_and_record("reset_test_user")
            .await
            .unwrap();
        assert!(
            !result.allowed,
            "Should be rate limited after using all tokens"
        );

        // Reset the rate limiter
        rate_limiter.reset("reset_test_user").await.unwrap();

        // Should have full capacity again
        let result = rate_limiter
            .check_and_record("reset_test_user")
            .await
            .unwrap();
        assert!(result.allowed, "Should be allowed after reset");
        assert_eq!(
            result.remaining, 4,
            "Should have 4 tokens remaining after reset and using 1"
        );
    }
}
