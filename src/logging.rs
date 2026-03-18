use crate::config::DebugConfig;
use tracing_subscriber::{EnvFilter, fmt};

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        tracing::info!($($arg)*)
    };
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        tracing::debug!($($arg)*)
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        tracing::warn!($($arg)*)
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        tracing::error!($($arg)*)
    };
}

#[macro_export]
macro_rules! profile_span {
    ($name:expr) => {
        ::tracy_client::Client::running()
            .map(|client| client.span_alloc(Some($name), module_path!(), file!(), line!(), 0))
    };
}

pub struct RuntimeServices {
    _tracy: Option<TracyProfiler>,
}

struct TracyProfiler {
    client: tracy_client::Client,
}

impl Drop for TracyProfiler {
    fn drop(&mut self) {
        let _ = &self.client;

        // SAFETY: The profiler is started and shut down from the main thread.
        // `RuntimeServices` lives in `main`, so it drops only after the app has returned
        // and the renderer/threaded mesher have already been dropped and joined.
        unsafe {
            tracy_client::sys::___tracy_shutdown_profiler();
        }
    }
}

pub fn init(debug: &DebugConfig) -> RuntimeServices {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("decaf=info,wgpu=warn,wgpu_core=warn,wgpu_hal=warn,naga=warn")
    });

    fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .compact()
        .init();

    let tracy = if debug.enable_profiler {
        let client = tracy_client::Client::start();
        client.set_thread_name("main");
        client.message("Tracy profiler enabled", 0);
        tracing::info!("Tracy profiler enabled");
        Some(TracyProfiler { client })
    } else {
        tracing::debug!("Tracy profiler disabled");
        None
    };

    RuntimeServices { _tracy: tracy }
}

pub fn frame_mark() {
    if let Some(client) = tracy_client::Client::running() {
        client.frame_mark();
    }
}
