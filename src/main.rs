mod config;
mod logging;

fn main() {
    logging::init_logging();

    tracing::info!("Engine starting");
    tracing::debug!("Debug logging enabled if RUST_LOG allows it");
}
