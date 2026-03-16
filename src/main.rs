mod config;
mod logging;

use config::Config;

fn main() {
    let config = Config::load();
    logging::init_logging();
    tracing::info!("Engine starting");
    tracing::debug!("Debug logging enabled if RUST_LOG allows it");
}
