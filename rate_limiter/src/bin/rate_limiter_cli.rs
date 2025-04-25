// src/bin/rate_limiter_cli.rs

use std::time::{Duration, Instant};
use structopt::StructOpt;
use tokio::time;
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use rate_limiter::algorithms::{FixedWindow, RateLimitAlgorithm, SlidingWindow, TokenBucket};
use rate_limiter::config::{
    FixedWindowConfig, InMemoryConfig, SlidingWindowConfig, TokenBucketConfig,
};
use rate_limiter::storage::MemoryStorage;
use rate_limiter::RateLimiter;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "rate_limiter_cli",
    about = "A CLI for testing rate limiting algorithms"
)]
struct Opt {
    /// Rate limiting algorithm to use
    #[structopt(short, long, possible_values = &["fixed_window", "sliding_window", "token_bucket"], default_value = "fixed_window")]
    algorithm: String,

    /// Key to use for rate limiting
    #[structopt(short, long, default_value = "default_user")]
    key: String,

    /// Maximum number of requests allowed
    #[structopt(short, long, default_value = "10")]
    max_requests: u64,

    /// Window duration in seconds
    #[structopt(short, long, default_value = "60")]
    window_seconds: u64,

    /// Precision for sliding window algorithm (number of buckets)
    #[structopt(short, long, default_value = "6")]
    precision: u32,

    /// Refill rate for token bucket algorithm (tokens per second)
    #[structopt(long, default_value = "0.0")]
    refill_rate: f64,

    /// Simulation mode
    #[structopt(long, possible_values = &["burst", "steady", "sine_wave", "custom"], default_value = "burst")]
    simulation: String,

    /// Number of requests to simulate
    #[structopt(short = "n", long, default_value = "20")]
    num_requests: usize,

    /// Time between requests in milliseconds (for steady and custom modes)
    #[structopt(short = "t", long, default_value = "100")]
    request_interval_ms: u64,

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
                "rate_limiter_cli={},rate_limiter={}",
                log_level, log_level
            )))
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set tracing subscriber");
    } else {
        // Set up minimal logging (errors only)
        let subscriber = FmtSubscriber::builder()
            .with_env_filter(EnvFilter::new("rate_limiter_cli=error,rate_limiter=error"))
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set tracing subscriber");
    }

    // Create memory storage
    let memory_config = InMemoryConfig {
        max_entries: 10_000,
        use_background_task: true,
        cleanup_interval: Duration::from_secs(60),
    };
    let storage = MemoryStorage::new(memory_config);

    if !opt.disable_logs {
        info!("Starting rate limiter CLI with {} algorithm", opt.algorithm);
        info!(
            "Configuration: max_requests={}, window={}s",
            opt.max_requests, opt.window_seconds
        );
    }

    // Create rate limiter with the selected algorithm
    let window_duration = Duration::from_secs(opt.window_seconds);

    match opt.algorithm.as_str() {
        "fixed_window" => {
            let algorithm_config = FixedWindowConfig {
                max_requests: opt.max_requests,
                window: window_duration,
            };
            let algorithm = FixedWindow::new(storage.clone(), algorithm_config);
            let rate_limiter = RateLimiter::new(algorithm, storage, "cli".to_string());

            if !opt.disable_logs {
                info!("Running simulation with Fixed Window algorithm");
            }
            run_simulation(&opt, rate_limiter).await?;
        }
        "sliding_window" => {
            let algorithm_config = SlidingWindowConfig {
                max_requests: opt.max_requests,
                window: window_duration,
                precision: opt.precision,
            };
            let algorithm = SlidingWindow::new(storage.clone(), algorithm_config);
            let rate_limiter = RateLimiter::new(algorithm, storage, "cli".to_string());

            if !opt.disable_logs {
                info!(
                    "Running simulation with Sliding Window algorithm (precision: {})",
                    opt.precision
                );
            }
            run_simulation(&opt, rate_limiter).await?;
        }
        "token_bucket" => {
            let algorithm_config = TokenBucketConfig {
                capacity: opt.max_requests,
                refill_rate: opt.refill_rate,
                initial_tokens: Some(opt.max_requests),
            };
            let algorithm = TokenBucket::new(storage.clone(), algorithm_config);
            let rate_limiter = RateLimiter::new(algorithm, storage, "cli".to_string());

            if !opt.disable_logs {
                info!(
                    "Running simulation with Token Bucket algorithm (refill rate: {}/s)",
                    opt.refill_rate
                );
            }
            run_simulation(&opt, rate_limiter).await?;
        }
        _ => {
            error!("Unknown algorithm: {}", opt.algorithm);
            return Err("Unknown algorithm".into());
        }
    }

    Ok(())
}

