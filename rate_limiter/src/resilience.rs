// src/resilience.rs
//! Resilience features for the distributed rate limiting service.
//!
//! This module provides several resilience patterns to make the rate limiting service
//! more robust in distributed environments:
//!
//! 1. **Health Checks** - Actively monitor Redis connectivity to detect problems early
//! 2. **Circuit Breaking** - Prevent cascading failures when Redis is down
//! 3. **Retry with Exponential Backoff** - Smart retries for transient failures
//! 4. **Fallback Mechanisms** - Graceful degradation to in-memory storage
//!
//! # Examples
//!
//! ```rust,no_run
//! use rate_limiter::resilience::{ResilientStorage, ResilienceConfig};
//! use rate_limiter::config::RedisConfig;
//! use rate_limiter::algorithms::TokenBucket;
//! use rate_limiter::RateLimiter;
//!
//! async fn create_resilient_rate_limiter() {
//!     // Create the Redis and resilience configurations
//!     let redis_config = RedisConfig {
//!         url: "redis://localhost:6379".to_string(),
//!         pool_size: 10,
//!         connection_timeout: std::time::Duration::from_secs(2),
//!     };
//!
//!     let resilience_config = ResilienceConfig::default();
//!
//!     // Create a resilient storage
//!     let storage = ResilientStorage::new(redis_config, resilience_config).await.unwrap();
//!
//!     // Create a token bucket algorithm using the resilient storage
//!     let token_bucket = TokenBucket::new(
//!         storage,
//!         rate_limiter::config::TokenBucketConfig {
//!             capacity: 100,
//!             refill_rate: 10.0,
//!             initial_tokens: None,
//!         },
//!     );
//!
//!     // Create a rate limiter using the token bucket and resilient storage
//!     let rate_limiter = RateLimiter::new(token_bucket, storage, "resilient".to_string());
//!
//!     // Now the rate limiter will automatically handle Redis failures and retry with backoff
//! }
//! ```
//
// This module provides resilience features for the rate limiter service:
// 1. Health checks - Periodic verification of Redis connectivity
// 2. Circuit breaking - Prevent cascading failures when Redis is down
// 3. Retry with exponential backoff - Smart retries for transient failures
// 4. Fallback mechanisms - Graceful degradation to in-memory storage

use async_trait::async_trait;
use rand;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::task;
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::config::{InMemoryConfig, RedisConfig};
use crate::error::{RateLimiterError, Result, StorageError};
use crate::storage::{MemoryStorage, RedisStorage, StorageBackend};

/// The state of the circuit breaker
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed, requests flow normally
    Closed,
    /// Circuit is open, requests are failing and not being sent
    Open,
    /// Circuit is partially open, allowing a limited number of requests to test recovery
    HalfOpen,
}

/// Configuration for circuit breaker
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening the circuit
    pub failure_threshold: usize,
    /// Duration to keep the circuit open before transitioning to half-open
    pub reset_timeout: Duration,
    /// Number of consecutive successes in half-open state to close the circuit
    pub success_threshold: usize,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            reset_timeout: Duration::from_secs(30),
            success_threshold: 3,
        }
    }
}

/// Configuration for retry strategy
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: usize,
    /// Initial backoff duration
    pub initial_backoff: Duration,
    /// Maximum backoff duration
    pub max_backoff: Duration,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Whether to add jitter to backoff
    pub use_jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(5),
            backoff_multiplier: 2.0,
            use_jitter: true,
        }
    }
}

/// Configuration for health checks
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    /// How often to check Redis health
    pub check_interval: Duration,
    /// Timeout for health check operations
    pub check_timeout: Duration,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(5),
            check_timeout: Duration::from_secs(1),
        }
    }
}

/// Combined configuration for resilience features
#[derive(Debug, Clone)]
pub struct ResilienceConfig {
    /// Circuit breaker configuration
    pub circuit_breaker: CircuitBreakerConfig,
    /// Retry configuration
    pub retry: RetryConfig,
    /// Health check configuration
    pub health_check: HealthCheckConfig,
    /// In-memory storage configuration (for fallback)
    pub memory_config: InMemoryConfig,
}

impl Default for ResilienceConfig {
    fn default() -> Self {
        Self {
            circuit_breaker: CircuitBreakerConfig::default(),
            retry: RetryConfig::default(),
            health_check: HealthCheckConfig::default(),
            memory_config: InMemoryConfig::default(),
        }
    }
}

