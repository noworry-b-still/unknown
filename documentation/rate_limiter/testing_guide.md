# Rate Limiter Testing Guide

This guide explains the testing strategy for the distributed rate limiting service and provides instructions for running the tests.

## Testing Strategy

The rate limiter employs a comprehensive testing approach with multiple layers:

1. **Unit Tests**: Testing individual components in isolation
2. **Integration Tests**: Testing components working together
3. **End-to-End Tests**: Testing the complete system

### Unit Testing Philosophy

Our unit tests follow these key principles:

- **Comprehensive Coverage**: Each algorithm and component is tested thoroughly
- **Isolation**: Components are tested in isolation using mock objects
- **Time Control**: Time-dependent behavior is tested using controlled time simulations
- **Edge Cases**: Special attention is given to boundary conditions and edge cases
- **Behavior Verification**: Tests verify behavior, not implementation details

## Test Organization

### Algorithm Tests

Each rate limiting algorithm has its own set of tests focusing on algorithm-specific behavior:

#### TokenBucket Tests
- **Initialization**: Tests different configuration options
- **Token Consumption**: Verifies tokens are consumed correctly
- **Token Regeneration**: Tests time-based token regeneration
- **Concurrency**: Tests behavior under concurrent access
- **Edge Cases**: Tests extreme configurations (zero capacity, etc.)
- **Reset**: Tests the reset functionality

#### FixedWindow Tests
- **Window Boundaries**: Tests behavior at window boundaries
- **Window Resets**: Verifies window counter resets correctly
- **Counter Accuracy**: Tests request counting accuracy
- **Multi-Window**: Tests behavior across multiple time windows
- **Edge Cases**: Tests extreme configurations
- **Reset**: Tests the reset functionality

#### SlidingWindow Tests
- **Precision Impact**: Tests how precision settings affect behavior
- **Partial Expiration**: Tests partial window expiration
- **Weighted Counting**: Tests the weighted counting mechanism
- **Bucket Transitions**: Tests transitions between time buckets
- **Edge Cases**: Tests extreme configurations
- **Reset**: Tests the reset functionality

### Storage Tests
- Tests for Redis storage implementation
- Tests for in-memory storage implementation
- Tests for storage pipeline operations

### Resilience Tests
- Tests for circuit breaker behavior
- Tests for exponential backoff mechanism
- Tests for health checking
- Tests for fallback mechanisms

## How to Run Tests

### Prerequisites
- Rust 1.60 or higher
- Redis server running locally (for integration tests)

### Running Unit Tests

To run all unit tests:

```bash
cargo test
```

To run tests for a specific algorithm:

```bash
cargo test --lib -- algorithms::token_bucket_tests
cargo test --lib -- algorithms::fixed_window_tests
cargo test --lib -- algorithms::sliding_window_tests
```

### Running Integration Tests

To run integration tests (requires Redis):

```bash
cargo test --test integration_tests
```

### Running with Custom Redis URL

For tests that require Redis, you can specify a custom Redis URL:

```bash
REDIS_URL=redis://custom-host:6379 cargo test --test integration_tests
```

### Time-Sensitive Tests

Some tests involve time-based behavior. If these tests are failing on slower machines, you can adjust the timing factors:

```bash
TEST_TIME_FACTOR=2.0 cargo test
```

## Testing Tools

The project provides several testing utilities:

### MockStorage

A mock implementation of the `StorageBackend` trait that simulates storage operations in memory:

```rust
let storage = MockStorage::new();
```

### TestClock

A controlled clock implementation for testing time-dependent behavior:

```rust
let clock = TestClock::new();
// Advance time by 5 seconds
clock.advance(Duration::from_secs(5));
```

### Test Helpers

Helper functions to simplify test setup:

```rust
// Create a test rate limiter with a mock storage
let limiter = create_test_rate_limiter(algorithm).await;

// Test a specific rate limit scenario
let success = test_rate_limit_scenario(
    algorithm,
    request_count,
    expected_allowed,
    between_requests,
).await;
```

## Best Practices

When writing new tests:

1. **Use the existing testing infrastructure** to simplify test setup
2. **Control time explicitly** when testing time-dependent behavior
3. **Test both success and failure cases**
4. **Include edge cases** in your tests
5. **Keep tests independent** - don't rely on state from other tests
6. **Use descriptive test names** that explain what's being tested
7. **Add new test helpers** when you find common testing patterns

## Continuous Integration

The test suite runs automatically on pull requests and commits to the main branch, ensuring code quality is maintained.
