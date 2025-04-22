// src/resilience/tests/health_checker_tests.rs

use async_trait::async_trait;
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::task;
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::error::{RateLimiterError, Result, StorageError};
use crate::resilience::HealthCheckConfig;
use crate::storage::RedisStorage;

// Define a trait that abstracts the ping functionality we need
#[async_trait]
trait PingChecker: Debug + Send + Sync {
    async fn ping(&self) -> Result<()>;
}

// Implement the trait for RedisStorage
#[async_trait]
impl PingChecker for RedisStorage {
    async fn ping(&self) -> Result<()> {
        RedisStorage::ping(self).await
    }
}

// Create a mock implementation for testing
#[derive(Debug)]
struct MockPingChecker {
    should_fail: AtomicBool,
    add_delay: AtomicBool,
}

impl MockPingChecker {
    fn new() -> Self {
        Self {
            should_fail: AtomicBool::new(false),
            add_delay: AtomicBool::new(false),
        }
    }

    fn set_failure(&self, should_fail: bool) {
        self.should_fail.store(should_fail, Ordering::SeqCst);
    }

    fn set_delay(&self, add_delay: bool) {
        self.add_delay.store(add_delay, Ordering::SeqCst);
    }
}

#[async_trait]
impl PingChecker for MockPingChecker {
    async fn ping(&self) -> Result<()> {
        // Add delay if configured (simulates timeout)
        if self.add_delay.load(Ordering::SeqCst) {
            time::sleep(Duration::from_millis(100)).await;
        }

        // Return error if configured
        if self.should_fail.load(Ordering::SeqCst) {
            return Err(RateLimiterError::Storage(StorageError::RedisCommand(
                "Mock Redis failure".to_string(),
            )));
        }

        Ok(())
    }
}

// Test-specific health checker implementation
struct TestHealthChecker {
    is_healthy: Arc<AtomicBool>,
    checker: Arc<dyn PingChecker>,
    config: HealthCheckConfig,
    cancel_flag: Arc<AtomicBool>,
}

impl Debug for TestHealthChecker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestHealthChecker")
            .field("is_healthy", &self.is_healthy.load(Ordering::SeqCst))
            .field("checker", &self.checker)
            .field("config", &self.config)
            .field("cancel_flag", &self.cancel_flag.load(Ordering::SeqCst))
            .finish()
    }
}

impl TestHealthChecker {
    fn new(checker: Arc<dyn PingChecker>, config: HealthCheckConfig) -> Self {
        Self {
            is_healthy: Arc::new(AtomicBool::new(true)), // Assume healthy initially
            checker,
            config,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    fn start(&self) -> task::JoinHandle<()> {
        let checker = self.checker.clone();
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
                    // Use the trait method for health check
                    match checker.ping().await {
                        Ok(_) => true,
                        Err(e) => {
                            error!("Redis health check failed: {:?}", e);
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

    fn stop(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    fn is_healthy(&self) -> bool {
        self.is_healthy.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create a health checker for testing
    fn create_test_health_checker() -> (TestHealthChecker, Arc<MockPingChecker>) {
        let mock = Arc::new(MockPingChecker::new());

        let config = HealthCheckConfig {
            check_interval: Duration::from_millis(50), // Fast for testing
            check_timeout: Duration::from_millis(25),  // Short timeout
        };

        let checker = TestHealthChecker::new(mock.clone(), config);

        (checker, mock)
    }

    #[tokio::test]
    async fn test_health_status_updates() {
        // Create the health checker and mock
        let (health_checker, mock) = create_test_health_checker();

        // Initially healthy
        assert!(health_checker.is_healthy(), "Should start in healthy state");

        // Start the background task
        let handle = health_checker.start();

        // Simulate Redis failure
        mock.set_failure(true);

        // Wait for a check cycle
        time::sleep(Duration::from_millis(75)).await;

        // Should detect the failure
        assert!(
            !health_checker.is_healthy(),
            "Should be unhealthy after failure"
        );

        // Restore Redis
        mock.set_failure(false);

        // Wait for another check cycle
        time::sleep(Duration::from_millis(75)).await;

        // Should detect recovery
        assert!(
            health_checker.is_healthy(),
            "Should be healthy after recovery"
        );

        // Clean up
        health_checker.stop();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_periodic_checking() {
        // Create the health checker and mock
        let (health_checker, mock) = create_test_health_checker();

        // Track health changes
        let health_changes = Arc::new(AtomicUsize::new(0));
        let health_changes_clone = health_changes.clone();

        // Start the health checker
        let handle = health_checker.start();

        // Wrap in Arc for monitoring thread
        let health_checker_ref = Arc::new(health_checker);
        let health_checker_monitor = health_checker_ref.clone();

        // Start monitoring thread
        let monitor_handle = tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(25));
            let mut prev_health = true; // Start with assumed healthy

            // Monitor for about 400ms
            for _ in 0..16 {
                interval.tick().await;

                let current_health = health_checker_monitor.is_healthy();
                if current_health != prev_health {
                    health_changes.fetch_add(1, Ordering::SeqCst);
                    prev_health = current_health;
                }
            }
        });

        // Wait a bit, then start toggling the health status
        time::sleep(Duration::from_millis(75)).await;

        // Toggle health status several times
        for i in 0..3 {
            // Alternate between true/false
            mock.set_failure(i % 2 == 0);
            time::sleep(Duration::from_millis(75)).await;
        }

        // Wait for monitoring to finish
        monitor_handle.await.unwrap();

        // Should have detected at least 2 changes
        let changes = health_changes_clone.load(Ordering::SeqCst);
        assert!(
            changes >= 2,
            "Should detect at least 2 health changes, detected {}",
            changes
        );

        // Clean up
        health_checker_ref.stop();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_timeout_handling() {
        // Create the health checker and mock
        let (health_checker, mock) = create_test_health_checker();

        // Initially healthy
        assert!(health_checker.is_healthy(), "Should start healthy");

        // Start the health checker
        let handle = health_checker.start();

        // Add delay to simulate timeout
        mock.set_delay(true);

        // Wait for a check cycle
        time::sleep(Duration::from_millis(75)).await;

        // Should detect the timeout
        assert!(
            !health_checker.is_healthy(),
            "Should be unhealthy after timeout"
        );

        // Remove the delay
        mock.set_delay(false);

        // Wait for another check cycle
        time::sleep(Duration::from_millis(75)).await;

        // Should recover
        assert!(
            health_checker.is_healthy(),
            "Should be healthy after removing delay"
        );

        // Clean up
        health_checker.stop();
        let _ = handle.await;
    }
}