async fn run_simulation<A, S>(
    opt: &Opt,
    rate_limiter: RateLimiter<A, S>,
) -> Result<(), Box<dyn std::error::Error>>
where
    A: RateLimitAlgorithm,
    S: rate_limiter::storage::StorageBackend,
{
    match opt.simulation.as_str() {
        "burst" => simulate_burst(opt, rate_limiter).await,
        "steady" => simulate_steady(opt, rate_limiter).await,
        "sine_wave" => simulate_sine_wave(opt, rate_limiter).await,
        "custom" => simulate_custom(opt, rate_limiter).await,
        _ => {
            error!("Unknown simulation mode: {}", opt.simulation);
            Err("Unknown simulation mode".into())
        }
    }
}

// Simulate a burst of requests all at once
async fn simulate_burst<A, S>(
    opt: &Opt,
    rate_limiter: RateLimiter<A, S>,
) -> Result<(), Box<dyn std::error::Error>>
where
    A: RateLimitAlgorithm,
    S: rate_limiter::storage::StorageBackend,
{
    if !opt.disable_logs {
        info!(
            "Simulating burst of {} requests for key: {}",
            opt.num_requests, opt.key
        );
    }

    let mut allowed_count = 0;
    let mut denied_count = 0;
    let start_time = Instant::now();

    for i in 0..opt.num_requests {
        let result = rate_limiter.check_and_record(&opt.key).await?;

        if result.allowed {
            allowed_count += 1;
            if !opt.disable_logs {
                info!(
                    "Request {}: ALLOWED (remaining: {})",
                    i + 1,
                    result.remaining
                );
            }
        } else {
            denied_count += 1;
            if !opt.disable_logs {
                warn!("Request {}: DENIED (limit: {})", i + 1, result.limit);
            }
        }
    }

    let elapsed = start_time.elapsed();

    println!("\nBurst Simulation Results:");
    println!("-------------------------");
    println!("Total requests: {}", opt.num_requests);
    println!("Allowed: {}", allowed_count);
    println!("Denied: {}", denied_count);
    println!("Time elapsed: {:?}", elapsed);

    Ok(())
}

// Simulate a steady stream of requests with fixed intervals
async fn simulate_steady<A, S>(
    opt: &Opt,
    rate_limiter: RateLimiter<A, S>,
) -> Result<(), Box<dyn std::error::Error>>
where
    A: RateLimitAlgorithm,
    S: rate_limiter::storage::StorageBackend,
{
    if !opt.disable_logs {
        info!(
            "Simulating steady stream of {} requests with {}ms intervals for key: {}",
            opt.num_requests, opt.request_interval_ms, opt.key
        );
    }

    let mut allowed_count = 0;
    let mut denied_count = 0;
    let interval = Duration::from_millis(opt.request_interval_ms);
    let start_time = Instant::now();

    for i in 0..opt.num_requests {
        let request_time = Instant::now();

        let result = rate_limiter.check_and_record(&opt.key).await?;

        if result.allowed {
            allowed_count += 1;
            if !opt.disable_logs {
                info!(
                    "Request {}: ALLOWED (remaining: {})",
                    i + 1,
                    result.remaining
                );
            }
        } else {
            denied_count += 1;
            if !opt.disable_logs {
                warn!("Request {}: DENIED (limit: {})", i + 1, result.limit);
            }
        }

        // Calculate how long to wait before next request
        let elapsed = request_time.elapsed();
        if elapsed < interval {
            time::sleep(interval - elapsed).await;
        }
    }

    let elapsed = start_time.elapsed();

    println!("\nSteady Simulation Results:");
    println!("---------------------------");
    println!("Total requests: {}", opt.num_requests);
    println!("Allowed: {}", allowed_count);
    println!("Denied: {}", denied_count);
    println!("Time elapsed: {:?}", elapsed);

    Ok(())
}

