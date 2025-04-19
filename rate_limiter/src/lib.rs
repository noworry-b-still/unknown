// library entry
pub mod error;
pub mod logging;

// Re-export key components for convenience
pub use error::{RateLimiterError, Result};
pub use logging::init as init_logging;
