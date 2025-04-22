// src/storage/redis.rs

use async_trait::async_trait;
use redis::{aio::ConnectionManager, AsyncCommands, Client, Pipeline};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::config::RedisConfig;
use crate::error::{RateLimiterError, Result, StorageError};
use crate::storage::{StorageBackend, StoragePipeline};

/// Redis pipeline implementation
pub struct RedisPipeline {
    pipeline: Pipeline,
}

impl RedisPipeline {
    /// Creates a new Redis pipeline
    fn new() -> Self {
        Self {
            pipeline: Pipeline::new(),
        }
    }
}

impl StoragePipeline for RedisPipeline {
    fn get(&mut self, key: &str) -> &mut Self {
        self.pipeline.cmd("GET").arg(key);
        self
    }

    fn set(&mut self, key: &str, value: &[u8], ttl: Option<Duration>) -> &mut Self {
        if let Some(ttl) = ttl {
            self.pipeline
                .cmd("SETEX")
                .arg(key)
                .arg(ttl.as_secs() as usize)
                .arg(value);
        } else {
            self.pipeline.cmd("SET").arg(key).arg(value);
        }
        self
    }

    fn increment(&mut self, key: &str, amount: i64) -> &mut Self {
        self.pipeline.cmd("INCRBY").arg(key).arg(amount);
        self
    }

    fn expire(&mut self, key: &str, ttl: Duration) -> &mut Self {
        self.pipeline
            .cmd("EXPIRE")
            .arg(key)
            .arg(ttl.as_secs() as usize);
        self
    }
}

// Let's take a different approach - instead of trying to derive Debug,
// we'll manually implement it
pub struct RedisStorage {
    client: Client,
    connection: Arc<tokio::sync::Mutex<ConnectionManager>>,
    config: RedisConfig,
}

// Manually implement Debug
impl fmt::Debug for RedisStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisStorage")
            .field("url", &self.config.url)
            .field("pool_size", &self.config.pool_size)
            .finish()
    }
}

// Manually implement Clone
impl Clone for RedisStorage {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            connection: Arc::clone(&self.connection),
            config: self.config.clone(),
        }
    }
}

impl RedisStorage {
    /// Creates a new Redis storage with the given configuration
    pub async fn new(config: RedisConfig) -> Result<Self> {
        // Open the client - this doesn't actually connect to Redis yet
        let client = Client::open(config.url.as_str())
            .map_err(|e| RateLimiterError::Storage(StorageError::RedisConnection(e.to_string())))?;

        // Create a connection manager with timeout
        let connection_future = ConnectionManager::new(client.clone());

        // Apply the connection timeout using tokio::time::timeout
        let connection_manager =
            match tokio::time::timeout(config.connection_timeout, connection_future).await {
                Ok(result) => {
                    // Connection attempt completed within timeout
                    result.map_err(|e| {
                        RateLimiterError::Storage(StorageError::RedisConnection(e.to_string()))
                    })?
                }
                Err(_) => {
                    // Connection attempt timed out
                    return Err(RateLimiterError::Storage(StorageError::RedisConnection(
                        format!(
                            "Connection to Redis at {} timed out after {:?}",
                            config.url, config.connection_timeout
                        ),
                    )));
                }
            };

        Ok(Self {
            client,
            connection: Arc::new(tokio::sync::Mutex::new(connection_manager)),
            config,
        })
    }

    /// Ping Redis to check health with timeout
    pub async fn ping(&self) -> Result<()> {
        // Use the same approach other methods use to get a connection
        let mut conn = self.connection.lock().await;

        // Apply the connection timeout to the ping operation
        let ping_future = redis::AsyncCommands::ping::<String>(&mut *conn);

        let result = match tokio::time::timeout(self.config.connection_timeout, ping_future).await {
            Ok(inner_result) => inner_result.map_err(|e| {
                RateLimiterError::Storage(StorageError::RedisCommand(e.to_string()))
            })?,
            Err(_) => {
                return Err(RateLimiterError::Storage(StorageError::RedisCommand(
                    format!(
                        "Redis PING operation timed out after {:?}",
                        self.config.connection_timeout
                    ),
                )));
            }
        };

        if result == "PONG" {
            Ok(())
        } else {
            Err(RateLimiterError::Storage(StorageError::RedisCommand(
                format!("Unexpected response from Redis PING: {}", result),
            )))
        }
    }
}

