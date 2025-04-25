// src/bin/rate_limiter_bench.rs

use std::sync::Arc;
use std::time::{Duration, Instant};
use structopt::StructOpt;
use tokio::sync::Barrier;
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use rate_limiter::algorithms::{FixedWindow, RateLimitAlgorithm, SlidingWindow, TokenBucket};
use rate_limiter::config::{
    FixedWindowConfig, InMemoryConfig, RedisConfig, SlidingWindowConfig, TokenBucketConfig,
};
use rate_limiter::storage::{MemoryStorage, RedisStorage, StorageBackend};
use rate_limiter::RateLimiter;

#[derive(Debug, Clone, StructOpt)]
#[structopt(
    name = "rate_limiter_bench",
    about = "A benchmarking tool for rate limiting algorithms"
)]
struct Opt {
    /// Rate limiting algorithm to benchmark
    #[structopt(short, long, possible_values = &["fixed_window", "sliding_window", "token_bucket", "all"], default_value = "all")]
    algorithm: String,

    /// Storage backend to use
    #[structopt(short, long, possible_values = &["memory", "redis"], default_value = "memory")]
    storage: String,

    /// Redis URL (when using Redis storage)
    #[structopt(long, default_value = "redis://localhost:6379")]
    redis_url: String,

    /// Maximum number of requests allowed
    #[structopt(short, long, default_value = "1000")]
    max_requests: u64,

    /// Window duration in seconds
    #[structopt(short, long, default_value = "60")]
    window_seconds: u64,

    /// Precision for sliding window algorithm (number of buckets)
    #[structopt(short, long, default_value = "6")]
    precision: u32,

    /// Number of concurrent users to simulate
    #[structopt(short = "u", long, default_value = "10")]
    num_users: usize,

    /// Number of requests per user
    #[structopt(short = "r", long, default_value = "100")]
    requests_per_user: usize,

    /// Number of iterations to run
    #[structopt(short, long, default_value = "3")]
    iterations: usize,

    /// Maximum concurrency level
    #[structopt(short = "c", long, default_value = "100")]
    concurrency: usize,

    /// Verbosity level
    #[structopt(short, long, parse(from_occurrences))]
    verbose: usize,

    /// Disable logs
    #[structopt(long)]
    disable_logs: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let opt = Opt::from_args();

    // Set up logging based on disable_logs flag
    if !opt.disable_logs {
        let log_level = match opt.verbose {
            0 => "info",
            1 => "debug",
            _ => "trace",
        };
        let subscriber = FmtSubscriber::builder()
            .with_env_filter(EnvFilter::new(format!(
                "rate_limiter_bench={},rate_limiter={}",
                log_level, log_level
            )))
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set tracing subscriber");
    } else {
        // Set up minimal logging (errors only)
        let subscriber = FmtSubscriber::builder()
            .with_env_filter(EnvFilter::new(
                "rate_limiter_bench=error,rate_limiter=error",
            ))
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set tracing subscriber");
    }

    // Determine which algorithms to benchmark
    let algorithms = if opt.algorithm == "all" {
        vec!["fixed_window", "sliding_window", "token_bucket"]
    } else {
        vec![opt.algorithm.as_str()]
    };

    // Run benchmarks for selected algorithms
    for alg in algorithms {
        match opt.storage.as_str() {
            "memory" => {
                benchmark_with_memory_storage(alg, &opt).await?;
            }
            "redis" => {
                benchmark_with_redis_storage(alg, &opt).await?;
            }
            _ => {
                return Err(format!("Unknown storage backend: {}", opt.storage).into());
            }
        }
    }

    Ok(())
}