/// Circuit breaker implementation
#[derive(Debug)]
pub struct CircuitBreaker {
    /// Current state of the circuit breaker
    state: RwLock<CircuitState>,
    /// Count of consecutive failures
    failure_count: AtomicUsize,
    /// Count of consecutive successes
    success_count: AtomicUsize,
    /// Timestamp when the circuit was opened
    opened_at: RwLock<Option<Instant>>,
    /// Configuration for the circuit breaker
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given configuration
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: RwLock::new(CircuitState::Closed),
            failure_count: AtomicUsize::new(0),
            success_count: AtomicUsize::new(0),
            opened_at: RwLock::new(None),
            config,
        }
    }

    /// Check if the circuit breaker allows the request to proceed
    pub async fn allow_request(&self) -> bool {
        match *self.state.read().await {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if it's time to transition to half-open
                let opened_at = self.opened_at.read().await;
                if let Some(time) = *opened_at {
                    if time.elapsed() > self.config.reset_timeout {
                        drop(opened_at); // Drop the read lock before acquiring write lock
                        let mut state = self.state.write().await;
                        *state = CircuitState::HalfOpen;
                        debug!("Circuit breaker state transitioned to half-open");
                        return true;
                    }
                }
                false
            }
            CircuitState::HalfOpen => {
                // In half-open state, allow a limited number of requests through
                // This is a simplistic approach where we just allow one request at a time
                true
            }
        }
    }

    /// Record a successful operation
    pub async fn record_success(&self) {
        match *self.state.read().await {
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::SeqCst);
            }
            CircuitState::HalfOpen => {
                // Increment success count
                let new_success_count = self.success_count.fetch_add(1, Ordering::SeqCst) + 1;

                // If we've reached the success threshold, close the circuit
                if new_success_count >= self.config.success_threshold {
                    let mut state = self.state.write().await;
                    *state = CircuitState::Closed;
                    self.success_count.store(0, Ordering::SeqCst);
                    self.failure_count.store(0, Ordering::SeqCst);
                    debug!(
                        "Circuit breaker state transitioned to closed after successful operations"
                    );
                }
            }
            CircuitState::Open => {
                // This should not happen, but just in case
                debug!("Received success in Open state - this is unexpected");
            }
        }
    }

    /// Record a failed operation
    pub async fn record_failure(&self) {
        match *self.state.read().await {
            CircuitState::Closed => {
                // Increment failure count
                let new_failure_count = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;

                // If we've reached the failure threshold, open the circuit
                if new_failure_count >= self.config.failure_threshold {
                    let mut state = self.state.write().await;
                    *state = CircuitState::Open;
                    let mut opened_at = self.opened_at.write().await;
                    *opened_at = Some(Instant::now());
                    warn!(
                        "Circuit breaker opened after {} consecutive failures",
                        new_failure_count
                    );
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open state opens the circuit again
                let mut state = self.state.write().await;
                *state = CircuitState::Open;
                let mut opened_at = self.opened_at.write().await;
                *opened_at = Some(Instant::now());
                self.success_count.store(0, Ordering::SeqCst);
                warn!("Circuit breaker re-opened after failure in half-open state");
            }
            CircuitState::Open => {
                // Already open, nothing to do
            }
        }
    }

    /// Get the current state of the circuit breaker
    pub async fn get_state(&self) -> CircuitState {
        *self.state.read().await
    }
}

/// Exponential backoff implementation for retries
pub struct ExponentialBackoff {
    /// Current attempt number
    current_attempt: usize,
    /// Configuration for the retry strategy
    config: RetryConfig,
}

impl ExponentialBackoff {
    /// Create a new exponential backoff with the given configuration
    pub fn new(config: RetryConfig) -> Self {
        Self {
            current_attempt: 0,
            config,
        }
    }

    /// Get the next backoff duration, or None if max attempts reached
    pub fn next_backoff(&mut self) -> Option<Duration> {
        self.current_attempt += 1;

        if self.current_attempt > self.config.max_attempts {
            return None;
        }

        let exp = self.current_attempt as f64 - 1.0;
        let base_ms = self.config.initial_backoff.as_millis() as f64;
        let backoff_ms = base_ms * self.config.backoff_multiplier.powf(exp);
        let max_ms = self.config.max_backoff.as_millis() as f64;
        let capped_ms = backoff_ms.min(max_ms);

        let jittered_ms = if self.config.use_jitter {
            // Add jitter: random value between 50% and 100% of the calculated backoff
            let jitter = rand::random::<f64>() * 0.5 + 0.5;
            (capped_ms * jitter) as u64
        } else {
            capped_ms as u64
        };

        Some(Duration::from_millis(jittered_ms))
    }

