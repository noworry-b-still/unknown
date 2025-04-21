use rand;
use std::time::Duration;

/// Configuration for retry strategy
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: usize,
    /// Initial backoff duration
    pub initial_backoff: Duration,
    /// Maximum backoff duration
    pub max_backoff: Duration,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Whether to add jitter to backoff
    pub use_jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(5),
            backoff_multiplier: 2.0,
            use_jitter: true,
        }
    }
}

/// Exponential backoff implementation for retries
pub struct ExponentialBackoff {
    /// Current attempt number
    current_attempt: usize,
    /// Configuration for the retry strategy
    config: RetryConfig,
}

impl ExponentialBackoff {
    /// Create a new exponential backoff with the given configuration
    pub fn new(config: RetryConfig) -> Self {
        Self {
            current_attempt: 0,
            config,
        }
    }

    /// Get the next backoff duration, or None if max attempts reached
    pub fn next_backoff(&mut self) -> Option<Duration> {
        self.current_attempt += 1;

        if self.current_attempt > self.config.max_attempts {
            return None;
        }

        let exp = self.current_attempt as f64 - 1.0;
        let base_ms = self.config.initial_backoff.as_millis() as f64;
        let backoff_ms = base_ms * self.config.backoff_multiplier.powf(exp);
        let max_ms = self.config.max_backoff.as_millis() as f64;
        let capped_ms = backoff_ms.min(max_ms);

        let jittered_ms = if self.config.use_jitter {
            // Add jitter: random value between 50% and 100% of the calculated backoff
            let jitter = rand::random::<f64>() * 0.5 + 0.5;
            (capped_ms * jitter) as u64
        } else {
            capped_ms as u64
        };

        Some(Duration::from_millis(jittered_ms))
    }

    /// Reset the backoff to start from the beginning
    pub fn reset(&mut self) {
        self.current_attempt = 0;
    }
}
