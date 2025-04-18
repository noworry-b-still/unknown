// for error definitions
use redis;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RateLimiterError {
    /// Returned when a rate limit has been exceeded
    #[error("Rate limit exceeded: {0}")]
    LimitExceeded(String),

    /// Errors related to the storage backend
    #[error("Storage error: {0}")]
    Storage(StorageError),

    /// Configuration-related errors
    #[error("Configuration error: {0}")]
    Config(String),

    /// Algorithm-specific errors
    #[error("Algorithm error: {0}")]
    Algorithm(String),

    /// Unexpected or internal errors
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Storage-specific errors
#[derive(Error, Debug)]
pub enum StorageError {
    /// Redis connection errors
    #[error("Redis connection error: {0}")]
    RedisConnection(String),

    // Redis authentication errors
    #[error("Redis authentication error: {0}")]
    RedisAuth(String),

    /// Redis command errors
    #[error("Redis command error: {0}")]
    RedisCommand(String),

    /// Data serialization/deserialization errors
    #[error("Data serialization error: {0}")]
    Serialization(String),

    /// Key not found in storage
    #[error("Key not found: {0}")]
    KeyNotFound(String),
}

// Implement conversions from redis::RedisError to StorageError
impl From<redis::RedisError> for RateLimiterError {
    fn from(err: redis::RedisError) -> Self {
        match err.kind() {
            redis::ErrorKind::AuthenticationFailed => {
                // authentication errors
                RateLimiterError::Storage(StorageError::RedisAuth(err.to_string()))
            }
            redis::ErrorKind::IoError | redis::ErrorKind::ClientError => {
                // Connection-related errors
                RateLimiterError::Storage(StorageError::RedisConnection(err.to_string()))
            }
            _ => {
                // Command/operation related errors
                RateLimiterError::Storage(StorageError::RedisCommand(err.to_string()))
            }
        }
    }
}

// implement conversions from serde_json::Error to RateLimiterError
impl From<serde_json::Error> for RateLimiterError {
    fn from(err: serde_json::Error) -> Self {
        RateLimiterError::Storage(StorageError::Serialization(err.to_string()))
    }
}

// define a Result type alias for convenience
pub type Result<T> = std::result::Result<T, RateLimiterError>;
