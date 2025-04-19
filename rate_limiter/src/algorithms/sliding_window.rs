// src/algorithms/sliding_window.rs

use super::super::algorithms::{RateLimitAlgorithm, RateLimitStatus};
use super::super::config::SlidingWindowConfig;
use super::super::error::{RateLimiterError, Result};
use super::super::storage::{StorageBackend, StoragePipeline};
use async_trait::async_trait;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Sliding Window rate limiting algorithm
///
/// The sliding window algorithm divides time into smaller buckets and uses them
/// to approximate a continuous sliding window. It provides better granularity
/// than the fixed window approach and avoids the burst problem.
#[derive(Debug)]
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
        Self { storage, config }
    }

    /// Calculate the current timestamp in seconds
    fn current_time_secs(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs()
    }

    /// Calculate the bucket size in seconds
    fn bucket_size_secs(&self) -> u64 {
        self.config.window.as_secs() / self.config.precision as u64
    }

    /// Calculate the current bucket index
    fn current_bucket_index(&self) -> u64 {
        let now = self.current_time_secs();
        now / self.bucket_size_secs()
    }

    /// Get the counts for all relevant buckets
    async fn get_bucket_counts(&self, key: &str) -> Result<Vec<(u64, u64)>> {
        let current_index = self.current_bucket_index();
        let window_buckets = self.config.precision as u64;

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

    /// Increment the count for the current bucket
    async fn increment_current_bucket(&self, key: &str) -> Result<u64> {
        let current_index = self.current_bucket_index();
        let bucket_key = format!("{}:bucket:{}", key, current_index);

        // Increment the counter
        let result = self.storage.increment(&bucket_key, 1).await?;

        // Set expiration for the bucket key
        // We need to keep buckets around for the window duration
        let ttl = self.config.window + Duration::from_secs(60); // Add buffer for clock skew
        self.storage.expire(&bucket_key, ttl).await?;

        Ok(result as u64)
    }

    /// Calculate the total count across all buckets, weighted by their contribution to the current window
    fn calculate_weighted_count(&self, bucket_counts: &[(u64, u64)]) -> u64 {
        let current_index = self.current_bucket_index();
        let bucket_size = self.bucket_size_secs();
        let current_time = self.current_time_secs();
        let current_bucket_start = current_index * bucket_size;
        let time_in_current_bucket = current_time - current_bucket_start;

        let mut total_count = 0;

        for &(index, count) in bucket_counts {
            if index == current_index {
                // Current bucket is weighted by the fraction of time elapsed
                let weight = time_in_current_bucket as f64 / bucket_size as f64;
                total_count += (count as f64 * weight) as u64;
            } else if index > current_index.saturating_sub(self.config.precision as u64) {
                // Previous buckets within the window
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
        let bucket_counts = self.get_bucket_counts(key).await?;
        let total_count = self.calculate_weighted_count(&bucket_counts);

        let allowed = total_count < self.config.max_requests;

        // Only increment if allowed
        if allowed {
            self.increment_current_bucket(key).await?;
        }

        // Calculate time until window resets
        // This is approximate - for a sliding window, it depends on request patterns
        let remaining = self
            .config
            .max_requests
            .saturating_sub(total_count + if allowed { 1 } else { 0 });

        // Calculate reset time based on oldest bucket cycling out
        let bucket_size = self.bucket_size_secs();
        let reset_after = Duration::from_secs(bucket_size * self.config.precision as u64);

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
        let window_buckets = self.config.precision as u64;

        // Delete all buckets that contribute to the current window
        let start_index = current_index.saturating_sub(window_buckets - 1);

        // Create a pipeline to delete all bucket keys
        let mut pipeline = self.storage.pipeline();

        for i in start_index..=current_index {
            let bucket_key = format!("{}:bucket:{}", key, i);
            // We can't directly delete in a pipeline, so we'll set to 0
            pipeline.set(&bucket_key, &0u64.to_be_bytes(), None);
        }

        // Execute the pipeline
        self.storage.execute_pipeline(pipeline).await?;

        Ok(())
    }
}
