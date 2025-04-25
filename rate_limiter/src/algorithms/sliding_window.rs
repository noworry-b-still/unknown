// src/algorithms/sliding_window.rs

use crate::algorithms::{RateLimitAlgorithm, RateLimitStatus};
use crate::config::SlidingWindowConfig;
use crate::error::{RateLimiterError, Result};
use crate::storage::{StorageBackend, StoragePipeline};
use async_trait::async_trait;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Sliding Window rate limiting algorithm
///
/// The sliding window algorithm divides time into smaller buckets and uses them
/// to approximate a continuous sliding window. It provides better granularity
/// than the fixed window approach and avoids the burst problem.
#[derive(Debug, Clone)]
pub struct SlidingWindow<S>
where
    S: StorageBackend,
{
    /// Storage backend for persisting window counters
    storage: S,

    /// Configuration for the sliding window
    config: SlidingWindowConfig,
}

impl<S> SlidingWindow<S>
where
    S: StorageBackend,
{
    /// Creates a new sliding window with the given storage and configuration
    pub fn new(storage: S, config: SlidingWindowConfig) -> Self {
        // Ensure precision is at least 1
        let mut config = config;
        if config.precision == 0 {
            config.precision = 1; // Default to 1 bucket if precision is invalid
        }

        Self { storage, config }
    }

    /// Calculate the current timestamp in seconds
    fn current_time_secs(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs()
    }

    /// Calculate the bucket size in seconds, ensuring it's never zero
    fn bucket_size_secs(&self) -> u64 {
        let window_secs = self.config.window.as_secs().max(1); // Ensure window is at least 1 second
        let precision = self.config.precision.max(1) as u64; // Ensure precision is at least 1

        // Calculate bucket size, ensuring it's at least 1 second
        (window_secs / precision).max(1)
    }

    /// Calculate the current bucket index
    fn current_bucket_index(&self) -> u64 {
        let now = self.current_time_secs();
        let bucket_size = self.bucket_size_secs();

        // No need for division if bucket_size is guaranteed to be at least 1
        now / bucket_size
    }

    /// Get the counts for all relevant buckets
    async fn get_bucket_counts(&self, key: &str) -> Result<Vec<(u64, u64)>> {
        let current_index = self.current_bucket_index();
        let window_buckets = self.config.precision.max(1) as u64;

        // We need to consider all buckets that contribute to the current window
        let start_index = current_index.saturating_sub(window_buckets - 1);

        let mut bucket_counts = Vec::with_capacity(window_buckets as usize);

        // Create a pipeline to fetch all bucket counts
        let mut pipeline = self.storage.pipeline();

        for i in start_index..=current_index {
            let bucket_key = format!("{}:bucket:{}", key, i);
            pipeline.get(&bucket_key);
        }

        // Execute the pipeline
        let results = self.storage.execute_pipeline(pipeline).await?;

        // Process the results
        for (i, result) in results.into_iter().enumerate() {
            let bucket_index = start_index + i as u64;
            let count = match result {
                Ok(bytes) => {
                    if bytes.len() == 8 {
                        let mut arr = [0; 8];
                        arr.copy_from_slice(&bytes);
                        u64::from_be_bytes(arr)
                    } else if !bytes.is_empty() {
                        // Try to parse as string if it's not a byte array
                        match std::str::from_utf8(&bytes) {
                            Ok(s) => s.parse::<u64>().unwrap_or(0),
                            Err(_) => 0,
                        }
                    } else {
                        0
                    }
                }
                Err(_) => 0,
            };

            bucket_counts.push((bucket_index, count));
        }

        Ok(bucket_counts)
    }

    /// Initialize or increment the current bucket
    async fn increment_current_bucket(&self, key: &str) -> Result<u64> {
        let current_index = self.current_bucket_index();
        let bucket_key = format!("{}:bucket:{}", key, current_index);

        // First check if the key exists
        let exists = self.storage.exists(&bucket_key).await?;

        // If key doesn't exist, initialize it first
        if !exists {
            // Initialize with zero (stored as an integer)
            self.storage.set(&bucket_key, b"0", None).await?;
        }

        // Now increment the counter
        let result = self.storage.increment(&bucket_key, 1).await?;

        // Set expiration for the bucket key
        let ttl = self.config.window + Duration::from_secs(60); // Add buffer for clock skew
        self.storage.expire(&bucket_key, ttl).await?;

        Ok(result as u64)
    }

    /// Calculate the total count across all buckets, weighted by their contribution to the current window
    fn calculate_weighted_count(&self, bucket_counts: &[(u64, u64)]) -> u64 {
        if bucket_counts.is_empty() {
            return 0;
        }

        let current_index = self.current_bucket_index();
        let precision = self.config.precision.max(1) as u64;

        let mut total_count = 0;

        for &(index, count) in bucket_counts {
            // Add the count for any bucket within our window
            if index >= current_index.saturating_sub(precision - 1) && index <= current_index {
                total_count += count;
            }
        }

        total_count
    }
}

