// src/resilience/tests/circuit_breaker_tests.rs

use std::time::Duration;
use tokio::time;

use crate::resilience::{CircuitBreaker, CircuitBreakerConfig, CircuitState};

#[tokio::test]
async fn test_initial_state_is_closed() {
    let config = CircuitBreakerConfig::default();
    let breaker = CircuitBreaker::new(config);

    assert_eq!(breaker.get_state().await, CircuitState::Closed);
    assert!(breaker.allow_request().await);
}

#[tokio::test]
async fn test_circuit_opens_after_failures() {
    let config = CircuitBreakerConfig {
        failure_threshold: 3,
        reset_timeout: Duration::from_secs(1),
        success_threshold: 2,
    };

    let breaker = CircuitBreaker::new(config);

    // Record failures
    breaker.record_failure().await;
    assert_eq!(breaker.get_state().await, CircuitState::Closed);

    breaker.record_failure().await;
    assert_eq!(breaker.get_state().await, CircuitState::Closed);

    // This should open the circuit
    breaker.record_failure().await;

    // Important: Make sure state has been updated
    let state = breaker.get_state().await;
    assert_eq!(
        state,
        CircuitState::Open,
        "Circuit should be Open after 3 failures"
    );

    // Request should be denied
    assert!(!breaker.allow_request().await);
}

#[tokio::test]
async fn test_circuit_transitions_to_half_open() {
    let config = CircuitBreakerConfig {
        failure_threshold: 3,
        reset_timeout: Duration::from_millis(100), // Short timeout for testing
        success_threshold: 2,
    };

    let breaker = CircuitBreaker::new(config);

    // Open the circuit
    for _ in 0..3 {
        breaker.record_failure().await;
    }

    // Verify it's open
    assert_eq!(
        breaker.get_state().await,
        CircuitState::Open,
        "Circuit should be Open after failures"
    );

    // Wait for the reset timeout
    time::sleep(Duration::from_millis(150)).await;

    // Next request should transition to half-open
    let allowed = breaker.allow_request().await;
    assert!(allowed, "Request should be allowed after timeout");

    // Verify state transition
    assert_eq!(
        breaker.get_state().await,
        CircuitState::HalfOpen,
        "Circuit should be HalfOpen after timeout"
    );
}

#[tokio::test]
async fn test_circuit_closes_after_successes_in_half_open() {
    let config = CircuitBreakerConfig {
        failure_threshold: 3,
        reset_timeout: Duration::from_millis(100),
        success_threshold: 2,
    };

    let breaker = CircuitBreaker::new(config);

    // Open the circuit
    for _ in 0..3 {
        breaker.record_failure().await;
    }

    // Verify it's open
    assert_eq!(
        breaker.get_state().await,
        CircuitState::Open,
        "Circuit should be Open after failures"
    );

    // Wait for reset timeout
    time::sleep(Duration::from_millis(150)).await;

    // Allow request to transition to half-open
    let allowed = breaker.allow_request().await;
    assert!(allowed, "Request should be allowed after timeout");
    assert_eq!(
        breaker.get_state().await,
        CircuitState::HalfOpen,
        "Circuit should transition to HalfOpen"
    );

    // Record successes in half-open state
    breaker.record_success().await;
    assert_eq!(
        breaker.get_state().await,
        CircuitState::HalfOpen,
        "Still in HalfOpen after first success"
    );

    // This should close the circuit
    breaker.record_success().await;

    // Verify state transition
    assert_eq!(
        breaker.get_state().await,
        CircuitState::Closed,
        "Circuit should be Closed after success threshold"
    );
}

#[tokio::test]
async fn test_failure_in_half_open_reopens_circuit() {
    let config = CircuitBreakerConfig {
        failure_threshold: 3,
        reset_timeout: Duration::from_millis(100),
        success_threshold: 2,
    };

    let breaker = CircuitBreaker::new(config);

    // Open the circuit
    for _ in 0..3 {
        breaker.record_failure().await;
    }

    // Verify it's open
    assert_eq!(
        breaker.get_state().await,
        CircuitState::Open,
        "Circuit should be Open after failures"
    );

    // Wait for reset timeout
    time::sleep(Duration::from_millis(150)).await;

    // Allow request to transition to half-open
    breaker.allow_request().await;
    assert_eq!(
        breaker.get_state().await,
        CircuitState::HalfOpen,
        "Circuit should transition to HalfOpen"
    );

    // Record one success
    breaker.record_success().await;
    assert_eq!(
        breaker.get_state().await,
        CircuitState::HalfOpen,
        "Still in HalfOpen after first success"
    );

    // Record a failure - should reopen the circuit
    breaker.record_failure().await;

    // Verify reopened
    assert_eq!(
        breaker.get_state().await,
        CircuitState::Open,
        "Circuit should be Open after failure in HalfOpen"
    );
}

#[tokio::test]
async fn test_success_in_closed_state_resets_failure_count() {
    let config = CircuitBreakerConfig {
        failure_threshold: 3,
        reset_timeout: Duration::from_secs(1),
        success_threshold: 2,
    };

    let breaker = CircuitBreaker::new(config);

    // Record two failures
    breaker.record_failure().await;
    breaker.record_failure().await;
    assert_eq!(
        breaker.get_state().await,
        CircuitState::Closed,
        "Circuit should be Closed after 2 failures"
    );

    // Record a success
    breaker.record_success().await;

    // Record a failure - shouldn't open the circuit since failure count was reset
    breaker.record_failure().await;
    assert_eq!(
        breaker.get_state().await,
        CircuitState::Closed,
        "Circuit should remain Closed after success reset"
    );

    // Two more failures should open it
    breaker.record_failure().await;
    breaker.record_failure().await;

    // Verify it's open
    assert_eq!(
        breaker.get_state().await,
        CircuitState::Open,
        "Circuit should open after reaching threshold again"
    );
}

#[tokio::test]
async fn test_custom_configuration() {
    let config = CircuitBreakerConfig {
        failure_threshold: 5, // High threshold
        reset_timeout: Duration::from_secs(2),
        success_threshold: 3, // Need 3 successes to close
    };

    let breaker = CircuitBreaker::new(config);

    // Should take 5 failures to open
    for i in 0..4 {
        breaker.record_failure().await;
        assert_eq!(
            breaker.get_state().await,
            CircuitState::Closed,
            "Circuit should remain closed after {} failures",
            i + 1
        );
    }

    // 5th failure opens it
    breaker.record_failure().await;
    assert_eq!(
        breaker.get_state().await,
        CircuitState::Open,
        "Circuit should open after 5 failures"
    );

    // Wait for reset
    time::sleep(Duration::from_secs(2)).await;

    // Transition to half-open
    breaker.allow_request().await;
    assert_eq!(
        breaker.get_state().await,
        CircuitState::HalfOpen,
        "Circuit should transition to HalfOpen"
    );

    // Should take 3 successes to close
    breaker.record_success().await;
    assert_eq!(
        breaker.get_state().await,
        CircuitState::HalfOpen,
        "Circuit should remain half-open after 1 success"
    );

    breaker.record_success().await;
    assert_eq!(
        breaker.get_state().await,
        CircuitState::HalfOpen,
        "Circuit should remain half-open after 2 successes"
    );

    // 3rd success closes it
    breaker.record_success().await;
    assert_eq!(
        breaker.get_state().await,
        CircuitState::Closed,
        "Circuit should close after 3 successes"
    );
}
