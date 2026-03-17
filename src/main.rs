#![forbid(unsafe_op_in_unsafe_fn)]
#![deny(unused_must_use)]
#![warn(clippy::dbg_macro, clippy::todo, clippy::unwrap_used, clippy::undocumented_unsafe_blocks)]

mod config;
mod engine;
mod logging;

fn main() {
    let config = config::Config::load();
    let _runtime_services = logging::init(&config.debug);

    crate::log_info!("Engine starting");
    crate::log_debug!("Runtime config loaded successfully");
    pollster::block_on(engine::app::run(config)).unwrap();
}
