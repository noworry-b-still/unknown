// src/algorithms/token_bucket.rs

use crate::algorithms::{RateLimitAlgorithm, RateLimitStatus};
use crate::config::TokenBucketConfig;
use crate::error::{RateLimiterError, Result};
use crate::storage::{StorageBackend, StoragePipeline};
use async_trait::async_trait;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Token Bucket rate limiting algorithm
///
/// The token bucket algorithm works by maintaining a "bucket" of tokens that are
/// replenished at a constant rate. Each request consumes a token, and if there
/// are no tokens available, the request is rejected.
#[derive(Debug)]
pub struct TokenBucket<S>
where
    S: StorageBackend,
{
    /// Storage backend for persisting bucket state
    storage: S,

    /// Configuration for the token bucket
    config: TokenBucketConfig,
}

impl<S> TokenBucket<S>
where
    S: StorageBackend,
{
    /// Creates a new token bucket with the given storage and configuration
    pub fn new(storage: S, config: TokenBucketConfig) -> Self {
        Self { storage, config }
    }

    /// Get the current token count and last refill time for a key
    async fn get_bucket_state(&self, key: &str) -> Result<(u64, u64)> {
        // Get the last refill time
        let last_refill_key = format!("{}:last_refill", key);
        let last_refill = match self.storage.get(&last_refill_key).await? {
            Some(bytes) => {
                if bytes.len() == 8 {
                    let mut arr = [0; 8];
                    arr.copy_from_slice(&bytes);
                    u64::from_be_bytes(arr)
                } else {
                    // If the value is invalid, use current time
                    current_time_millis()
                }
            }
            None => current_time_millis(),
        };

        // Get the current token count
        let tokens_key = format!("{}:tokens", key);
        let tokens = match self.storage.get(&tokens_key).await? {
            Some(bytes) => {
                if bytes.len() == 8 {
                    let mut arr = [0; 8];
                    arr.copy_from_slice(&bytes);
                    u64::from_be_bytes(arr)
                } else {
                    // If the value is invalid, use capacity
                    self.config.capacity
                }
            }
            None => self.config.initial_tokens.unwrap_or(self.config.capacity),
        };

        Ok((tokens, last_refill))
    }

    /// Update the bucket state with new token count and refill time
    async fn update_bucket_state(&self, key: &str, tokens: u64, last_refill: u64) -> Result<()> {
        let last_refill_key = format!("{}:last_refill", key);
        let tokens_key = format!("{}:tokens", key);

        // TTL for keys - set to a reasonable value to prevent storage leaks
        let ttl = Duration::from_secs(60 * 60 * 24); // 24 hours

        // Create a pipeline to update both values atomically
        let mut pipeline = self.storage.pipeline();
        pipeline.set(&last_refill_key, &last_refill.to_be_bytes(), Some(ttl));
        pipeline.set(&tokens_key, &tokens.to_be_bytes(), Some(ttl));
        // Execute the pipeline
        self.storage.execute_pipeline(pipeline).await?;

        Ok(())
    }

    /// Calculate the number of tokens that should be refilled based on elapsed time
    fn calculate_refill(&self, tokens: u64, last_refill: u64, current_time: u64) -> u64 {
        // Calculate elapsed time in milliseconds
        let elapsed = current_time.saturating_sub(last_refill);

        // Convert elapsed time to seconds for rate calculation
        let elapsed_seconds = elapsed as f64 / 1000.0;

        // Calculate refill amount
        let refill_amount = (elapsed_seconds * self.config.refill_rate) as u64;

        // Add refill amount to current tokens, but don't exceed capacity
        std::cmp::min(tokens + refill_amount, self.config.capacity)
    }
}

#[async_trait]
impl<S> RateLimitAlgorithm for TokenBucket<S>
where
    S: StorageBackend,
{
    type Config = TokenBucketConfig;

    fn new(_config: Self::Config) -> Self {
        // This implementation is incomplete - we need a storage backend
        // It will be provided by the RateLimiter when constructing the algorithm
        panic!("Use TokenBucket::new(storage, config) instead");
    }

    async fn is_allowed(&self, key: &str) -> Result<bool> {
        let current_time = current_time_millis();
        let (tokens, last_refill) = self.get_bucket_state(key).await?;

        // Calculate how many tokens should be in the bucket now
        let new_tokens = self.calculate_refill(tokens, last_refill, current_time);

        // If we have at least one token, the request is allowed
        Ok(new_tokens >= 1)
    }

    async fn record_request(&self, key: &str) -> Result<()> {
        let current_time = current_time_millis();
        let (tokens, last_refill) = self.get_bucket_state(key).await?;

        // Calculate how many tokens should be in the bucket now
        let new_tokens = self.calculate_refill(tokens, last_refill, current_time);

        // If we have at least one token, consume it
        if new_tokens >= 1 {
            // Update the bucket state
            self.update_bucket_state(key, new_tokens - 1, current_time)
                .await?;
            Ok(())
        } else {
            Err(RateLimiterError::LimitExceeded(format!(
                "Token bucket depleted for key: {}",
                key
            )))
        }
    }

    async fn check_and_record(&self, key: &str) -> Result<RateLimitStatus> {
        let current_time = current_time_millis();
        let (tokens, last_refill) = self.get_bucket_state(key).await?;

        // Calculate how many tokens should be in the bucket now
        let new_tokens = self.calculate_refill(tokens, last_refill, current_time);

        // Prepare the status
        let allowed = new_tokens >= 1;

        // Only update if allowed
        let remaining = if allowed {
            // Consume a token
            self.update_bucket_state(key, new_tokens - 1, current_time)
                .await?;
            new_tokens - 1
        } else {
            new_tokens
        };

        // Calculate reset time (time until next token)
        let tokens_needed = if remaining > 0 { 0 } else { 1 - remaining };
        let seconds_until_refill = if self.config.refill_rate > 0.0 {
            tokens_needed as f64 / self.config.refill_rate
        } else {
            0.0
        };

        Ok(RateLimitStatus {
            allowed,
            remaining,
            limit: self.config.capacity,
            reset_after: Duration::from_secs_f64(seconds_until_refill),
            details: None,
        })
    }

    async fn reset(&self, key: &str) -> Result<()> {
        // Reset the bucket to full capacity
        self.update_bucket_state(key, self.config.capacity, current_time_millis())
            .await
    }
}

/// Helper function to get current time in milliseconds
fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis() as u64
}
