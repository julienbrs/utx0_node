// use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

pub fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env()) // uses RUST_LOG
        .with_target(true) // shows the module path
        .with_level(true) // shows log level
        .init();
}
