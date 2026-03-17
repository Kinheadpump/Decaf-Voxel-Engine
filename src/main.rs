#![forbid(unsafe_op_in_unsafe_fn)]
#![deny(unused_must_use)]
#![warn(clippy::dbg_macro, clippy::todo, clippy::unwrap_used, clippy::undocumented_unsafe_blocks)]

mod config;
mod engine;
mod logging;

fn main() {
    logging::init_logging();
    tracing::info!("Engine starting");
    pollster::block_on(engine::app::run()).unwrap();
}
