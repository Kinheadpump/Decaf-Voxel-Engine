#![forbid(unsafe_op_in_unsafe_fn)]
#![deny(unused_must_use)]
#![warn(
    clippy::dbg_macro,
    clippy::todo,
    clippy::unwrap_used,
    clippy::undocumented_unsafe_blocks
)]

mod config;
mod logging;

use config::Config;

fn main() {
    let config = Config::load();
    logging::init_logging();
    tracing::info!("Engine starting");
    tracing::debug!("Debug logging enabled if RUST_LOG allows it");
}
