// src/algorithms/mod.rs

pub mod token_bucket;

use super::error::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use std::time::Duration;

/// Status returned by rate limiting operations
#[derive(Debug, Clone)]
pub struct RateLimitStatus {
    /// Whether the request was allowed
    pub allowed: bool,

    /// Remaining allowed requests in the current window
    pub remaining: u64,

    /// Total capacity of the rate limiter
    pub limit: u64,

    /// When the rate limit will reset, in seconds
    pub reset_after: Duration,

    /// Optional details specific to the algorithm
    pub details: Option<String>,
}

/// Core trait that all rate limiting algorithms must implement
#[async_trait]
pub trait RateLimitAlgorithm: Send + Sync + Debug {
    /// The type of configuration this algorithm accepts
    type Config: Send + Sync;

    /// Creates a new instance of this algorithm with the given configuration
    fn new(config: Self::Config) -> Self;

    /// Checks if a request is allowed without recording it
    async fn is_allowed(&self, key: &str) -> Result<bool>;

    /// Records that a request was made
    async fn record_request(&self, key: &str) -> Result<()>;

    /// Combined operation to check if a request is allowed and record it if it is
    /// Returns information about whether the request was allowed and current limit state
    async fn check_and_record(&self, key: &str) -> Result<RateLimitStatus>;

    /// Reset the rate limit for a specific key
    async fn reset(&self, key: &str) -> Result<()>;
}
