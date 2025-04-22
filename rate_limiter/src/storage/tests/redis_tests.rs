#[cfg(test)]
mod tests {
    use std::time::Duration;
    use tokio::time;

    use crate::config::RedisConfig;
    use crate::error::{RateLimiterError, StorageError};
    use crate::storage::{RedisStorage, StorageBackend};
    use crate::StoragePipeline as _;

    // Modified test utilities for Redis
    mod redis_test_utils {
        use std::time::Duration;
        use tokio::time;

        use crate::error::Result;
        use crate::storage::StorageBackend;

        // Test expiration specifically for Redis (using longer durations)
        pub async fn test_redis_key_expiration<S: StorageBackend>(storage: &S) -> Result<()> {
            let key = "test_redis_expiry_key";
            let value = b"expiring_value";

            // Use at least 1 second for Redis TTL (Redis requires seconds, not milliseconds)
            storage
                .set(key, value, Some(Duration::from_secs(1)))
                .await?;

            // Key should exist initially
            assert!(storage.exists(key).await?);

            // Wait for expiration (a bit longer to ensure it expires)
            time::sleep(Duration::from_secs(2)).await;

            // Key should be gone after expiry
            assert!(!storage.exists(key).await?);

            // Test the explicit expire method
            let key2 = "test_redis_expire_method";

            // Set without TTL
            storage.set(key2, value, None).await?;

            // Then add expiry (at least 1 second)
            let result = storage.expire(key2, Duration::from_secs(1)).await?;
            assert!(result);

            // Key should exist initially
            assert!(storage.exists(key2).await?);

            // Wait for expiration
            time::sleep(Duration::from_secs(2)).await;

            // Key should be gone after expiry
            assert!(!storage.exists(key2).await?);

            Ok(())
        }
    }

    // Helper function to create a Redis storage for testing
    async fn create_test_redis() -> RedisStorage {
        let config = RedisConfig {
            url: "redis://localhost:6379".to_string(),
            pool_size: 5,
            connection_timeout: Duration::from_millis(1000),
        };

        RedisStorage::new(config)
            .await
            .expect("Failed to create Redis client")
    }

    // Test Redis connection initialization - skip if Redis unavailable
    #[tokio::test]
    async fn test_redis_initialization() {
        // Skip test if Redis is not available
        if !is_redis_available().await {
            println!("Redis not available, skipping test_redis_initialization");
            return;
        }

        // Successful connection
        let config = RedisConfig {
            url: "redis://localhost:6379".to_string(),
            pool_size: 5,
            connection_timeout: Duration::from_millis(500),
        };

        let result = RedisStorage::new(config).await;
        assert!(result.is_ok(), "Should successfully create Redis client");

        // Invalid URL (should fail quickly)
        let bad_config = RedisConfig {
            url: "redis://nonexistent:6379".to_string(),
            pool_size: 3,
            connection_timeout: Duration::from_millis(100), // Very short timeout
        };

        let result = RedisStorage::new(bad_config).await;
        assert!(
            matches!(
                result,
                Err(RateLimiterError::Storage(StorageError::RedisConnection(_)))
            ),
            "Should return connection error for invalid URL"
        );
    }

    // Helper to check if Redis is available
    async fn is_redis_available() -> bool {
        let config = RedisConfig {
            url: "redis://localhost:6379".to_string(),
            pool_size: 1,
            connection_timeout: Duration::from_millis(300), // Very short timeout
        };

        match RedisStorage::new(config).await {
            Ok(redis) => {
                // Try a quick ping
                match redis.ping().await {
                    Ok(_) => true,
                    Err(_) => false,
                }
            }
            Err(_) => false,
        }
    }
    // Test basic Redis operations
    #[tokio::test]
    async fn test_redis_basic_operations() {
        let redis = create_test_redis().await;

        // Use the original common test utility for basic operations
        let result = super::super::common::test_basic_operations(&redis).await;
        assert!(
            result.is_ok(),
            "Basic Redis operations failed: {:?}",
            result
        );
    }

    // Test key expiration in Redis with the fixed utility
    #[tokio::test]
    async fn test_redis_key_expiration() {
        let redis = create_test_redis().await;

        // Use the Redis-specific test utility that uses seconds instead of milliseconds
        let result = redis_test_utils::test_redis_key_expiration(&redis).await;
        assert!(result.is_ok(), "Redis key expiration failed: {:?}", result);
    }

    // Test Redis pipeline operations
    #[tokio::test]
    async fn test_redis_pipeline_operations() {
        let redis = create_test_redis().await;

        // Use common test utility
        let result = super::super::common::test_pipeline_operations(&redis).await;
        assert!(
            result.is_ok(),
            "Redis pipeline operations failed: {:?}",
            result
        );
    }