async fn benchmark_with_memory_storage(
    algorithm: &str,
    opt: &Opt,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create memory storage
    let memory_config = InMemoryConfig {
        max_entries: 100_000, // Large capacity for benchmarking
        use_background_task: true,
        cleanup_interval: Duration::from_secs(60),
    };
    let storage = MemoryStorage::new(memory_config);

    if !opt.disable_logs {
        info!("Benchmarking {} algorithm with memory storage", algorithm);
        info!(
            "Configuration: max_requests={}, window={}s",
            opt.max_requests, opt.window_seconds
        );
    }

    // Create rate limiter with the selected algorithm
    let window_duration = Duration::from_secs(opt.window_seconds);

    match algorithm {
        "fixed_window" => {
            let config = FixedWindowConfig {
                max_requests: opt.max_requests,
                window: window_duration,
            };
            let algorithm = FixedWindow::new(storage.clone(), config);
            let rate_limiter = RateLimiter::new(algorithm, storage, "bench".to_string());

            run_benchmark(rate_limiter, "Fixed Window (Memory)", opt.clone()).await?;
        }
        "sliding_window" => {
            let config = SlidingWindowConfig {
                max_requests: opt.max_requests,
                window: window_duration,
                precision: opt.precision,
            };
            let algorithm = SlidingWindow::new(storage.clone(), config);
            let rate_limiter = RateLimiter::new(algorithm, storage, "bench".to_string());

            run_benchmark(rate_limiter, "Sliding Window (Memory)", opt.clone()).await?;
        }
        "token_bucket" => {
            let config = TokenBucketConfig {
                capacity: opt.max_requests,
                refill_rate: (opt.max_requests as f64) / (opt.window_seconds as f64),
                initial_tokens: Some(opt.max_requests),
            };
            let algorithm = TokenBucket::new(storage.clone(), config);
            let rate_limiter = RateLimiter::new(algorithm, storage, "bench".to_string());

            run_benchmark(rate_limiter, "Token Bucket (Memory)", opt.clone()).await?;
        }
        _ => {
            return Err(format!("Unknown algorithm: {}", algorithm).into());
        }
    }

    Ok(())
}

async fn benchmark_with_redis_storage(
    algorithm: &str,
    opt: &Opt,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create Redis storage
    let redis_config = RedisConfig {
        url: opt.redis_url.clone(),
        pool_size: 20, // Increased pool size for benchmarking
        connection_timeout: Duration::from_secs(5),
    };

    let storage = match RedisStorage::new(redis_config).await {
        Ok(storage) => storage,
        Err(e) => {
            error!("Failed to connect to Redis: {}", e);
            return Err(format!("Failed to connect to Redis: {}", e).into());
        }
    };

    if !opt.disable_logs {
        info!("Benchmarking {} algorithm with Redis storage", algorithm);
        info!(
            "Configuration: max_requests={}, window={}s",
            opt.max_requests, opt.window_seconds
        );
    }

    // Create rate limiter with the selected algorithm
    let window_duration = Duration::from_secs(opt.window_seconds);

    match algorithm {
        "fixed_window" => {
            let config = FixedWindowConfig {
                max_requests: opt.max_requests,
                window: window_duration,
            };
            let algorithm = FixedWindow::new(storage.clone(), config);
            let rate_limiter = RateLimiter::new(algorithm, storage, "bench".to_string());

            run_benchmark(rate_limiter, "Fixed Window (Redis)", opt.clone()).await?;
        }
        "sliding_window" => {
            let config = SlidingWindowConfig {
                max_requests: opt.max_requests,
                window: window_duration,
                precision: opt.precision,
            };
            let algorithm = SlidingWindow::new(storage.clone(), config);
            let rate_limiter = RateLimiter::new(algorithm, storage, "bench".to_string());

            run_benchmark(rate_limiter, "Sliding Window (Redis)", opt.clone()).await?;
        }
        "token_bucket" => {
            let config = TokenBucketConfig {
                capacity: opt.max_requests,
                refill_rate: (opt.max_requests as f64) / (opt.window_seconds as f64),
                initial_tokens: Some(opt.max_requests),
            };
            let algorithm = TokenBucket::new(storage.clone(), config);
            let rate_limiter = RateLimiter::new(algorithm, storage, "bench".to_string());

            run_benchmark(rate_limiter, "Token Bucket (Redis)", opt.clone()).await?;
        }
        _ => {
            return Err(format!("Unknown algorithm: {}", algorithm).into());
        }
    }

    Ok(())
}