#[async_trait]
impl<S> RateLimitAlgorithm for SlidingWindow<S>
where
    S: StorageBackend,
{
    type Config = SlidingWindowConfig;

    fn new(_config: Self::Config) -> Self {
        // This implementation is incomplete - we need a storage backend
        // It will be provided by the RateLimiter when constructing the algorithm
        panic!("Use SlidingWindow::new(storage, config) instead");
    }

    async fn is_allowed(&self, key: &str) -> Result<bool> {
        // Guard against any potential zero precision or window
        if self.config.max_requests == 0 {
            return Ok(false);
        }

        let bucket_counts = self.get_bucket_counts(key).await?;
        let total_count = self.calculate_weighted_count(&bucket_counts);

        Ok(total_count < self.config.max_requests)
    }

    async fn record_request(&self, key: &str) -> Result<()> {
        // First check if the request is allowed
        if !self.is_allowed(key).await? {
            return Err(RateLimiterError::LimitExceeded(format!(
                "Sliding window limit exceeded for key: {}",
                key
            )));
        }

        // If allowed, increment the counter
        self.increment_current_bucket(key).await?;

        Ok(())
    }

    async fn check_and_record(&self, key: &str) -> Result<RateLimitStatus> {
        // Guard against any potential zero precision or window
        if self.config.max_requests == 0 {
            return Ok(RateLimitStatus {
                allowed: false,
                remaining: 0,
                limit: self.config.max_requests,
                reset_after: Duration::from_secs(0),
                details: None,
            });
        }

        let bucket_counts = self.get_bucket_counts(key).await?;
        let total_count = self.calculate_weighted_count(&bucket_counts);

        let allowed = total_count < self.config.max_requests;

        // Only increment if allowed
        if allowed {
            self.increment_current_bucket(key).await?;
        }

        // Calculate remaining requests
        let mut remaining = self.config.max_requests.saturating_sub(total_count);
        if allowed {
            // If we just used a token, subtract 1 more
            remaining = remaining.saturating_sub(1);
        }

        // Calculate reset time based on bucket expiration
        let bucket_size = self.bucket_size_secs();
        let reset_after = Duration::from_secs(bucket_size * self.config.precision.max(1) as u64);

        Ok(RateLimitStatus {
            allowed,
            remaining,
            limit: self.config.max_requests,
            reset_after,
            details: None,
        })
    }

    async fn reset(&self, key: &str) -> Result<()> {
        let current_index = self.current_bucket_index();
        let window_buckets = self.config.precision.max(1) as u64;

        // Calculate the oldest bucket in the window
        let start_index = current_index.saturating_sub(window_buckets - 1);

        // Create a pipeline to reset all bucket keys
        let mut pipeline = self.storage.pipeline();

        for i in start_index..=current_index {
            let bucket_key = format!("{}:bucket:{}", key, i);
            // Set all buckets to "0" as a string to ensure Redis compatibility
            pipeline.set(&bucket_key, b"0", None);
        }

        // Execute the pipeline
        self.storage.execute_pipeline(pipeline).await?;

        Ok(())
    }
}
