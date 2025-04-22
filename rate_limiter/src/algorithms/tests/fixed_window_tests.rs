#[cfg(test)]
mod tests {
    use futures::future::join_all;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Barrier;
    use tokio::time;

    use crate::{
        algorithms::FixedWindow,
        config::FixedWindowConfig,
        test_utils::{create_test_rate_limiter, MockStorage},
    };

    /// Test window boundaries (requests at window start/end)
    #[tokio::test]
    async fn test_window_boundaries() {
        let storage = MockStorage::new();
        let config = FixedWindowConfig {
            max_requests: 5,
            window: Duration::from_secs(2),
        };

        let algorithm = FixedWindow::new(storage, config);
        let rate_limiter = create_test_rate_limiter(algorithm).await;

        // PART 1: Basic window limit test
        // Make requests at the start of a window
        for i in 0..5 {
            let result = rate_limiter
                .check_and_record("fw_boundary_test1")
                .await
                .unwrap();
            assert!(
                result.allowed,
                "Request {} should be allowed at window start",
                i
            );
        }

        // Next request should be denied (window limit reached)
        let result = rate_limiter
            .check_and_record("fw_boundary_test1")
            .await
            .unwrap();
        assert!(
            !result.allowed,
            "Request beyond window limit should be denied"
        );

        // Wait for the window to expire (add extra time to ensure expiration)
        time::sleep(Duration::from_secs(3)).await;

        // Should be allowed again in the new window
        let result = rate_limiter
            .check_and_record("fw_boundary_test1")
            .await
            .unwrap();
        assert!(result.allowed, "Request in new window should be allowed");

        // PART 2: Test with a separate key to avoid interference
        let test_key = "fw_boundary_test2";

        // Make 4 requests (leaving 1 remaining)
        for i in 0..4 {
            let result = rate_limiter.check_and_record(test_key).await.unwrap();
            assert!(result.allowed, "Request {} should be allowed", i);
        }

        // Verify we can make one more request
        let result = rate_limiter.check_and_record(test_key).await.unwrap();
        assert!(result.allowed, "Fifth request should be allowed");

        // Next request should be denied
        let result = rate_limiter.check_and_record(test_key).await.unwrap();
        assert!(!result.allowed, "Sixth request should be denied");
    }

    /// Test window resets
    #[tokio::test]
    async fn test_window_resets() {
        let storage = MockStorage::new();
        let config = FixedWindowConfig {
            max_requests: 3,
            window: Duration::from_secs(1),
        };

        let algorithm = FixedWindow::new(storage, config);
        let rate_limiter = create_test_rate_limiter(algorithm).await;

        // Use all requests in a window
        for _ in 0..3 {
            rate_limiter
                .check_and_record("fw_reset_test")
                .await
                .unwrap();
        }

        // Verify rate limit is enforced
        let result = rate_limiter
            .check_and_record("fw_reset_test")
            .await
            .unwrap();
        assert!(!result.allowed, "Should be rate limited");

        // Wait for the window to reset (a little extra to ensure reset)
        time::sleep(Duration::from_secs(1) + Duration::from_millis(50)).await;

        // Should be allowed again
        let result = rate_limiter
            .check_and_record("fw_reset_test")
            .await
            .unwrap();
        assert!(
            result.allowed,
            "Request after window reset should be allowed"
        );
        assert_eq!(
            result.remaining, 2,
            "Should have 2 requests remaining after reset"
        );

        // Make sure reset_after is calculated correctly
        let result = rate_limiter
            .check_and_record("fw_reset_test")
            .await
            .unwrap();
        assert!(
            result.reset_after <= Duration::from_secs(1),
            "Reset time should be at most the window duration"
        );
    }