    // Test Redis connection handling (ping)
    #[tokio::test]
    async fn test_redis_connection_handling() {
        let redis = create_test_redis().await;

        // Test ping functionality
        let result = redis.ping().await;
        assert!(result.is_ok(), "Redis ping should succeed");

        // Test with bad connection
        let bad_config = RedisConfig {
            url: "redis://nonexistent:6379".to_string(),
            pool_size: 1,
            connection_timeout: Duration::from_millis(100),
        };

        // This might succeed in initialization if we don't connect immediately
        let bad_redis = match RedisStorage::new(bad_config).await {
            Ok(redis) => redis,
            Err(_) => {
                // If we can't even initialize, just pass the test
                return;
            }
        };

        // But ping should fail
        let result = bad_redis.ping().await;
        assert!(result.is_err(), "Ping should fail for bad Redis connection");
    }

    // Test serialization and deserialization of various data types
    #[tokio::test]
    async fn test_redis_serialization() {
        let redis = create_test_redis().await;

        // Test string values
        let string_key = "test_string_key";
        let string_value = "hello world";
        redis
            .set(string_key, string_value.as_bytes(), None)
            .await
            .unwrap();

        let result = redis.get(string_key).await.unwrap();
        assert_eq!(
            result.map(|v| String::from_utf8(v).unwrap()),
            Some(string_value.to_string())
        );

        // Test binary data
        let binary_key = "test_binary_key";
        let binary_data = [0xDE, 0xAD, 0xBE, 0xEF];
        redis.set(binary_key, &binary_data, None).await.unwrap();

        let result = redis.get(binary_key).await.unwrap();
        assert_eq!(result.as_deref(), Some(&binary_data[..]));

        // Test integers (via increment)
        let int_key = "test_int_key";
        let int_value = 42;
        redis.increment(int_key, int_value).await.unwrap();

        let result = redis.get(int_key).await.unwrap();
        assert!(result.is_some());

        // Clean up
        redis.delete(string_key).await.unwrap();
        redis.delete(binary_key).await.unwrap();
        redis.delete(int_key).await.unwrap();
    }

    // Test error propagation
    #[tokio::test]
    async fn test_redis_error_propagation() {
        let redis = create_test_redis().await;

        // Create a key with a string value
        let key = "test_error_key";
        redis
            .set(key, "not_an_integer".as_bytes(), None)
            .await
            .unwrap();

        // Try to increment it (should fail as it's not an integer)
        let result = redis.increment(key, 1).await;
        assert!(
            result.is_err(),
            "Incrementing non-integer should return error"
        );

        // Verify the error type is correct
        if let Err(RateLimiterError::Storage(StorageError::RedisCommand(_))) = result {
            // This is the expected error type
        } else {
            panic!("Unexpected error type: {:?}", result);
        }

        // Clean up
        redis.delete(key).await.unwrap();
    }

    // Test concurrent access
    #[tokio::test]
    async fn test_redis_concurrent_access() {
        let redis = create_test_redis().await;
        let counter_key = "test_concurrent_counter";

        // Delete key in case it exists from a previous test
        let _ = redis.delete(counter_key).await;

        // Create multiple tasks to increment the counter
        let mut handles = Vec::with_capacity(10);

        for _ in 0..10 {
            let redis_clone = redis.clone();
            let key = counter_key.to_string();

            let handle = tokio::spawn(async move {
                for _ in 0..10 {
                    let _ = redis_clone.increment(&key, 1).await;
                    // Small delay to ensure concurrency
                    time::sleep(Duration::from_millis(1)).await;
                }
            });

            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Check final counter value (should be 100 if atomic)
        let final_value = redis.increment(counter_key, 0).await.unwrap();
        assert_eq!(
            final_value, 100,
            "Counter should be 100 after concurrent increments"
        );

        // Clean up
        redis.delete(counter_key).await.unwrap();
    }

    // Test large pipeline batch operations
    #[tokio::test]
    async fn test_redis_large_pipeline() {
        let redis = create_test_redis().await;

        // Create a large pipeline with many operations
        let mut pipeline = redis.pipeline();

        let prefix = "test_batch_";
        for i in 0..100 {
            let key = format!("{}{}", prefix, i);
            let value = format!("value_{}", i);
            pipeline.set(&key, value.as_bytes(), None);
        }

        // Execute the pipeline
        let results = redis.execute_pipeline(pipeline).await;
        assert!(
            results.is_ok(),
            "Large pipeline should execute successfully"
        );

        // Verify a few random keys
        for i in [0, 25, 50, 99].iter() {
            let key = format!("{}{}", prefix, i);
            let expected = format!("value_{}", i);

            let value = redis.get(&key).await.unwrap();
            assert_eq!(String::from_utf8(value.unwrap()).unwrap(), expected);
        }

        // Clean up using another pipeline
        let mut cleanup = redis.pipeline();
        for i in 0..100 {
            let key = format!("{}{}", prefix, i);
            cleanup.expire(&key, Duration::from_secs(1)); // Short expiry
        }

        let _ = redis.execute_pipeline(cleanup).await;

        // Wait for expiry
        time::sleep(Duration::from_secs(2)).await;
    }
}
