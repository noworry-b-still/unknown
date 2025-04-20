// src/storage/memory.rs

// In-memory storage (for testing and lightweight usage)
// This module provides an in-memory storage backend for the rate limiter.
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::task;
use tokio::time;

use crate::config::InMemoryConfig;
use crate::error::{RateLimiterError, Result, StorageError};
use crate::storage::{StorageBackend, StoragePipeline};

/// A simple pipeline implementation for in-memory storage
#[derive(Default)]
pub struct MemoryPipeline {
    operations: Vec<MemoryOperation>,
}

/// Represents an operation in the memory pipeline
enum MemoryOperation {
    Get(String),
    Set(String, Vec<u8>, Option<Duration>),
    Increment(String, i64),
    Expire(String, Duration),
}

impl StoragePipeline for MemoryPipeline {
    fn get(&mut self, key: &str) -> &mut Self {
        self.operations.push(MemoryOperation::Get(key.to_string()));
        self
    }

    fn set(&mut self, key: &str, value: &[u8], ttl: Option<Duration>) -> &mut Self {
        self.operations
            .push(MemoryOperation::Set(key.to_string(), value.to_vec(), ttl));
        self
    }

    fn increment(&mut self, key: &str, amount: i64) -> &mut Self {
        self.operations
            .push(MemoryOperation::Increment(key.to_string(), amount));
        self
    }

    fn expire(&mut self, key: &str, ttl: Duration) -> &mut Self {
        self.operations
            .push(MemoryOperation::Expire(key.to_string(), ttl));
        self
    }
}

/// Entry in the in-memory storage
#[derive(Debug)]
struct MemoryEntry {
    value: Vec<u8>,
    expiry: Option<Instant>,
}

/// In-memory storage backend implementation
#[derive(Debug, Clone)]
pub struct MemoryStorage {
    data: Arc<RwLock<HashMap<String, MemoryEntry>>>,
    config: InMemoryConfig,
    _cleanup_task: Option<Arc<()>>, // Kepp a reference to prevent task from being dropped
}

impl MemoryStorage {
    /// Creates a new in-memory storage with the given configuration
    pub fn new(config: InMemoryConfig) -> Self {
        let data = Arc::new(RwLock::new(HashMap::with_capacity(
            config.max_entries.min(10_000),
        )));

        let cleanup_task = if config.use_background_task {
            let data_clone = Arc::clone(&data);
            let interval = config.cleanup_interval;

            // Create a background task for cleaning up expired entries
            let _task = task::spawn(async move {
                let mut interval = time::interval(interval);
                loop {
                    interval.tick().await;
                    Self::cleanup_expired_entries(&data_clone);
                }
            });

            // Create a reference to keep the task alive
            Some(Arc::new(()))
        } else {
            None
        };

        Self {
            data,
            config,
            _cleanup_task: cleanup_task,
        }
    }

    /// Clean up expired entries
    fn cleanup_expired_entries(data: &Arc<RwLock<HashMap<String, MemoryEntry>>>) {
        let now = Instant::now();
        let mut data = data.write().unwrap();

        // Remove expired entries
        data.retain(|_, entry| match entry.expiry {
            Some(expiry) => expiry > now,
            None => true,
        });
    }

    /// Check if an entry is expired and remove it if necessary
    fn check_expiry(&self, key: &str) -> Result<bool> {
        let data = self.data.read().unwrap();

        if let Some(entry) = data.get(key) {
            if let Some(expiry) = entry.expiry {
                if expiry <= Instant::now() {
                    // Key is expired, drop read lock and acquire write lock to remove it
                    drop(data);
                    let mut data = self.data.write().unwrap();
                    data.remove(key);
                    return Ok(false);
                }
            }
            Ok(true) // Key exists and is not expired
        } else {
            Ok(false) // Key doesn't exist
        }
    }
}

#[async_trait]
impl StorageBackend for MemoryStorage {
    type Config = InMemoryConfig;
    type Pipeline = MemoryPipeline;

    async fn new(config: Self::Config) -> Result<Self> {
        Ok(Self::new(config))
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        // Check if key is expired
        if !self.check_expiry(key)? {
            return Ok(None);
        }

        // Get the value
        let data = self.data.read().unwrap();
        if let Some(entry) = data.get(key) {
            Ok(Some(entry.value.clone()))
        } else {
            Ok(None)
        }
    }

    async fn set(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> Result<()> {
        let mut data = self.data.write().unwrap();

        // Apply max entries limit
        if data.len() >= self.config.max_entries && !data.contains_key(key) {
            return Err(RateLimiterError::Storage(StorageError::RedisCommand(
                "Maximum entries limit exceeded".to_string(),
            )));
        }

        // Set the entry with expiry
        let expiry = ttl.map(|duration| Instant::now() + duration);
        data.insert(
            key.to_string(),
            MemoryEntry {
                value: value.to_vec(),
                expiry,
            },
        );

        Ok(())
    }

    async fn increment(&self, key: &str, amount: i64) -> Result<i64> {
        // Check if key is expired
        if !self.check_expiry(key)? {
            // Key doesn't exist or is expired, set to amount
            self.set(key, &amount.to_be_bytes(), None).await?;
            return Ok(amount);
        }

        let mut data = self.data.write().unwrap();

        let current_value = if let Some(entry) = data.get(key) {
            if entry.value.len() == 8 {
                let mut bytes = [0; 8];
                bytes.copy_from_slice(&entry.value);
                i64::from_be_bytes(bytes)
            } else {
                0
            }
        } else {
            0
        };

        let new_value = current_value + amount;
        let expiry = if let Some(entry) = data.get(key) {
            entry.expiry
        } else {
            None
        };

        data.insert(
            key.to_string(),
            MemoryEntry {
                value: new_value.to_be_bytes().to_vec(),
                expiry,
            },
        );

        Ok(new_value)
    }

    async fn expire(&self, key: &str, ttl: Duration) -> Result<bool> {
        let mut data = self.data.write().unwrap();

        if let Some(entry) = data.get_mut(key) {
            entry.expiry = Some(Instant::now() + ttl);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        // Check if key is expired
        if !self.check_expiry(key)? {
            return Ok(false);
        }

        let data = self.data.read().unwrap();
        Ok(data.contains_key(key))
    }

    async fn delete(&self, key: &str) -> Result<bool> {
        let mut data = self.data.write().unwrap();
        Ok(data.remove(key).is_some())
    }

    fn pipeline(&self) -> Self::Pipeline {
        MemoryPipeline::default()
    }

    async fn execute_pipeline(&self, pipeline: Self::Pipeline) -> Result<Vec<Result<Vec<u8>>>> {
        let mut results = Vec::with_capacity(pipeline.operations.len());

        for op in pipeline.operations {
            match op {
                MemoryOperation::Get(key) => {
                    let result = self.get(&key).await?;
                    results.push(Ok(result.unwrap_or_default()));
                }
                MemoryOperation::Set(key, value, ttl) => {
                    self.set(&key, &value, ttl).await?;
                    results.push(Ok(vec![]));
                }
                MemoryOperation::Increment(key, amount) => {
                    let result = self.increment(&key, amount).await?;
                    results.push(Ok(result.to_be_bytes().to_vec()));
                }
                MemoryOperation::Expire(key, ttl) => {
                    let result = self.expire(&key, ttl).await?;
                    results.push(Ok(vec![result as u8]));
                }
            }
        }

        Ok(results)
    }
}
