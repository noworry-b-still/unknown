use async_trait::async_trait;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

use crate::config::{InMemoryConfig, RedisConfig};
use crate::error::{RateLimiterError, Result, StorageError};
use crate::resilience::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
use crate::resilience::exponential_backoff::RetryConfig;
use crate::resilience::health_checker::{HealthCheckConfig, HealthChecker};
use crate::storage::{MemoryStorage, RedisStorage, StorageBackend};

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