    /// Test counter accuracy
    #[tokio::test]
    async fn test_counter_accuracy() {
        let storage = MockStorage::new();
        let config = FixedWindowConfig {
            max_requests: 10,
            window: Duration::from_secs(2),
        };

        let algorithm = FixedWindow::new(storage, config);
        let rate_limiter = create_test_rate_limiter(algorithm).await;

        // Make requests and verify counter
        for i in 0..10 {
            let result = rate_limiter
                .check_and_record("fw_counter_test")
                .await
                .unwrap();
            assert!(result.allowed, "Request {} should be allowed", i);
            assert_eq!(
                result.remaining,
                10 - i - 1,
                "Remaining count should be accurate"
            );
        }

        // Counter should be 0 when limit is reached
        let result = rate_limiter
            .check_and_record("fw_counter_test")
            .await
            .unwrap();
        assert!(!result.allowed, "Request beyond limit should be denied");
        assert_eq!(
            result.remaining, 0,
            "Remaining should be 0 when limit reached"
        );

        // Verify limit and reset time are reported correctly
        assert_eq!(result.limit, 10, "Limit should match configuration");
        assert!(
            result.reset_after > Duration::from_secs(0),
            "Reset time should be positive"
        );
        assert!(
            result.reset_after <= Duration::from_secs(2),
            "Reset time should not exceed window"
        );
    }

    /// Test multi-window behavior
    #[tokio::test]
    async fn test_multi_window_behavior() {
        let storage = MockStorage::new();
        let config = FixedWindowConfig {
            max_requests: 5,
            window: Duration::from_secs(1),
        };

        let algorithm = FixedWindow::new(storage, config);
        let rate_limiter = create_test_rate_limiter(algorithm).await;

        // Test behavior across multiple windows

        // Window 1: Use all requests
        for _ in 0..5 {
            rate_limiter
                .check_and_record("fw_multi_window_test")
                .await
                .unwrap();
        }
        let result = rate_limiter
            .check_and_record("fw_multi_window_test")
            .await
            .unwrap();
        assert!(!result.allowed, "Window 1: Exceeded limit should be denied");

        // Wait for window 2 (add a little buffer for timing)
        time::sleep(Duration::from_secs(1) + Duration::from_millis(50)).await;

        // Window 2: Use 3 requests
        for _ in 0..3 {
            let result = rate_limiter
                .check_and_record("fw_multi_window_test")
                .await
                .unwrap();
            assert!(result.allowed, "Window 2: Request should be allowed");
        }

        // Wait for window 3
        time::sleep(Duration::from_secs(1) + Duration::from_millis(50)).await;

        // Window 3: Should have full allowance again
        for i in 0..5 {
            let result = rate_limiter
                .check_and_record("fw_multi_window_test")
                .await
                .unwrap();
            assert!(result.allowed, "Window 3: Request {} should be allowed", i);
        }

        // Ensure windows are independent
        let result = rate_limiter
            .check_and_record("fw_multi_window_new_user")
            .await
            .unwrap();
        assert!(result.allowed, "New user should get their own window");
    }

