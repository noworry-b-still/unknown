// src/resilience/mod.rs
//! Resilience features for the distributed rate limiting service.
//!
//! This module provides several resilience patterns to make the rate limiting service
//! more robust in distributed environments:
//!
//! 1. **Health Checks** - Actively monitor Redis connectivity to detect problems early
//! 2. **Circuit Breaking** - Prevent cascading failures when Redis is down
//! 3. **Retry with Exponential Backoff** - Smart retries for transient failures
//! 4. **Fallback Mechanisms** - Graceful degradation to in-memory storage

mod circuit_breaker;
mod exponential_backoff;
mod health_checker;
mod resilient_storage;

#[cfg(test)]
mod tests;

// Re-export key components
pub use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
pub use exponential_backoff::{ExponentialBackoff, RetryConfig};
pub use health_checker::{HealthCheckConfig, HealthChecker};
pub use resilient_storage::{ResilienceConfig, ResilientStorage};

