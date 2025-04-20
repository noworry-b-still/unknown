# Resilience Features for Distributed Rate Limiting

This guide explains the resilience features we've implemented for the distributed rate limiting service and how to use them in your applications.

## Overview

Distributed systems face unique challenges, especially when it comes to managing external dependencies like Redis. Our rate limiting service implements four key resilience patterns:

1. **Health Checks** - Actively monitor Redis connectivity
2. **Circuit Breaking** - Prevent cascading failures 
3. **Retry with Exponential Backoff** - Smart retries for transient issues
4. **Fallback Mechanisms** - Graceful degradation to in-memory storage

## How to Use Resilient Storage

The `ResilientStorage` combines all these patterns into a single component:

```rust
// Create Redis configuration
let redis_config = RedisConfig {
    url: "redis://localhost:6379".to_string(),
    pool_size: 10,
    connection_timeout: Duration::from_secs(1),
};

// Create resilience configuration (or use defaults)
let resilience_config = ResilienceConfig::default();

// Create the resilient storage
let storage = ResilientStorage::new(redis_config, resilience_config).await?;

// Use it with any rate limiting algorithm
let token_bucket = TokenBucket::new(storage.clone(), token_bucket_config);
let rate_limiter = RateLimiter::new(token_bucket, storage, "example".to_string());
```

## Resilience Features in Detail

### 1. Health Checks

Health checks actively monitor Redis connectivity to detect problems early:

- A background task periodically pings Redis to verify connectivity
- Health status is used to make intelligent routing decisions
- When Redis is unhealthy, requests automatically route to fallback storage

Configuration options:
```rust
HealthCheckConfig {
    check_interval: Duration::from_secs(5),  // How often to check
    check_timeout: Duration::from_millis(500), // Max time for a health check
}
```

### 2. Circuit Breaking

The circuit breaker prevents cascading failures when Redis is down:

- **Closed State**: Requests flow normally to Redis
- **Open State**: When too many failures occur, the circuit "opens" and fails fast
- **Half-Open State**: After a timeout, allows test requests to verify recovery

This pattern:
- Prevents overwhelming Redis with requests it can't handle
- Reduces latency by failing fast when the system is known to be down
- Automatically recovers when the underlying issue is resolved

Configuration options:
```rust
CircuitBreakerConfig {
    failure_threshold: 3,  // Number of failures before opening
    reset_timeout: Duration::from_secs(10),  // Time before testing recovery
    success_threshold: 2,  // Successes needed to close the circuit
}
```

### 3. Retry with Exponential Backoff

Smart retry logic handles transient failures with exponential backoff:

- Automatically retries failed operations up to a configured limit
- Increases delay between retries exponentially
- Optional jitter prevents "thundering herd" problems

Configuration options:
```rust
RetryConfig {
    max_attempts: 3,  // Maximum number of attempts
    initial_backoff: Duration::from_millis(100),  // First retry delay
    max_backoff: Duration::from_secs(1),  // Maximum delay cap
    backoff_multiplier: 2.0,  // How quickly backoff increases
    use_jitter: true,  // Add randomness to prevent synchronized retries
}
```

### 4. Fallback Mechanisms

When Redis is unavailable, operations gracefully degrade to in-memory storage:

- Automatically routes requests to in-memory storage when Redis fails
- Preserves basic functionality even during Redis outages
- Trade-off: loses distribution guarantees during fallback operation

Configuration options:
```rust
InMemoryConfig {
    max_entries: 10_000,  // Maximum number of keys to store
    use_background_task: true,  // Clean up expired entries
    cleanup_interval: Duration::from_secs(60),  // How often to clean up
}
```

## Advanced Usage Patterns

### Manual Circuit Breaking

You can access the circuit breaker directly for more control:

```rust
let circuit_breaker = resilient_storage.circuit_breaker();

// Check if requests should be allowed
if circuit_breaker.allow_request().await {
    // Perform operation
    match perform_operation().await {
        Ok(result) => {
            circuit_breaker.record_success().await;
            // Handle success
        }
        Err(e) => {
            circuit_breaker.record_failure().await;
            // Handle error
        }
    }
} else {
    // Circuit is open, fail fast
}
```

### Health Status Monitoring

You can check the health status for monitoring:

```rust
let health_checker = resilient_storage.health_checker();

if health_checker.is_healthy() {
    println!("Redis is connected and healthy");
} else {
    println!("Redis is currently unavailable");
}
```

## Best Practices

1. **Configure Appropriately for Your Workload**:
   - High-throughput services may need more aggressive circuit breaking
   - Critical services might need more retry attempts

2. **Monitor Health and Circuit State**:
   - Expose metrics about Redis health and circuit breaker state
   - Set up alerts when the service falls back to in-memory storage

3. **Testing Resilience**:
   - Regularly test how your application behaves during Redis outages
   - Use chaos engineering to verify resilience in production

4. **Recovery Planning**:
   - Understand the consistency implications of falling back to in-memory storage
   - Plan for re-synchronization when Redis recovers

## Conclusion

These resilience features make your rate limiting service robust in the face of infrastructure issues. By properly configuring the resilience options, you can ensure your service:

- Recovers gracefully from transient failures
- Degrades functionality rather than failing completely
- Protects itself and dependent services during outages
- Provides the best possible user experience even during partial system failures
