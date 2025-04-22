// src/storage/tests/mod.rs

mod memory_tests;
mod redis_tests;

// Common utilities for storage tests
pub(crate) mod common {
    use std::time::Duration;
    use tokio::time;

    use crate::error::Result;
    use crate::storage::StorageBackend;
    use crate::StoragePipeline as _;

    // Test basic storage operations that should work on any backend
    pub async fn test_basic_operations<S: StorageBackend>(storage: &S) -> Result<()> {
        // Test set and get
        let key = "test_basic_key";
        let value: &[u8] = b"test_value";

        // Set a value
        storage.set(key, value, None).await?;

        // Get the value back
        let result = storage.get(key).await?;
        assert_eq!(result.as_deref(), Some(value));

        // Test increment
        let counter_key = "test_counter";

        // Initial increment
        let result = storage.increment(counter_key, 1).await?;
        assert_eq!(result, 1);

        // Increment again
        let result = storage.increment(counter_key, 3).await?;
        assert_eq!(result, 4);

        // Test exists on existing keys
        assert!(storage.exists(key).await?);
        assert!(storage.exists(counter_key).await?);

        // Test exists on non-existing key
        assert!(!storage.exists("non_existent_key").await?);

        // Test delete
        let result = storage.delete(key).await?;
        assert!(result);

        // Key should no longer exist
        assert!(!storage.exists(key).await?);

        // Cleanup
        let _ = storage.delete(counter_key).await;

        Ok(())
    }

    // Test expiration functionality that should work on any backend
    pub async fn test_key_expiration<S: StorageBackend>(storage: &S) -> Result<()> {
        let key = "test_expiry_key";
        let value = b"expiring_value";

        // Set with a short TTL (100ms)
        storage
            .set(key, value, Some(Duration::from_millis(100)))
            .await?;

        // Key should exist initially
        assert!(storage.exists(key).await?);

        // Wait for expiration
        time::sleep(Duration::from_millis(150)).await;

        // Key should be gone after expiry
        assert!(!storage.exists(key).await?);

        // Test the explicit expire method
        let key2 = "test_expire_method";

        // Set without TTL
        storage.set(key2, value, None).await?;

        // Then add expiry
        let result = storage.expire(key2, Duration::from_millis(100)).await?;
        assert!(result);

        // Key should exist initially
        assert!(storage.exists(key2).await?);

        // Wait for expiration
        time::sleep(Duration::from_millis(150)).await;

        // Key should be gone after expiry
        assert!(!storage.exists(key2).await?);

        Ok(())
    }

    // Test pipeline operations that should work on any backend
    pub async fn test_pipeline_operations<S: StorageBackend>(storage: &S) -> Result<()> {
        // Create a pipeline with multiple operations
        let mut pipeline = storage.pipeline();

        // Add various operations
        pipeline
            .set("pipe_key1", b"value1", None)
            .set("pipe_key2", b"value2", None)
            .get("pipe_key1")
            .increment("pipe_counter", 5)
            .expire("pipe_key1", Duration::from_secs(60));

        // Execute the pipeline
        let results = storage.execute_pipeline(pipeline).await?;

        // Verify the pipeline executed successfully
        assert_eq!(results.len(), 5);

        // Check that the keys were actually set
        assert!(storage.exists("pipe_key1").await?);
        assert!(storage.exists("pipe_key2").await?);

        // Check the increment result
        let counter_val = storage.get("pipe_counter").await?;
        assert!(counter_val.is_some());

        if let Some(bytes) = counter_val {
            if bytes.len() == 8 {
                let mut arr = [0; 8];
                arr.copy_from_slice(&bytes);
                assert_eq!(i64::from_be_bytes(arr), 5);
            }
        }

        // Cleanup
        storage.delete("pipe_key1").await?;
        storage.delete("pipe_key2").await?;
        storage.delete("pipe_counter").await?;

        Ok(())
    }
}
