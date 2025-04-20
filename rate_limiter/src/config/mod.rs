// src/config/mod.rs

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Common configuration for all rate limiters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimiterConfig {
    /// Key prefix to use for all keys in storage
    #[serde(default = "default_key_prefix")]
    pub key_prefix: String,

    /// Default timeout for rate limiting operations
    #[serde(default = "default_timeout", with = "duration_serde")]
    pub timeout: Duration,
}

fn default_key_prefix() -> String {
    "ratelimit".to_string()
}

fn default_timeout() -> Duration {
    Duration::from_secs(5)
}

/// Configuration for token bucket algorithm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBucketConfig {
    /// Capacity of the token bucket
    pub capacity: u64,

    /// Rate at which tokens are refilled (tokens per second)
    pub refill_rate: f64,

    /// Initial token count
    #[serde(default = "full_capacity")]
    pub initial_tokens: Option<u64>,
}

fn full_capacity() -> Option<u64> {
    None
}

/// Configuration for fixed window algorithm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixedWindowConfig {
    /// Maximum number of requests allowed in the window
    pub max_requests: u64,

    /// Window duration
    #[serde(with = "duration_serde")]
    pub window: Duration,
}

/// Configuration for sliding window algorithm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlidingWindowConfig {
    /// Maximum number of requests allowed in the window
    pub max_requests: u64,

    /// Window duration
    #[serde(with = "duration_serde")]
    pub window: Duration,

    /// Precision (number of smaller buckets to divide the window into)
    #[serde(default = "default_precision")]
    pub precision: u32,
}

fn default_precision() -> u32 {
    10
}

/// Configuration for Redis storage backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    /// Redis connection URL
    pub url: String,

    /// Connection pool size
    #[serde(default = "default_pool_size")]
    pub pool_size: u32,

    /// Connection timeout
    #[serde(default = "default_conn_timeout", with = "duration_serde")]
    pub connection_timeout: Duration,
}

fn default_pool_size() -> u32 {
    10
}

fn default_conn_timeout() -> Duration {
    Duration::from_secs(2)
}

/// Configuration for in-memory storage backend
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InMemoryConfig {
    /// Maximum number of entries to store
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,

    /// Whether to use a background task for expiration
    #[serde(default = "default_use_background_task")]
    pub use_background_task: bool,

    /// How often to run the background expiration task
    #[serde(default = "default_cleanup_interval", with = "duration_serde")]
    pub cleanup_interval: Duration,
}

fn default_max_entries() -> usize {
    10_000
}

fn default_use_background_task() -> bool {
    true
}

fn default_cleanup_interval() -> Duration {
    Duration::from_secs(60)
}

// Helper module to serialize/deserialize Duration with serde
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_millis() as u64)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}
