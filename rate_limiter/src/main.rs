use dotenv::dotenv;
use rate_limiter::init_logging;
use tracing::{debug, error, info, warn};

fn main() {
    dotenv().ok();
    init_logging();
    info!("Rate limiter starting up!!");

    // start Rest of your application logic
    println!("Hello, world!");
    // end

    debug!("Configuration loaded");

    // Example of different log levels
    if true
    /* some condition */
    {
        warn!("Potentially concerning situation");
    }

    info!(
        tenant_id = "example_tenant",
        operation = "startup",
        "Rate limiter initialized succesfully"
    );
}
