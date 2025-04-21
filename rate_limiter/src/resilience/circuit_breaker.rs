use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// The state of the circuit breaker
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed, requests flow normally
    Closed,
    /// Circuit is open, requests are failing and not being sent
    Open,
    /// Circuit is partially open, allowing a limited number of requests to test recovery
    HalfOpen,
}

/// Configuration for circuit breaker
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening the circuit
    pub failure_threshold: usize,
    /// Duration to keep the circuit open before transitioning to half-open
    pub reset_timeout: Duration,
    /// Number of consecutive successes in half-open state to close the circuit
    pub success_threshold: usize,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            reset_timeout: Duration::from_secs(30),
            success_threshold: 3,
        }
    }
}

/// Circuit breaker implementation
#[derive(Debug)]
pub struct CircuitBreaker {
    /// Current state of the circuit breaker
    state: RwLock<CircuitState>,
    /// Count of consecutive failures
    failure_count: AtomicUsize,
    /// Count of consecutive successes
    success_count: AtomicUsize,
    /// Timestamp when the circuit was opened
    opened_at: RwLock<Option<Instant>>,
    /// Configuration for the circuit breaker
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given configuration
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: RwLock::new(CircuitState::Closed),
            failure_count: AtomicUsize::new(0),
            success_count: AtomicUsize::new(0),
            opened_at: RwLock::new(None),
            config,
        }
    }

    /// Check if the circuit breaker allows the request to proceed
    pub async fn allow_request(&self) -> bool {
        // Get a copy of the current state to avoid holding the lock
        let current_state = *self.state.read().await;

        match current_state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if it's time to transition to half-open
                let opened_at_value = self.opened_at.read().await.clone();

                if let Some(time) = opened_at_value {
                    if time.elapsed() > self.config.reset_timeout {
                        // Update state to half-open
                        let mut state = self.state.write().await;
                        *state = CircuitState::HalfOpen;
                        debug!("Circuit breaker state transitioned to half-open");
                        return true;
                    }
                }
                false
            }
            CircuitState::HalfOpen => {
                // In half-open state, allow a limited number of requests through
                true
            }
        }
    }

    /// Record a successful operation
    pub async fn record_success(&self) {
        // Get a copy of the current state to avoid holding the lock
        let current_state = *self.state.read().await;

        match current_state {
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::SeqCst);
            }
            CircuitState::HalfOpen => {
                // Increment success count
                let new_success_count = self.success_count.fetch_add(1, Ordering::SeqCst) + 1;

                // If we've reached the success threshold, close the circuit
                if new_success_count >= self.config.success_threshold {
                    let mut state = self.state.write().await;
                    *state = CircuitState::Closed;
                    self.success_count.store(0, Ordering::SeqCst);
                    self.failure_count.store(0, Ordering::SeqCst);
                    debug!(
                        "Circuit breaker state transitioned to closed after successful operations"
                    );
                }
            }
            CircuitState::Open => {
                // This should not happen, but just in case
                debug!("Received success in Open state - this is unexpected");
            }
        }
    }

    /// Record a failed operation
    pub async fn record_failure(&self) {
        // Get a copy of the current state to avoid holding the lock
        let current_state = *self.state.read().await;

        match current_state {
            CircuitState::Closed => {
                // Increment failure count
                let new_failure_count = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;

                // If we've reached the failure threshold, open the circuit
                if new_failure_count >= self.config.failure_threshold {
                    let now = Instant::now();

                    // First update the state
                    {
                        let mut state = self.state.write().await;
                        *state = CircuitState::Open;
                    }

                    // Then update the opened_at time
                    {
                        let mut opened_at = self.opened_at.write().await;
                        *opened_at = Some(now);
                    }

                    warn!(
                        "Circuit breaker opened after {} consecutive failures",
                        new_failure_count
                    );
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open state opens the circuit again
                {
                    let mut state = self.state.write().await;
                    *state = CircuitState::Open;
                }

                {
                    let mut opened_at = self.opened_at.write().await;
                    *opened_at = Some(Instant::now());
                }

                self.success_count.store(0, Ordering::SeqCst);
                warn!("Circuit breaker re-opened after failure in half-open state");
            }
            CircuitState::Open => {
                // Already open, nothing to do
            }
        }
    }

    /// Get the current state of the circuit breaker
    pub async fn get_state(&self) -> CircuitState {
        *self.state.read().await
    }
}
