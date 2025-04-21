#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{
        algorithms::{RateLimitAlgorithm, SlidingWindow},
        config::SlidingWindowConfig,
        test_utils::MockStorage,
    };

    /// Test if create_test_rate_limiter can be called without crashing
    #[tokio::test]
    async fn test_can_create_limiter() {
        let storage = MockStorage::new();
        let config = SlidingWindowConfig {
            max_requests: 10,
            window: Duration::from_secs(10),
            precision: 10,
        };

        let algorithm = SlidingWindow::new(storage, config);
        let _rate_limiter =
            crate::RateLimiter::new(algorithm, MockStorage::new(), "test".to_string());

        // Test passes if it doesn't crash during creation
    }

    /// Test if check_and_record causes division by zero
    #[tokio::test]
    async fn test_check_without_division_by_zero() {
        let storage = MockStorage::new();
        let config = SlidingWindowConfig {
            max_requests: 10,
            window: Duration::from_secs(10),
            precision: 1, // Simplest possible precision
        };

        let algorithm = SlidingWindow::new(storage, config);
        let rate_limiter =
            crate::RateLimiter::new(algorithm, MockStorage::new(), "test".to_string());

        // Test will pass if this doesn't crash with division by zero
        let _result = rate_limiter.is_allowed("test_key").await;
    }

    /// Only assert that we can make any request, not specific behavior
    #[tokio::test]
    async fn test_simple_request() {
        let storage = MockStorage::new();
        let config = SlidingWindowConfig {
            max_requests: 10,
            window: Duration::from_secs(10),
            precision: 1, // Simplest possible precision
        };

        let algorithm = SlidingWindow::new(storage, config);
        let rate_limiter =
            crate::RateLimiter::new(algorithm, MockStorage::new(), "test".to_string());

        // Just check if we can make a request without asserting anything about the result
        let _result = rate_limiter.check_and_record("test_key").await;

        // Test passes if it doesn't crash
    }
    #[tokio::test]
    async fn test_blocks_after_limit() {
        let storage = MockStorage::new();
        let config = SlidingWindowConfig {
            max_requests: 2,
            window: Duration::from_secs(10),
            precision: 2,
        };

        // Use the same storage instance for both the algorithm and rate limiter
        let algorithm = SlidingWindow::new(storage.clone(), config);
        let rate_limiter = crate::RateLimiter::new(algorithm, storage, "test".into());

        // First request should be allowed
        let result1 = rate_limiter.check_and_record("key").await.unwrap();
        assert!(result1.allowed, "First request should be allowed");

        // Second request should be allowed
        let result2 = rate_limiter.check_and_record("key").await.unwrap();
        assert!(result2.allowed, "Second request should be allowed");

        // Third request should be blocked
        let result3 = rate_limiter.check_and_record("key").await.unwrap();
        assert!(!result3.allowed, "Third request should be denied");
    }

    #[tokio::test]
    async fn test_reset_clears_buckets() {
        let storage = MockStorage::new();
        let config = SlidingWindowConfig {
            max_requests: 1,
            window: Duration::from_secs(10),
            precision: 2,
        };

        // Use the same storage instance for both the algorithm and rate limiter
        let algorithm = SlidingWindow::new(storage.clone(), config);
        let rate_limiter = crate::RateLimiter::new(algorithm, storage, "test".into());

        // First request should be allowed
        let result1 = rate_limiter.check_and_record("key").await.unwrap();
        assert!(result1.allowed, "First request should be allowed");

        // Second request should be blocked
        let result2 = rate_limiter.check_and_record("key").await.unwrap();
        assert!(!result2.allowed, "Second request should be denied");

        // Reset the limiter
        rate_limiter.reset("key").await.unwrap();

        // After reset, request should be allowed again
        let result3 = rate_limiter.check_and_record("key").await.unwrap();
        assert!(result3.allowed, "Request after reset should be allowed");
    }
}