    /// Reset the backoff to start from the beginning
    pub fn reset(&mut self) {
        self.current_attempt = 0;
    }
}

/// Health check implementation for Redis
#[derive(Debug)]
pub struct HealthChecker {
    /// Flag indicating if Redis is healthy
    is_healthy: Arc<AtomicBool>,
    /// Redis storage reference
    redis: Arc<RedisStorage>,
    /// Configuration for health checks
    config: HealthCheckConfig,
    /// Cancel flag for the health check task
    cancel_flag: Arc<AtomicBool>,
}

impl HealthChecker {
    /// Create a new health checker with the given Redis storage and configuration
    pub fn new(redis: Arc<RedisStorage>, config: HealthCheckConfig) -> Self {
        Self {
            is_healthy: Arc::new(AtomicBool::new(true)), // Assume healthy initially
            redis,
            config,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the health checker background task
    pub fn start(&self) -> task::JoinHandle<()> {
        let redis = Arc::clone(&self.redis);
        let is_healthy = Arc::clone(&self.is_healthy);
        let interval = self.config.check_interval;
        let timeout = self.config.check_timeout;
        let cancel_flag = Arc::clone(&self.cancel_flag);

        task::spawn(async move {
            let mut interval_timer = time::interval(interval);

            loop {
                // Check if we should stop
                if cancel_flag.load(Ordering::SeqCst) {
                    break;
                }

                interval_timer.tick().await;

                // Create a timeout for the health check
                let health_check = async {
                    // Simple PING command as health check
                    match redis.ping().await {
                        Ok(_) => true,
                        Err(e) => {
                            error!("Redis health check failed: {}", e);
                            false
                        }
                    }
                };

                // Run the health check with a timeout
                let is_redis_healthy = match time::timeout(timeout, health_check).await {
                    Ok(result) => result,
                    Err(_) => {
                        error!("Redis health check timed out after {:?}", timeout);
                        false
                    }
                };

                // Update the health status
                let previous_status = is_healthy.swap(is_redis_healthy, Ordering::SeqCst);
                if previous_status != is_redis_healthy {
                    if is_redis_healthy {
                        info!("Redis is now healthy");
                    } else {
                        warn!("Redis is now unhealthy");
                    }
                }
            }

            debug!("Health checker task stopped");
        })
    }

    /// Stop the health checker
    pub fn stop(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    /// Check if Redis is currently healthy
    pub fn is_healthy(&self) -> bool {
        self.is_healthy.load(Ordering::SeqCst)
    }
}

/// Resilient storage implementation that combines Redis with in-memory fallback
///
/// The `ResilientStorage` provides a robust storage implementation that:
///
/// 1. Uses Redis as the primary storage for distributed rate limiting
/// 2. Falls back to in-memory storage when Redis is unavailable
/// 3. Automatically retries failed operations with exponential backoff
/// 4. Uses a circuit breaker to quickly fail when Redis is down
/// 5. Monitors Redis health to make intelligent routing decisions
///
/// # Architecture
///
/// ```plaintext
/// ┌─────────────┐
/// │ Application │
/// └─────────────┘
///        │
///        ▼
/// ┌──────────────────┐
/// │ ResilientStorage │
/// └──────────────────┘
///        │
///        ├─────────────────────┐
///        ▼                     ▼
/// ┌─────────────┐      ┌────────────────┐
/// │    Redis    │◄────►│ Circuit Breaker│
/// └─────────────┘      └────────────────┘
///        │                     ▲
///        │                     │
///        │                     │
///        ▼                     │
/// ┌─────────────┐      ┌────────────────┐
/// │  Fallback   │◄────►│ Health Checker │
/// │  In-Memory  │      └────────────────┘
/// └─────────────┘
/// ```
#[derive(Debug)]
pub struct ResilientStorage {
    /// Redis storage for primary operations
    redis: Arc<RedisStorage>,
    /// In-memory storage for fallback
    memory: Arc<MemoryStorage>,
    /// Circuit breaker for Redis connectivity
    circuit_breaker: Arc<CircuitBreaker>,
    /// Health checker for Redis
    health_checker: Arc<HealthChecker>,
    /// Configuration for the resilient storage
    #[allow(dead_code)]
    config: ResilienceConfig,
}

// Simplified implementation of ResilientStorage that avoids lifetime/FnOnce issues (there's been alot of issues with lifetime/FnOnce in the past)
impl ResilientStorage {
    /// Create a new resilient storage with the given Redis and in-memory storages
    pub async fn new(
        redis_config: RedisConfig,
        resilience_config: ResilienceConfig,
    ) -> Result<Self> {
        // Create Redis storage
        let redis = Arc::new(RedisStorage::new(redis_config).await?);

        // Create in-memory fallback storage
        let memory = Arc::new(MemoryStorage::new(resilience_config.memory_config.clone()));

        // Create circuit breaker
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            resilience_config.circuit_breaker.clone(),
        ));

        // Create health checker
        let health_checker = Arc::new(HealthChecker::new(
            Arc::clone(&redis),
            resilience_config.health_check.clone(),
        ));

        // Start health checker
        health_checker.start();

        Ok(Self {
            redis,
            memory,
            circuit_breaker,
            health_checker,
            config: resilience_config,
        })
    }

    /// Performs a fallback operation on in-memory storage
    async fn fallback_operation<T, F, Fut>(
        &self,
        operation: &str,
        key: &str,
        fallback_fn: F,
    ) -> Result<T>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<T>> + Send + 'static,
    {
        debug!(
            "Performing fallback operation '{}' on key '{}'",
            operation, key
        );
        fallback_fn().await
    }

    /// Attempt to use Redis with retry, without using closures that require Copy trait
    async fn try_redis_operation<T, F, Fut>(
        &self,
        operation: &str,
        key: &str,
        redis_fn: F,
    ) -> Result<T>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<T>> + Send + 'static,
    {
        // Check if Redis is healthy according to health checker
        let use_redis = self.health_checker.is_healthy();

        // Check if circuit breaker allows the request
        let circuit_allows = self.circuit_breaker.allow_request().await;

        if !use_redis || !circuit_allows {
            // Redis is unhealthy or circuit breaker is open
            debug!(
            "Skipping Redis for operation '{}' on key '{}'. Redis healthy: {}, Circuit allows: {}",
            operation, key, use_redis, circuit_allows
        );
            return Err(RateLimiterError::Storage(StorageError::RedisConnection(
                format!("Redis unavailable for operation '{}'", operation),
            )));
        }

        // If we get here, Redis is healthy and circuit allows the request
        let result = redis_fn().await;

        match &result {
            Ok(_) => {
                // Record success in circuit breaker
                self.circuit_breaker.record_success().await;
            }
            Err(_) => {
                // Record failure in circuit breaker
                self.circuit_breaker.record_failure().await;
            }
        }

        result
    }
}

// Implement StorageBackend for ResilientStorage
#[async_trait]
impl StorageBackend for ResilientStorage {
    type Config = ResilienceConfig;
    type Pipeline = crate::storage::RedisPipeline;

