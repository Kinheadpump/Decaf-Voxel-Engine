mod config;
mod logging;

use config::DebugConfig;

fn main() {
    let config = DebugConfig::load();
    logging::init_logging();
    tracing::info!("Engine starting");
    tracing::debug!("Debug logging enabled if RUST_LOG allows it");
}