#[async_trait]
impl StorageBackend for RedisStorage {
    type Config = RedisConfig;
    type Pipeline = RedisPipeline;

    async fn new(config: Self::Config) -> Result<Self> {
        Self::new(config).await
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let mut conn = self.connection.lock().await;
        let result: Option<Vec<u8>> = redis::AsyncCommands::get(&mut *conn, key)
            .await
            .map_err(|e| RateLimiterError::Storage(StorageError::RedisCommand(e.to_string())))?;

        Ok(result)
    }

    async fn set(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> Result<()> {
        let mut conn = self.connection.lock().await;

        match ttl {
            Some(ttl) => {
                let _: () = redis::AsyncCommands::set_ex(&mut *conn, key, value, ttl.as_secs())
                    .await
                    .map_err(|e| {
                        RateLimiterError::Storage(StorageError::RedisCommand(e.to_string()))
                    })?;
            }
            None => {
                let _: () = redis::AsyncCommands::set(&mut *conn, key, value)
                    .await
                    .map_err(|e| {
                        RateLimiterError::Storage(StorageError::RedisCommand(e.to_string()))
                    })?;
            }
        }

        Ok(())
    }

    async fn increment(&self, key: &str, amount: i64) -> Result<i64> {
        let mut conn = self.connection.lock().await;
        let result: i64 = conn
            .incr(key, amount)
            .await
            .map_err(|e| RateLimiterError::Storage(StorageError::RedisCommand(e.to_string())))?;

        Ok(result)
    }

    async fn expire(&self, key: &str, ttl: Duration) -> Result<bool> {
        let mut conn = self.connection.lock().await;
        let result: bool = conn
            .expire(key, ttl.as_secs() as i64)
            .await
            .map_err(|e| RateLimiterError::Storage(StorageError::RedisCommand(e.to_string())))?;

        Ok(result)
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let mut conn = self.connection.lock().await;
        let result: bool = conn
            .exists(key)
            .await
            .map_err(|e| RateLimiterError::Storage(StorageError::RedisCommand(e.to_string())))?;

        Ok(result)
    }

    async fn delete(&self, key: &str) -> Result<bool> {
        let mut conn = self.connection.lock().await;
        let result: i64 = conn
            .del(key)
            .await
            .map_err(|e| RateLimiterError::Storage(StorageError::RedisCommand(e.to_string())))?;

        Ok(result > 0)
    }

    fn pipeline(&self) -> Self::Pipeline {
        RedisPipeline::new()
    }
    async fn execute_pipeline(&self, pipeline: Self::Pipeline) -> Result<Vec<Result<Vec<u8>>>> {
        let mut conn = self.connection.lock().await;
        let results: Vec<redis::Value> = pipeline
            .pipeline
            .query_async(&mut *conn)
            .await
            .map_err(|e| RateLimiterError::Storage(StorageError::RedisCommand(e.to_string())))?;

        // Convert Redis values to byte vectors
        let mut converted_results = Vec::with_capacity(results.len());
        for value in results {
            match value {
                redis::Value::Nil => converted_results.push(Ok(vec![])),
                redis::Value::Int(i) => converted_results.push(Ok(i.to_be_bytes().to_vec())),
                redis::Value::BulkString(bytes) => converted_results.push(Ok(bytes)),
                redis::Value::SimpleString(s) => converted_results.push(Ok(s.into_bytes())),
                redis::Value::Okay => converted_results.push(Ok("OK".as_bytes().to_vec())),
                redis::Value::Array(items) => {
                    let mut concatenated = Vec::new();
                    for item in items {
                        if let redis::Value::BulkString(bytes) = item {
                            concatenated.extend_from_slice(&bytes);
                        }
                    }
                    converted_results.push(Ok(concatenated));
                }
                redis::Value::Boolean(b) => converted_results.push(Ok(vec![b as u8])),
                redis::Value::Double(d) => {
                    let bytes = d.to_string().into_bytes();
                    converted_results.push(Ok(bytes));
                }
                _ => converted_results.push(Err(RateLimiterError::Storage(
                    StorageError::Serialization("Unsupported Redis value type".to_string()),
                ))),
            }
        }

        Ok(converted_results)
    }
}
