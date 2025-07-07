#![deny(warnings)]
#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(dead_code)] //TODO: temporary

mod config;
mod error;
mod protocol;
mod util;

use config::Config;
use util::logging::init_logging;

use tracing::info;

#[tokio::main]
async fn main() {
    init_logging();
    let config = Config::default();

    info!(config.port, "Kerma node starting up ");
}