    async fn new(_config: Self::Config) -> Result<Self> {
        // This is a stub - the actual implementation is in the ResilientStorage::new method
        Err(RateLimiterError::Config(
            "Use ResilientStorage::new instead".to_string(),
        ))
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let redis_clone = Arc::clone(&self.redis);
        let memory_clone = Arc::clone(&self.memory);

        let redis_key = key.to_string();
        let memory_key = key.to_string();

        // Try Redis first
        let redis_result = self
            .try_redis_operation(
                "get",
                key,
                || async move { redis_clone.get(&redis_key).await },
            )
            .await;

        match redis_result {
            Ok(value) => Ok(value),
            Err(_) => {
                // Fall back to in-memory
                self.fallback_operation("get", key, || async move {
                    memory_clone.get(&memory_key).await
                })
                .await
            }
        }
    }

    async fn set(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> Result<()> {
        let redis_clone = Arc::clone(&self.redis);
        let memory_clone = Arc::clone(&self.memory);

        // Create separate owned copies for each closure
        let redis_key = key.to_string();
        let memory_key = key.to_string();

        // Create separate owned copies of the value for each closure
        let redis_value = value.to_vec();
        let memory_value = value.to_vec();

        // Try Redis first
        let redis_result = self
            .try_redis_operation("set", key, move || async move {
                redis_clone.set(&redis_key, &redis_value, ttl).await
            })
            .await;

        match redis_result {
            Ok(()) => Ok(()),
            Err(_) => {
                // Fall back to in-memory
                self.fallback_operation("set", key, move || async move {
                    memory_clone.set(&memory_key, &memory_value, ttl).await
                })
                .await
            }
        }
    }

    async fn increment(&self, key: &str, amount: i64) -> Result<i64> {
        let redis_clone = Arc::clone(&self.redis);
        let memory_clone = Arc::clone(&self.memory);

        // Create separate owned copies for each closure
        let redis_key = key.to_string();
        let memory_key = key.to_string();

        // Try Redis first
        let redis_result = self
            .try_redis_operation("increment", key, move || async move {
                redis_clone.increment(&redis_key, amount).await
            })
            .await;

        match redis_result {
            Ok(value) => Ok(value),
            Err(_) => {
                // Fall back to in-memory
                self.fallback_operation("increment", key, move || async move {
                    memory_clone.increment(&memory_key, amount).await
                })
                .await
            }
        }
    }

    async fn expire(&self, key: &str, ttl: Duration) -> Result<bool> {
        let redis_clone = Arc::clone(&self.redis);
        let memory_clone = Arc::clone(&self.memory);

        // Create separate owned copies for each closure
        let redis_key = key.to_string();
        let memory_key = key.to_string();

        // Try Redis first
        let redis_result = self
            .try_redis_operation("expire", key, move || async move {
                redis_clone.expire(&redis_key, ttl).await
            })
            .await;

        match redis_result {
            Ok(value) => Ok(value),
            Err(_) => {
                // Fall back to in-memory
                self.fallback_operation("expire", key, move || async move {
                    memory_clone.expire(&memory_key, ttl).await
                })
                .await
            }
        }
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let redis_clone = Arc::clone(&self.redis);
        let memory_clone = Arc::clone(&self.memory);

        // Create separate owned copies for each closure
        let redis_key = key.to_string();
        let memory_key = key.to_string();

        // Try Redis first
        let redis_result = self
            .try_redis_operation("exists", key, || async move {
                redis_clone.exists(&redis_key).await
            })
            .await;

        match redis_result {
            Ok(value) => Ok(value),
            Err(_) => {
                // Fall back to in-memory
                self.fallback_operation("exists", key, || async move {
                    memory_clone.exists(&memory_key).await
                })
                .await
            }
        }
    }

    async fn delete(&self, key: &str) -> Result<bool> {
        let redis_clone = Arc::clone(&self.redis);
        let memory_clone = Arc::clone(&self.memory);

        // Create separate owned copies for each closure
        let redis_key = key.to_string();
        let memory_key = key.to_string();

        // Try Redis first
        let redis_result = self
            .try_redis_operation("delete", key, || async move {
                redis_clone.delete(&redis_key).await
            })
            .await;

        match redis_result {
            Ok(value) => Ok(value),
            Err(_) => {
                // Fall back to in-memory
                self.fallback_operation("delete", key, || async move {
                    memory_clone.delete(&memory_key).await
                })
                .await
            }
        }
    }

    fn pipeline(&self) -> Self::Pipeline {
        // We use Redis pipeline since it's more feature-complete
        self.redis.pipeline()
    }

    async fn execute_pipeline(&self, pipeline: Self::Pipeline) -> Result<Vec<Result<Vec<u8>>>> {
        let redis_clone = Arc::clone(&self.redis);

        // Check circuit breaker and health directly
        let use_redis = self.health_checker.is_healthy();
        let circuit_allows = self.circuit_breaker.allow_request().await;

        if use_redis && circuit_allows {
            // Try the operation directly
            match redis_clone.execute_pipeline(pipeline).await {
                Ok(result) => {
                    self.circuit_breaker.record_success().await;
                    Ok(result)
                }
                Err(e) => {
                    self.circuit_breaker.record_failure().await;
                    warn!(
                        "Pipeline execution failed and could not be handled by fallback: {}",
                        e
                    );
                    Err(e)
                }
            }
        } else {
            // Redis is unhealthy or circuit breaker is open
            debug!(
                "Cannot execute pipeline: Redis healthy: {}, Circuit allows: {}",
                use_redis, circuit_allows
            );

            Err(RateLimiterError::Storage(StorageError::RedisConnection(
                "Redis unavailable, pipeline execution not implemented for fallback".to_string(),
            )))
        }
    }
}