async fn run_benchmark<A, S>(
    rate_limiter: RateLimiter<A, S>,
    name: &str,
    opt: Opt, // Changed from &Opt to Opt to fix the borrowing issue
) -> Result<(), Box<dyn std::error::Error>>
where
    A: RateLimitAlgorithm + 'static, // Added 'static lifetime bound
    S: StorageBackend + 'static,     // Added 'static lifetime bound
{
    println!("\nRunning benchmark: {}", name);
    println!("======================={}", "=".repeat(name.len()));

    let mut total_duration = Duration::from_secs(0);
    let mut total_allowed = 0;
    let mut total_denied = 0;

    // Convert to Arc for sharing between tasks
    let rate_limiter = Arc::new(rate_limiter);

    for iteration in 0..opt.iterations {
        if !opt.disable_logs {
            info!("Starting iteration {} of {}", iteration + 1, opt.iterations);
        }

        // Reset before each iteration
        for i in 0..opt.num_users {
            let key = format!("user_{}", i);
            rate_limiter.reset(&key).await?;
        }

        let start_time = Instant::now();

        // Create a barrier to start all tasks at once
        let barrier = Arc::new(Barrier::new(opt.num_users));
        let mut handles = Vec::with_capacity(opt.num_users);

        let concurrency_semaphore = Arc::new(tokio::sync::Semaphore::new(opt.concurrency));

        for user_id in 0..opt.num_users {
            let rate_limiter_clone = rate_limiter.clone();
            let barrier_clone = barrier.clone();
            let semaphore_clone = concurrency_semaphore.clone();
            let key = format!("user_{}", user_id);
            let requests_per_user = opt.requests_per_user; // Capture by value
            let disable_logs = opt.disable_logs; // Capture the flag for the spawned task

            let handle = tokio::spawn(async move {
                // Wait for all tasks to be ready
                barrier_clone.wait().await;

                let mut allowed = 0;
                let mut denied = 0;

                for _ in 0..requests_per_user {
                    // Limit concurrency
                    let _permit = semaphore_clone.acquire().await.unwrap();

                    // Make the request
                    match rate_limiter_clone.check_and_record(&key).await {
                        Ok(status) => {
                            if status.allowed {
                                allowed += 1;
                            } else {
                                denied += 1;
                            }
                        }
                        Err(e) => {
                            if !disable_logs {
                                warn!("Error in rate limiting: {}", e);
                            }
                        }
                    }
                }

                (allowed, denied)
            });

            handles.push(handle);
        }

        // Wait for all tasks to complete
        let results = futures::future::join_all(handles).await;

        // Collect results
        let mut iteration_allowed = 0;
        let mut iteration_denied = 0;

        for result in results {
            if let Ok((allowed, denied)) = result {
                iteration_allowed += allowed;
                iteration_denied += denied;
            }
        }

        let elapsed = start_time.elapsed();
        total_duration += elapsed;
        total_allowed += iteration_allowed;
        total_denied += iteration_denied;

        let total_requests = iteration_allowed + iteration_denied;
        let requests_per_second = total_requests as f64 / elapsed.as_secs_f64();

        println!(
            "Iteration {}: {:?}, {} allowed, {} denied, {:.2} req/sec",
            iteration + 1,
            elapsed,
            iteration_allowed,
            iteration_denied,
            requests_per_second
        );
    }

    // Calculate and display results
    let avg_duration = total_duration / opt.iterations as u32;
    let total_requests = total_allowed + total_denied;
    let avg_requests_per_second = total_requests as f64 / total_duration.as_secs_f64();

    println!("\nBenchmark Results for {}:", name);
    println!("  Total Requests:     {}", total_requests);
    println!(
        "  Allowed:            {} ({:.1}%)",
        total_allowed,
        100.0 * total_allowed as f64 / total_requests as f64
    );
    println!(
        "  Denied:             {} ({:.1}%)",
        total_denied,
        100.0 * total_denied as f64 / total_requests as f64
    );
    println!("  Avg. Duration:      {:?}", avg_duration);
    println!(
        "  Avg. Throughput:    {:.2} requests/second",
        avg_requests_per_second
    );

    Ok(())
}