// Simulate a sine wave pattern of requests (varying intervals)
async fn simulate_sine_wave<A, S>(
    opt: &Opt,
    rate_limiter: RateLimiter<A, S>,
) -> Result<(), Box<dyn std::error::Error>>
where
    A: RateLimitAlgorithm,
    S: rate_limiter::storage::StorageBackend,
{
    if !opt.disable_logs {
        info!(
            "Simulating sine wave pattern of {} requests for key: {}",
            opt.num_requests, opt.key
        );
    }

    let mut allowed_count = 0;
    let mut denied_count = 0;
    let base_interval = Duration::from_millis(opt.request_interval_ms);
    let start_time = Instant::now();

    for i in 0..opt.num_requests {
        let request_time = Instant::now();

        let result = rate_limiter.check_and_record(&opt.key).await?;

        if result.allowed {
            allowed_count += 1;
            if !opt.disable_logs {
                info!(
                    "Request {}: ALLOWED (remaining: {})",
                    i + 1,
                    result.remaining
                );
            }
        } else {
            denied_count += 1;
            if !opt.disable_logs {
                warn!("Request {}: DENIED (limit: {})", i + 1, result.limit);
            }
        }

        // Calculate sine wave interval
        // Full cycle over the total number of requests
        let phase = (i as f64 * std::f64::consts::PI * 2.0) / (opt.num_requests as f64);
        let sine_factor = 0.5 + 0.5 * phase.sin(); // Scale to 0.0-1.0

        // Vary interval between 0.5x and 1.5x the base interval
        let interval_factor = 0.5 + sine_factor; // 0.5 to 1.5
        let this_interval = base_interval.mul_f64(interval_factor);

        // Calculate how long to wait before next request
        let elapsed = request_time.elapsed();
        if elapsed < this_interval {
            time::sleep(this_interval - elapsed).await;
        }
    }

    let elapsed = start_time.elapsed();

    println!("\nSine Wave Simulation Results:");
    println!("------------------------------");
    println!("Total requests: {}", opt.num_requests);
    println!("Allowed: {}", allowed_count);
    println!("Denied: {}", denied_count);
    println!("Time elapsed: {:?}", elapsed);

    Ok(())
}

// Simulate custom pattern with interactive input
async fn simulate_custom<A, S>(
    opt: &Opt,
    rate_limiter: RateLimiter<A, S>,
) -> Result<(), Box<dyn std::error::Error>>
where
    A: RateLimitAlgorithm,
    S: rate_limiter::storage::StorageBackend,
{
    if !opt.disable_logs {
        info!(
            "Starting custom interactive simulation for key: {}",
            opt.key
        );
    }

    println!("\nCustom Simulation Mode");
    println!("----------------------");
    println!("Press Enter to make a request, or type 'quit' to exit");

    let mut allowed_count = 0;
    let mut denied_count = 0;
    let start_time = Instant::now();
    let mut input_buffer = String::new();

    loop {
        input_buffer.clear();
        std::io::stdin().read_line(&mut input_buffer)?;

        let trimmed = input_buffer.trim();
        if trimmed == "quit" || trimmed == "exit" || trimmed == "q" {
            break;
        }

        let result = rate_limiter.check_and_record(&opt.key).await?;

        if result.allowed {
            allowed_count += 1;
            println!("ALLOWED (remaining: {})", result.remaining);
        } else {
            denied_count += 1;
            println!("DENIED (reset after: {:?})", result.reset_after);
        }
    }

    let elapsed = start_time.elapsed();

    println!("\nCustom Simulation Results:");
    println!("--------------------------");
    println!("Allowed: {}", allowed_count);
    println!("Denied: {}", denied_count);
    println!("Time elapsed: {:?}", elapsed);

    Ok(())
}
