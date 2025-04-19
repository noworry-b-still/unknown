// library entry
pub mod algorithms;
pub mod config;
pub mod error;
pub mod logging;
pub mod storage;
pub mod test_utils;
// Re-export the memory module from storage

// Re-export key components for convenience
pub use algorithms::{RateLimitAlgorithm, RateLimitStatus};
pub use config::{FixedWindowConfig, RateLimiterConfig, SlidingWindowConfig, TokenBucketConfig};
pub use error::{RateLimiterError, Result};
pub use logging::init as init_logging;
pub use storage::{StorageBackend, StoragePipeline};

// Main rate limiter that coordinates algorithms and storage
#[derive(Debug)]
pub struct RateLimiter<A, S>
where
    A: RateLimitAlgorithm,
    S: StorageBackend,
{
    // The algorithm to use for rate limiting
    algorithm: A,

    // The storage backend to use
    storage: S,

    // Key prefix to use for all keys in storage
    key_prefix: String,
}

impl<A, S> RateLimiter<A, S>
where
    A: RateLimitAlgorithm,
    S: StorageBackend,
{
    // Creates a new rate limiter with the given algorithm and storage backend
    pub fn new(algorithm: A, storage: S, key_prefix: String) -> Self {
        Self {
            algorithm,
            storage,
            key_prefix,
        }
    }

    // Formats a key with the prefix
    fn format_key(&self, key: &str) -> String {
        format!("{}:{}", self.key_prefix, key)
    }

    // Checks if a request is allowed without recording it
    pub async fn is_allowed(&self, key: &str) -> Result<bool> {
        let formatted_key = self.format_key(key);
        self.algorithm.is_allowed(&formatted_key).await
    }

    // Records that a request was made
    pub async fn record_request(&self, key: &str) -> Result<()> {
        let formatted_key = self.format_key(key);
        self.algorithm.record_request(&formatted_key).await
    }

    // Combined operation to check if a request is allowed and record it if it is
    pub async fn check_and_record(&self, key: &str) -> Result<RateLimitStatus> {
        let formatted_key = self.format_key(key);
        let result = self.algorithm.check_and_record(&formatted_key).await?;

        // Log the rate limiting event
        rate_limit_event!(
            "default", // tenant ID could be customizable in the future
            key,
            result.allowed,
            result.limit,
            result.reset_after.as_secs()
        );

        Ok(result)
    }

    // Reset the rate limit for a specific key
    pub async fn reset(&self, key: &str) -> Result<()> {
        let formatted_key = self.format_key(key);
        self.algorithm.reset(&formatted_key).await
    }

    // Get a reference to the storage backend
    pub fn storage(&self) -> &S {
        &self.storage
    }

    // Get a reference to the algorithm
    pub fn algorithm(&self) -> &A {
        &self.algorithm
    }
}
