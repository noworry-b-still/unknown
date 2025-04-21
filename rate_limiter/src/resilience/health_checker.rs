use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::task;
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::storage::RedisStorage;

/// Configuration for health checks
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    /// How often to check Redis health
    pub check_interval: Duration,
    /// Timeout for health check operations
    pub check_timeout: Duration,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(5),
            check_timeout: Duration::from_secs(1),
        }
    }
}

/// Health check implementation for Redis
#[derive(Debug)]
pub struct HealthChecker {
    /// Flag indicating if Redis is healthy
    is_healthy: Arc<AtomicBool>,
    /// Redis storage reference
    redis: Arc<RedisStorage>,
    /// Configuration for health checks
    config: HealthCheckConfig,
    /// Cancel flag for the health check task
    cancel_flag: Arc<AtomicBool>,
}

impl HealthChecker {
    /// Create a new health checker with the given Redis storage and configuration
    pub fn new(redis: Arc<RedisStorage>, config: HealthCheckConfig) -> Self {
        Self {
            is_healthy: Arc::new(AtomicBool::new(true)), // Assume healthy initially
            redis,
            config,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the health checker background task
    pub fn start(&self) -> task::JoinHandle<()> {
        let redis = Arc::clone(&self.redis);
        let is_healthy = Arc::clone(&self.is_healthy);
        let interval = self.config.check_interval;
        let timeout = self.config.check_timeout;
        let cancel_flag = Arc::clone(&self.cancel_flag);

        task::spawn(async move {
            let mut interval_timer = time::interval(interval);

            loop {
                // Check if we should stop
                if cancel_flag.load(Ordering::SeqCst) {
                    break;
                }

                interval_timer.tick().await;

                // Create a timeout for the health check
                let health_check = async {
                    // Simple PING command as health check
                    match redis.ping().await {
                        Ok(_) => true,
                        Err(e) => {
                            error!("Redis health check failed: {}", e);
                            false
                        }
                    }
                };

                // Run the health check with a timeout
                let is_redis_healthy = match time::timeout(timeout, health_check).await {
                    Ok(result) => result,
                    Err(_) => {
                        error!("Redis health check timed out after {:?}", timeout);
                        false
                    }
                };

                // Update the health status
                let previous_status = is_healthy.swap(is_redis_healthy, Ordering::SeqCst);
                if previous_status != is_redis_healthy {
                    if is_redis_healthy {
                        info!("Redis is now healthy");
                    } else {
                        warn!("Redis is now unhealthy");
                    }
                }
            }

            debug!("Health checker task stopped");
        })
    }

    /// Stop the health checker
    pub fn stop(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    /// Check if Redis is currently healthy
    pub fn is_healthy(&self) -> bool {
        self.is_healthy.load(Ordering::SeqCst)
    }
}