    /// Test concurrent access to the same window
    #[tokio::test]
    async fn test_concurrent_window_access() {
        let storage = MockStorage::new();
        let config = FixedWindowConfig {
            max_requests: 5,
            window: Duration::from_secs(10), // Long window for test
        };

        let algorithm = FixedWindow::new(storage, config);
        let rate_limiter = Arc::new(create_test_rate_limiter(algorithm).await);
        let key = "fw_concurrent_test";

        // Create a barrier to synchronize all tasks
        let barrier = Arc::new(Barrier::new(10));
        let mut handles = Vec::with_capacity(10);

        // Launch 10 concurrent tasks
        for i in 0..10 {
            let rate_limiter_clone = rate_limiter.clone();
            let barrier_clone = barrier.clone();
            let key = key.to_string();

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
        let results = join_all(handles).await;

        // Extract results and count how many succeeded
        let results: Vec<_> = results.into_iter().map(|r| r.unwrap()).collect();
        let allowed_count = results.iter().filter(|(_, allowed)| *allowed).count();

        // Exactly 5 tasks should have succeeded (the window allows 5 requests)
        assert_eq!(
            allowed_count, 5,
            "Expected exactly 5 out of 10 concurrent requests to be allowed"
        );
    }

    /// Test edge cases (zero requests, small windows)
    #[tokio::test]
    async fn test_edge_cases() {
        // Test with zero max_requests
        let storage1 = MockStorage::new();
        let config = FixedWindowConfig {
            max_requests: 0,
            window: Duration::from_secs(60), // Use a large window to avoid timing issues
        };

        let algorithm = FixedWindow::new(storage1, config);
        let rate_limiter = create_test_rate_limiter(algorithm).await;

        let result = rate_limiter
            .check_and_record("fw_edge_zero_requests")
            .await
            .unwrap();
        assert!(
            !result.allowed,
            "Request with zero max_requests should be denied"
        );

        // Test with very small window (but not zero)
        let storage2 = MockStorage::new();
        let config = FixedWindowConfig {
            max_requests: 1,
            window: Duration::from_secs(1), // 1 second window - minimum non-zero value
        };

        let algorithm = FixedWindow::new(storage2, config);
        let rate_limiter = create_test_rate_limiter(algorithm).await;

        // First request should be allowed
        let result = rate_limiter
            .check_and_record("fw_edge_small_window")
            .await
            .unwrap();
        assert!(
            result.allowed,
            "First request in small window should be allowed"
        );

        // Second request should be denied
        let result = rate_limiter
            .check_and_record("fw_edge_small_window")
            .await
            .unwrap();
        assert!(
            !result.allowed,
            "Second request in small window should be denied"
        );

        // Wait for the small window to expire (a bit longer to ensure it's expired)
        time::sleep(Duration::from_secs(1) + Duration::from_millis(100)).await;

        // Should be allowed again
        let result = rate_limiter
            .check_and_record("fw_edge_small_window")
            .await
            .unwrap();
        assert!(
            result.allowed,
            "Request after small window expiry should be allowed"
        );

        // Test with large window and large request limit
        let storage3 = MockStorage::new();
        let config = FixedWindowConfig {
            max_requests: 1000,
            window: Duration::from_secs(3600), // 1 hour window
        };

        let algorithm = FixedWindow::new(storage3, config);
        let rate_limiter = create_test_rate_limiter(algorithm).await;

        // Make just a few requests to avoid taking too long
        for i in 0..10 {
            let result = rate_limiter
                .check_and_record("fw_edge_large_window")
                .await
                .unwrap();
            assert!(
                result.allowed,
                "Request {} with large limit should be allowed",
                i
            );
            // Don't test the exact remaining count
        }
    }

    /// Test reset functionality
    #[tokio::test]
    async fn test_reset_functionality() {
        let storage = MockStorage::new();
        let config = FixedWindowConfig {
            max_requests: 5,
            window: Duration::from_secs(60), // 1 minute window
        };

        let algorithm = FixedWindow::new(storage, config);
        let rate_limiter = create_test_rate_limiter(algorithm).await;

        // Use all requests
        for _ in 0..5 {
            rate_limiter
                .check_and_record("fw_reset_test")
                .await
                .unwrap();
        }

        // Verify limit is reached
        let result = rate_limiter
            .check_and_record("fw_reset_test")
            .await
            .unwrap();
        assert!(
            !result.allowed,
            "Should be rate limited after using all requests"
        );

        // Reset the rate limiter
        rate_limiter.reset("fw_reset_test").await.unwrap();

        // Should have full allowance again
        let result = rate_limiter
            .check_and_record("fw_reset_test")
            .await
            .unwrap();
        assert!(result.allowed, "Should be allowed after reset");
        assert_eq!(
            result.remaining, 4,
            "Should have 4 requests remaining after reset and using 1"
        );
    }
}
