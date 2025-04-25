# CLI Demonstration Files

## `{algorithm}_basic_demo.txt`
- Demonstrates basic rate limiting functionality for each algorithm.
- Uses simple parameters (e.g., `max_requests=5`, `window=10s`).
- Showcases core behavior under standard conditions.

## `{algorithm}_{simulation}_simulation.txt`
Contains simulation-specific demonstrations for each algorithm:

- **`burst_simulation.txt`**:  
  Behavior under sudden spikes in requests.

- **`steady_simulation.txt`**:  
  Behavior with evenly spaced, constant-rate requests.

- **`sine_wave_simulation.txt`**:  
  Behavior with varying request rates in a sinusoidal pattern.

## `{algorithm}_specific.txt`
- Highlights algorithm-specific features or edge-case behaviors.
- **Fixed Window**: Small window durations.  
- **Sliding Window**: High-precision settings.  
- **Token Bucket**: Specific refill rates and burst handling.

---

# Performance Benchmark Files

## `memory_all_basic.txt`
- Combined benchmark results for all algorithms using **memory** storage.
- Uses standard settings for direct comparison.

## `{algorithm}_memory_basic.txt`
- Memory-based benchmark for a specific algorithm.
- Moderate load test (20 users × 50 requests each).

## `{algorithm}_memory_limits.txt`
- Stress test with tight rate limits.
- Shows how algorithms handle high denial rates.

## `memory_high_load.txt`
- High concurrency and request volume using memory storage.
- Evaluates max throughput and performance limits.

## `redis_all_basic.txt`
- Combined benchmark results for all algorithms using **Redis** storage.
- Directly comparable to `memory_all_basic.txt`.

## `{algorithm}_redis_basic.txt`
- Redis-based benchmark for a specific algorithm.
- Useful for comparing memory vs. Redis performance.

## `redis_medium_load.txt`
- Medium load benchmark using Redis.
- Highlights performance differences between memory and Redis storage.

---

# Summary Report

## `summary.md`
- Comprehensive analysis of all benchmark results.
- Includes:
  - Tables comparing algorithms and storage backends.
  - Calculated metrics like throughput and performance ratios.
  - Conclusions and practical recommendations.
