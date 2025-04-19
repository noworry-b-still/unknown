use std::sync::Once;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

// Ensure initialization happens only once
static INIT: Once = Once::new();

/// Initialize the logging system with sensible defaults.
///
/// Log level can be set using the RUST_LOG environment variable.
/// Example: RUST_LOG=debug,rate_limiter=trace
pub fn init() {
    INIT.call_once(|| {
        // Create a filter based on the RUST_LOG environment variable
        // Default to 'info' level if not specified
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

        // Set up the subscriber with a simple console format
        tracing_subscriber::registry()
            .with(filter)
            .with(
                fmt::layer()
                    .with_target(true) // Include module path in logs
                    .with_thread_ids(true) // Useful for debugging concurrency issues
                    .with_line_number(true),
            ) // Include line numbers for error location
            .init();

        tracing::info!("Logging initialized");
    });
}

/// Macro for logging rate limiting events
#[macro_export]
macro_rules! rate_limit_event {
    ($tenant:expr, $key:expr, $allowed:expr, $limit:expr, $window:expr) => {
        tracing::info!(
            tenant_id = $tenant,
            key = $key,
            allowed = $allowed,
            limit = $limit,
            window_seconds = $window,
            "Rate limit check"
        )
    };
}

/// Macro for logging storage operations with timing
#[macro_export]
macro_rules! storage_op {
    ($operation:expr, $key:expr, $result:expr, $elapsed_ms:expr) => {
        tracing::debug!(
            operation = $operation,
            key = $key,
            success = $result.is_ok(),
            elapsed_ms = $elapsed_ms,
            "Storage operation"
        )
    };
}
