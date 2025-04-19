// src/test_utils.rs

use super::algorithms::RateLimitAlgorithm;
use super::error::Result;
use super::storage::{StorageBackend, StoragePipeline};
use async_trait::async_trait;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Mock implementation of the StoragePipeline trait for testing
#[derive(Debug, Default)]
pub struct MockStoragePipeline {
    operations: Vec<MockOperation>,
}

#[derive(Debug)]
enum MockOperation {
    Get(String),
    Set(String, Vec<u8>, Option<Duration>),
    Increment(String, i64),
    Expire(String, Duration),
}

impl StoragePipeline for MockStoragePipeline {
    fn get(&mut self, key: &str) -> &mut Self {
        self.operations.push(MockOperation::Get(key.to_string()));
        self
    }

    fn set(&mut self, key: &str, value: &[u8], ttl: Option<Duration>) -> &mut Self {
        self.operations
            .push(MockOperation::Set(key.to_string(), value.to_vec(), ttl));
        self
    }

    fn increment(&mut self, key: &str, amount: i64) -> &mut Self {
        self.operations
            .push(MockOperation::Increment(key.to_string(), amount));
        self
    }

    fn expire(&mut self, key: &str, ttl: Duration) -> &mut Self {
        self.operations
            .push(MockOperation::Expire(key.to_string(), ttl));
        self
    }
}

/// Mock implementation of the StorageBackend trait for testing
#[derive(Debug)]
pub struct MockStorage {
    data: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    expiry: Arc<Mutex<HashMap<String, std::time::Instant>>>,
}

impl MockStorage {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
            expiry: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn check_expiry(&self, key: &str) -> bool {
        let expiry = self.expiry.lock().unwrap();
        if let Some(instant) = expiry.get(key) {
            if instant > &std::time::Instant::now() {
                true
            } else {
                // Key is expired, should be removed
                drop(expiry);
                let mut data = self.data.lock().unwrap();
                data.remove(key);
                let mut expiry = self.expiry.lock().unwrap();
                expiry.remove(key);
                false
            }
        } else {
            true // No expiry set
        }
    }
}

#[async_trait]
impl StorageBackend for MockStorage {
    type Config = ();
    type Pipeline = MockStoragePipeline;

    async fn new(_config: Self::Config) -> Result<Self> {
        Ok(Self::new())
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        if !self.check_expiry(key) {
            return Ok(None);
        }

        let data = self.data.lock().unwrap();
        Ok(data.get(key).cloned())
    }

    async fn set(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> Result<()> {
        let mut data = self.data.lock().unwrap();
        data.insert(key.to_string(), value.to_vec());

        if let Some(ttl) = ttl {
            let mut expiry = self.expiry.lock().unwrap();
            expiry.insert(key.to_string(), std::time::Instant::now() + ttl);
        }

        Ok(())
    }

    async fn increment(&self, key: &str, amount: i64) -> Result<i64> {
        if !self.check_expiry(key) {
            let mut data = self.data.lock().unwrap();
            data.insert(key.to_string(), amount.to_be_bytes().to_vec());
            return Ok(amount);
        }

        let mut data = self.data.lock().unwrap();
        let current = data.get(key).map_or(0, |bytes| {
            if bytes.len() == 8 {
                let mut arr = [0; 8];
                arr.copy_from_slice(bytes);
                i64::from_be_bytes(arr)
            } else {
                0
            }
        });

        let new_value = current + amount;
        data.insert(key.to_string(), new_value.to_be_bytes().to_vec());

        Ok(new_value)
    }

    async fn expire(&self, key: &str, ttl: Duration) -> Result<bool> {
        let exists = {
            let data = self.data.lock().unwrap();
            data.contains_key(key)
        };

        if exists {
            let mut expiry = self.expiry.lock().unwrap();
            expiry.insert(key.to_string(), std::time::Instant::now() + ttl);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        if !self.check_expiry(key) {
            return Ok(false);
        }

        let data = self.data.lock().unwrap();
        Ok(data.contains_key(key))
    }

    async fn delete(&self, key: &str) -> Result<bool> {
        let mut data = self.data.lock().unwrap();
        let existed = data.remove(key).is_some();

        let mut expiry = self.expiry.lock().unwrap();
        expiry.remove(key);

        Ok(existed)
    }

    fn pipeline(&self) -> Self::Pipeline {
        MockStoragePipeline::default()
    }

    async fn execute_pipeline(&self, pipeline: Self::Pipeline) -> Result<Vec<Result<Vec<u8>>>> {
        let mut results = Vec::new();

        for op in pipeline.operations {
            match op {
                MockOperation::Get(key) => {
                    let result = self.get(&key).await?;
                    results.push(Ok(result.unwrap_or_default()));
                }
                MockOperation::Set(key, value, ttl) => {
                    self.set(&key, &value, ttl).await?;
                    results.push(Ok(vec![]));
                }
                MockOperation::Increment(key, amount) => {
                    let result = self.increment(&key, amount).await?;
                    results.push(Ok(result.to_be_bytes().to_vec()));
                }
                MockOperation::Expire(key, ttl) => {
                    let result = self.expire(&key, ttl).await?;
                    results.push(Ok(vec![result as u8]));
                }
            }
        }

        Ok(results)
    }
}

/// Helper function to create a test rate limiter with a mock storage backend
pub async fn create_test_rate_limiter<A>(algorithm: A) -> crate::RateLimiter<A, MockStorage>
where
    A: RateLimitAlgorithm,
{
    let storage = MockStorage::new();
    crate::RateLimiter::new(algorithm, storage, "test".to_string())
}

/// Helper struct for testing rate limiters with controlled time
#[derive(Debug)]
pub struct TestClock {
    current_time: Arc<Mutex<std::time::Instant>>,
}

impl TestClock {
    pub fn new() -> Self {
        Self {
            current_time: Arc::new(Mutex::new(std::time::Instant::now())),
        }
    }

    pub fn advance(&self, duration: Duration) {
        let mut time = self.current_time.lock().unwrap();
        *time += duration;
    }

    pub fn now(&self) -> std::time::Instant {
        *self.current_time.lock().unwrap()
    }
}

/// Helper function to run rate limit tests with a specific pattern of requests
pub async fn test_rate_limit_scenario<A, F>(
    algorithm: A,
    request_count: usize,
    expected_allowed: usize,
    between_requests: Option<F>,
) -> bool
where
    A: RateLimitAlgorithm,
    F: Fn(usize) -> futures::future::BoxFuture<'static, ()>,
{
    let limiter = create_test_rate_limiter(algorithm).await;
    let key = "test_user";

    let mut allowed_count = 0;

    for i in 0..request_count {
        let status = limiter.check_and_record(key).await.unwrap();
        if status.allowed {
            allowed_count += 1;
        }

        if let Some(ref between) = between_requests {
            between(i).await;
        }
    }

    allowed_count == expected_allowed
}
