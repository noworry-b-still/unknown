#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Barrier;
    use tokio::time;

    use crate::config::InMemoryConfig;
    use crate::error::{RateLimiterError, StorageError};
    use crate::storage::{MemoryStorage, StorageBackend};
    use crate::StoragePipeline as _;

    use super::super::common;

    // Helper function to create a MemoryStorage instance for testing
    fn create_test_memory() -> MemoryStorage {
        let config = InMemoryConfig {
            max_entries: 1000,
            use_background_task: true,
            cleanup_interval: Duration::from_secs(1),
        };

        MemoryStorage::new(config)
    }

    #[test]
    fn test_minimal_memory_operation() {
        // Create memory with explicit large capacity
        let config = InMemoryConfig {
            max_entries: 100, // Explicitly set to a reasonable size
            use_background_task: false,
            cleanup_interval: Duration::from_secs(1),
        };
        let memory = MemoryStorage::new(config);

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            // Try a single set operation
            let key = "test_single_key";
            let value = b"test_value";

            // Print the data length before setting
            println!("About to set key: {}", key);

            // Set with error handling
            match memory.set(key, value, None).await {
                Ok(_) => println!("Set operation succeeded"),
                Err(e) => println!("Set operation failed: {:?}", e),
            }
        });
    }

    // Test memory storage initialization
    #[test]
    fn test_memory_initialization() {
        // Create fresh memory instances for testing
        // the below 2 lines doesn't work because serde default function
        // is when we deserialize the struct, not when we initialize it.

        // let default_config = InMemoryConfig::default();
        // let large_memory = MemoryStorage::new(default_config);

        let large_memory = MemoryStorage::new(InMemoryConfig {
            max_entries: 1000, // Large capacity
            use_background_task: false,
            cleanup_interval: Duration::from_secs(1),
        });

        let small_memory = MemoryStorage::new(InMemoryConfig {
            max_entries: 3, // Very limited capacity
            use_background_task: false,
            cleanup_interval: Duration::from_secs(1),
        });

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            // 1. Test basic operations on large-capacity memory
            let key = "test_key";
            let value = b"test_value";

            // Verify key doesn't exist initially
            let get_result = large_memory.get(key).await;
            assert!(get_result.is_ok(), "Get should succeed");
            assert_eq!(get_result.unwrap(), None, "Key shouldn't exist initially");

            // Set should succeed
            let set_result = large_memory.set(key, value, None).await;
            assert!(set_result.is_ok(), "Set should succeed");

            // Get should return the value now
            let get_result = large_memory.get(key).await;
            assert!(get_result.is_ok(), "Get should succeed");
            assert_eq!(
                get_result.unwrap(),
                Some(value.to_vec()),
                "Should return the stored value"
            );

            // 2. Test capacity limits on small memory

            // Add entries up to capacity
            for i in 0..3 {
                let key = format!("small_key_{}", i);
                let value = format!("value_{}", i);

                let result = small_memory.set(&key, value.as_bytes(), None).await;
                assert!(result.is_ok(), "Setting up to capacity should succeed");
            }

            // Try to add one more (should fail)
            let overflow_result = small_memory.set("overflow_key", b"overflow", None).await;
            assert!(
                overflow_result.is_err(),
                "Should fail when exceeding capacity"
            );

            if let Err(RateLimiterError::Storage(StorageError::RedisCommand(msg))) = overflow_result
            {
                assert!(
                    msg.contains("Maximum entries"),
                    "Error should mention capacity limit"
                );
            } else {
                panic!("Expected RedisCommand error");
            }

            // 3. Test expiration

            // Set with short TTL
            let expire_result = large_memory
                .set("expire_key", b"expiring", Some(Duration::from_millis(50)))
                .await;
            assert!(expire_result.is_ok(), "Set with TTL should succeed");

            // Key should exist initially
            let exists_result = large_memory.exists("expire_key").await;
            assert!(exists_result.is_ok(), "Exists check should succeed");
            assert!(exists_result.unwrap(), "Key should exist after setting");

            // Wait for expiration
            tokio::time::sleep(Duration::from_millis(100)).await;

            // Key should be gone
            let exists_after_result = large_memory.exists("expire_key").await;
            assert!(exists_after_result.is_ok(), "Exists check should succeed");
            assert!(
                !exists_after_result.unwrap(),
                "Key should be gone after expiration"
            );

            // 4. Clean up
            let _ = large_memory.delete(key).await;
            let _ = large_memory.delete("expire_key").await;

            for i in 0..3 {
                let _ = small_memory.delete(&format!("small_key_{}", i)).await;
            }
        });
    }

    // Test basic memory storage operations
    #[tokio::test]
    async fn test_memory_basic_operations() {
        let memory = create_test_memory();

        // Use common test utility with timeout
        let result = time::timeout(
            Duration::from_secs(5),
            common::test_basic_operations(&memory),
        )
        .await;

        assert!(result.is_ok(), "Basic operations timed out");
        assert!(result.unwrap().is_ok(), "Basic memory operations failed");
    }

    // Test memory storage key expiration
    #[tokio::test]
    async fn test_memory_key_expiration() {
        let memory = create_test_memory();

        // Memory storage can handle millisecond-level expiration
        let result =
            time::timeout(Duration::from_secs(5), common::test_key_expiration(&memory)).await;

        assert!(result.is_ok(), "Key expiration test timed out");
        assert!(result.unwrap().is_ok(), "Memory key expiration failed");
    }

    // Test memory storage pipeline operations
    #[tokio::test]
    async fn test_memory_pipeline_operations() {
        let memory = create_test_memory();

        // Use common test utility with timeout
        let result = time::timeout(
            Duration::from_secs(5),
            common::test_pipeline_operations(&memory),
        )
        .await;

        assert!(result.is_ok(), "Pipeline operations timed out");
        assert!(result.unwrap().is_ok(), "Memory pipeline operations failed");
    }

    // Test background cleanup task
    #[tokio::test]
    async fn test_memory_background_cleanup() {
        // Create memory storage with very short cleanup interval
        let config = InMemoryConfig {
            max_entries: 100,
            use_background_task: true,
            cleanup_interval: Duration::from_millis(100), // Very short for testing
        };

        let memory = MemoryStorage::new(config);

        // Set keys with short TTL
        let test_keys = ["cleanup_key1", "cleanup_key2", "cleanup_key3"];

        for key in &test_keys {
            memory
                .set(key, b"test_value", Some(Duration::from_millis(50)))
                .await
                .unwrap();
        }

        // Verify keys exist initially
        for key in &test_keys {
            assert!(memory.exists(key).await.unwrap());
        }

        // Wait for expiration and cleanup with timeout
        let wait_result = time::timeout(
            Duration::from_secs(1), // Generous timeout
            time::sleep(Duration::from_millis(200)),
        )
        .await;
        assert!(wait_result.is_ok(), "Wait for cleanup timed out");

        // Keys should be gone (expired and cleaned up)
        for key in &test_keys {
            assert!(
                !memory.exists(key).await.unwrap(),
                "Key '{}' should be gone after cleanup",
                key
            );
        }
    }

    // Test memory limits
    #[tokio::test]
    async fn test_memory_limits() {
        // Create memory storage with small capacity
        let config = InMemoryConfig {
            max_entries: 5, // Only allow 5 entries
            use_background_task: false,
            cleanup_interval: Duration::from_secs(1),
        };

        let memory = MemoryStorage::new(config);

        // Fill to capacity
        for i in 0..5 {
            let key = format!("limit_key_{}", i);
            let value = format!("value_{}", i);

            let result = time::timeout(
                Duration::from_millis(500),
                memory.set(&key, value.as_bytes(), None),
            )
            .await;

            assert!(result.is_ok(), "Set operation timed out");
            assert!(result.unwrap().is_ok(), "Set operation failed");
        }

        // Try to add one more (should fail)
        let result = time::timeout(
            Duration::from_millis(500),
            memory.set("overflow_key", b"overflow", None),
        )
        .await;

        assert!(result.is_ok(), "Set operation timed out");
        let err = result.unwrap();
        assert!(
            matches!(
                err,
                Err(RateLimiterError::Storage(StorageError::RedisCommand(_)))
            ),
            "Should fail when exceeding capacity"
        );

        // Should be able to update existing key
        let result = time::timeout(
            Duration::from_millis(500),
            memory.set("limit_key_0", b"updated", None),
        )
        .await;

        assert!(result.is_ok(), "Update operation timed out");
        assert!(
            result.unwrap().is_ok(),
            "Should be able to update existing key"
        );

        // Should be able to add after deleting one
        let delete_result =
            time::timeout(Duration::from_millis(500), memory.delete("limit_key_1")).await;

        assert!(delete_result.is_ok(), "Delete operation timed out");
        assert!(delete_result.unwrap().unwrap(), "Delete operation failed");

        let add_result = time::timeout(
            Duration::from_millis(500),
            memory.set("new_key", b"new_value", None),
        )
        .await;

        assert!(add_result.is_ok(), "Add operation timed out");
        assert!(
            add_result.unwrap().is_ok(),
            "Should be able to add after deleting"
        );
    }

    // Test concurrent access patterns
    #[tokio::test]
    async fn test_memory_concurrent_access() {
        let memory = Arc::new(create_test_memory());
        let counter_key = "concurrent_test_counter";

        // Delete key in case it exists from a previous test
        let _ = memory.delete(counter_key).await;

        // Create a barrier to synchronize tasks
        let barrier = Arc::new(Barrier::new(10));

        // Spawn multiple tasks to increment the counter
        let mut handles = Vec::with_capacity(10);

        for _ in 0..10 {
            let memory_clone = Arc::clone(&memory);
            let barrier_clone = Arc::clone(&barrier);
            let key = counter_key.to_string();

            let handle = tokio::spawn(async move {
                // Wait for all tasks to be ready
                barrier_clone.wait().await;

                for _ in 0..10 {
                    let _ =
                        time::timeout(Duration::from_millis(100), memory_clone.increment(&key, 1))
                            .await;

                    // Small delay to encourage interleaving
                    time::sleep(Duration::from_millis(1)).await;
                }
            });

            handles.push(handle);
        }

        // Wait for all tasks to complete with a timeout
        let wait_result = time::timeout(Duration::from_secs(5), async {
            for handle in handles {
                handle.await.unwrap();
            }
        })
        .await;

        assert!(wait_result.is_ok(), "Concurrent test timed out");

        // Check final counter value (should be 100 if atomic)
        let counter_result =
            time::timeout(Duration::from_millis(500), memory.increment(counter_key, 0)).await;

        assert!(counter_result.is_ok(), "Counter read timed out");
        assert_eq!(
            counter_result.unwrap().unwrap(),
            100,
            "Counter should be 100 after concurrent increments"
        );

        // Clean up
        let _ = memory.delete(counter_key).await;
    }

    // Test explicit check_expiry functionality
    #[tokio::test]
    async fn test_memory_check_expiry() {
        let memory = create_test_memory();
        let key = "check_expiry_test";

        // Set a key with short TTL
        let set_result = time::timeout(
            Duration::from_millis(500),
            memory.set(key, b"test_value", Some(Duration::from_millis(50))),
        )
        .await;

        assert!(set_result.is_ok(), "Set operation timed out");
        assert!(set_result.unwrap().is_ok(), "Set operation failed");

        // Verify it exists initially
        let exists_result = time::timeout(Duration::from_millis(500), memory.exists(key)).await;

        assert!(exists_result.is_ok(), "Exists check timed out");
        assert!(
            exists_result.unwrap().unwrap(),
            "Key should exist initially"
        );

        // Wait for expiration with timeout
        let wait_result = time::timeout(
            Duration::from_millis(500),
            time::sleep(Duration::from_millis(100)),
        )
        .await;

        assert!(wait_result.is_ok(), "Wait for expiration timed out");

        // Should be gone after expiry
        let gone_result = time::timeout(Duration::from_millis(500), memory.exists(key)).await;

        assert!(gone_result.is_ok(), "Exists check timed out");
        assert!(
            !gone_result.unwrap().unwrap(),
            "Key should be gone after expiration"
        );

        // Get should also return None for expired key
        let get_result = time::timeout(Duration::from_millis(500), memory.get(key)).await;

        assert!(get_result.is_ok(), "Get operation timed out");
        assert_eq!(
            get_result.unwrap().unwrap(),
            None,
            "Get should return None for expired key"
        );
    }

    // Test memory storage with large values
    #[tokio::test]
    async fn test_memory_large_values() {
        let memory = create_test_memory();
        let key = "large_value_test";

        // Create a large value (~100KB)
        let large_value = vec![42u8; 100 * 1024];

        // Set the large value with timeout
        let set_result =
            time::timeout(Duration::from_secs(1), memory.set(key, &large_value, None)).await;

        assert!(set_result.is_ok(), "Set operation timed out");
        assert!(set_result.unwrap().is_ok(), "Set operation failed");

        // Retrieve and verify with timeout
        let get_result = time::timeout(Duration::from_secs(1), memory.get(key)).await;

        assert!(get_result.is_ok(), "Get operation timed out");
        assert_eq!(
            get_result.unwrap().unwrap().as_deref(),
            Some(large_value.as_slice()),
            "Retrieved value doesn't match stored value"
        );

        // Clean up
        let _ = memory.delete(key).await;
    }

    // Test large batch operations
    #[tokio::test]
    async fn test_memory_large_pipeline() {
        let memory = create_test_memory();

        // Create a large pipeline with many operations
        let mut pipeline = memory.pipeline();

        let prefix = "batch_mem_";
        for i in 0..100 {
            let key = format!("{}{}", prefix, i);
            let value = format!("value_{}", i);
            pipeline.set(&key, value.as_bytes(), None);
        }

        // Execute the pipeline with timeout
        let pipeline_result =
            time::timeout(Duration::from_secs(2), memory.execute_pipeline(pipeline)).await;

        assert!(pipeline_result.is_ok(), "Pipeline execution timed out");
        assert!(
            pipeline_result.unwrap().is_ok(),
            "Pipeline execution failed"
        );

        // Verify a few random keys with timeout
        for i in [0, 25, 50, 99].iter() {
            let key = format!("{}{}", prefix, i);
            let expected = format!("value_{}", i);

            let value_result = time::timeout(Duration::from_millis(500), memory.get(&key)).await;

            assert!(value_result.is_ok(), "Get operation timed out");
            let value = value_result.unwrap().unwrap();

            assert_eq!(
                String::from_utf8(value.unwrap()).unwrap(),
                expected,
                "Retrieved value doesn't match expected for key {}",
                key
            );
        }

        // Clean up with timeout
        let cleanup_result = time::timeout(Duration::from_secs(2), async {
            for i in 0..100 {
                let key = format!("{}{}", prefix, i);
                let _ = memory.delete(&key).await;
            }
        })
        .await;

        assert!(cleanup_result.is_ok(), "Cleanup timed out");
    }
}
