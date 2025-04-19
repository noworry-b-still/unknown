// src/algorithms/fixed_window.rs

use super::super::algorithms::{RateLimitAlgorithm, RateLimitStatus};
use super::super::config::FixedWindowConfig;
use super::super::error::{RateLimiterError, Result};
use super::super::storage::StorageBackend;
use async_trait::async_trait;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Fixed Window rate limiting algorithm
///
/// The fixed window algorithm divides time into fixed windows (e.g., 1 minute)
/// and limits the number of requests in each window. When a new window starts,
/// the counter resets.
#[derive(Debug)]
pub struct FixedWindow<S>
where
    S: StorageBackend,
{
    /// Storage backend for persisting window counters
    storage: S,

    /// Configuration for the fixed window
    config: FixedWindowConfig,
}

impl<S> FixedWindow<S>
where
    S: StorageBackend,
{
    /// Creates a new fixed window with the given storage and configuration
    pub fn new(storage: S, config: FixedWindowConfig) -> Self {
        Self { storage, config }
    }

    /// Calculate the current window timestamp
    fn current_window(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();

        // Calculate the window start time by truncating to window size
        now - (now % self.config.window.as_secs())
    }

    /// Get the counter for the current window
    async fn get_window_counter(&self, key: &str) -> Result<u64> {
        let window = self.current_window();
        let counter_key = format!("{}:{}:{}", key, "window", window);

        match self.storage.get(&counter_key).await? {
            Some(bytes) => {
                if bytes.len() == 8 {
                    let mut arr = [0; 8];
                    arr.copy_from_slice(&bytes);
                    Ok(u64::from_be_bytes(arr))
                } else {
                    // Invalid value, treat as zero
                    Ok(0)
                }
            }
            None => Ok(0),
        }
    }

    /// Increment the counter for the current window
    async fn increment_window_counter(&self, key: &str) -> Result<u64> {
        let window = self.current_window();
        let counter_key = format!("{}:{}:{}", key, "window", window);

        // Convert to i64 for storage increment operation
        let result = self.storage.increment(&counter_key, 1).await?;

        // Set expiration for the counter key
        // Add some buffer to the TTL to account for clock skew
        let ttl = self.config.window + Duration::from_secs(60);
        self.storage.expire(&counter_key, ttl).await?;

        // Convert back to u64
        Ok(result as u64)
    }
}

#[async_trait]
impl<S> RateLimitAlgorithm for FixedWindow<S>
where
    S: StorageBackend,
{
    type Config = FixedWindowConfig;

    fn new(_config: Self::Config) -> Self {
        // This implementation is incomplete - we need a storage backend
        // It will be provided by the RateLimiter when constructing the algorithm
        panic!("Use FixedWindow::new(storage, config) instead");
    }

    async fn is_allowed(&self, key: &str) -> Result<bool> {
        let counter = self.get_window_counter(key).await?;
        Ok(counter < self.config.max_requests)
    }

    async fn record_request(&self, key: &str) -> Result<()> {
        let counter = self.increment_window_counter(key).await?;

        if counter <= self.config.max_requests {
            Ok(())
        } else {
            Err(RateLimiterError::LimitExceeded(format!(
                "Fixed window limit exceeded for key: {}",
                key
            )))
        }
    }

    async fn check_and_record(&self, key: &str) -> Result<RateLimitStatus> {
        let counter = self.get_window_counter(key).await?;
        let allowed = counter < self.config.max_requests;

        let counter = if allowed {
            self.increment_window_counter(key).await?
        } else {
            counter
        };

        // Calculate time until next window
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();

        let window_end = self.current_window() + self.config.window.as_secs();
        let reset_after = Duration::from_secs(window_end.saturating_sub(now));

        Ok(RateLimitStatus {
            allowed,
            remaining: self.config.max_requests.saturating_sub(counter),
            limit: self.config.max_requests,
            reset_after,
            details: None,
        })
    }

    async fn reset(&self, key: &str) -> Result<()> {
        let window = self.current_window();
        let counter_key = format!("{}:{}:{}", key, "window", window);

        // Delete the counter key
        self.storage.delete(&counter_key).await?;

        Ok(())
    }
}
