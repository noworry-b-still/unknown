// src/resilience/tests/mod.rs
//! Tests for resilience features

mod circuit_breaker_tests;
mod exponential_backoff_tests;
mod health_checker_tests;

// Common test utilities for resilience testing
pub(crate) mod utils {
    use crate::config::{InMemoryConfig, RedisConfig};
    use crate::error::Result;
    use crate::storage::{MemoryStorage, RedisStorage};
    use std::time::Duration;

    /// Creates a mock Redis storage for testing
    #[allow(dead_code)]
    pub async fn create_mock_redis() -> Result<RedisStorage> {
        // This assumes a Redis instance is running for tests
        // In a real test environment, you might want to set up a test Redis instance
        let config = RedisConfig {
            url: "redis://localhost:6379".to_string(),
            pool_size: 5,
            connection_timeout: Duration::from_secs(1),
        };

        RedisStorage::new(config).await
    }

    #[allow(dead_code)]
    /// Creates a memory storage for testing
    pub fn create_mock_memory() -> MemoryStorage {
        let config = InMemoryConfig {
            max_entries: 10_000,
            use_background_task: true,
            cleanup_interval: Duration::from_secs(60),
        };

        MemoryStorage::new(config)
    }

    #[allow(dead_code)]
    /// Mock Redis client that can simulate failures
    pub struct MockRedisClient {
        pub fail_next_request: bool,
        pub success_after_failures: Option<usize>,
        pub failure_count: usize,
    }

    #[allow(dead_code)]
    impl MockRedisClient {
        pub fn new() -> Self {
            Self {
                fail_next_request: false,
                success_after_failures: None,
                failure_count: 0,
            }
        }

        pub fn with_failures(failures: usize) -> Self {
            Self {
                fail_next_request: true,
                success_after_failures: Some(failures),
                failure_count: 0,
            }
        }

        pub async fn execute<T>(&mut self, success_fn: impl FnOnce() -> Result<T>) -> Result<T> {
            if self.fail_next_request {
                self.failure_count += 1;

                if let Some(limit) = self.success_after_failures {
                    if self.failure_count >= limit {
                        self.fail_next_request = false;
                        return success_fn();
                    }
                }

                return Err(crate::error::RateLimiterError::Storage(
                    crate::error::StorageError::RedisConnection("Simulated failure".to_string()),
                ));
            }

            success_fn()
        }
    }
}
