// src/storage/mod.rs

pub mod memory;
pub mod redis;

pub use memory::{MemoryPipeline, MemoryStorage};
pub use redis::{RedisPipeline, RedisStorage};

use super::error::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use std::time::Duration;

// Represents a pipeline of operations to be executed as a transaction
pub trait StoragePipeline: Send + Sync {
    // Add a get operation to the pipeline
    fn get(&mut self, key: &str) -> &mut Self;

    // Add a set operation to the pipeline
    fn set(&mut self, key: &str, value: &[u8], ttl: Option<Duration>) -> &mut Self;

    // Add an increment operation to the pipeline
    fn increment(&mut self, key: &str, amount: i64) -> &mut Self;

    // Add an expire operation to the pipeline
    fn expire(&mut self, key: &str, ttl: Duration) -> &mut Self;
}

/// Core trait that all storage backends must implement
#[async_trait]
pub trait StorageBackend: Send + Sync + Debug {
    // The type of configuration this storage backend accepts
    type Config: Send + Sync;

    // The type of pipeline this storage backend uses
    type Pipeline: StoragePipeline;

    // Creates a new instance of this storage backend with the given configuration
    async fn new(config: Self::Config) -> Result<Self>
    where
        Self: Sized;

    // Retrieves a value by key
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;

    // Stores a value with a key
    async fn set(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> Result<()>;

    // Atomically increments a counter
    async fn increment(&self, key: &str, amount: i64) -> Result<i64>;

    // Sets expiration time for a key
    async fn expire(&self, key: &str, ttl: Duration) -> Result<bool>;

    // Checks if a key exists
    async fn exists(&self, key: &str) -> Result<bool>;

    // Deletes a key
    async fn delete(&self, key: &str) -> Result<bool>;

    // Creates a new pipeline for executing multiple operations
    fn pipeline(&self) -> Self::Pipeline;

    // Executes a pipeline of operations
    async fn execute_pipeline(&self, pipeline: Self::Pipeline) -> Result<Vec<Result<Vec<u8>>>>;
}
