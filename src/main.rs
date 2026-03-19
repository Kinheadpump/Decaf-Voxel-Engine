#![forbid(unsafe_op_in_unsafe_fn)]
#![deny(unused_must_use)]
#![warn(clippy::dbg_macro, clippy::todo, clippy::unwrap_used, clippy::undocumented_unsafe_blocks)]

mod config;
mod engine;
mod logging;

fn main() -> anyhow::Result<()> {
    let command = Command::parse(std::env::args().skip(1))?;
    let config = config::Config::load();
    let _runtime_services = logging::init(&config.debug);

    crate::log_info!("Engine starting");
    crate::log_debug!("Runtime config loaded successfully");

    match command {
        Command::RunApp => pollster::block_on(engine::app::run(config)),
        Command::BenchmarkMeshingUpload => {
            crate::log_info!("Running meshing/upload benchmark");
            pollster::block_on(engine::app::run_meshing_upload_benchmark(config))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    RunApp,
    BenchmarkMeshingUpload,
}

impl Command {
    fn parse(args: impl Iterator<Item = String>) -> anyhow::Result<Self> {
        let args: Vec<_> = args.collect();
        match args.as_slice() {
            [] => Ok(Self::RunApp),
            [flag] if flag == "--benchmark" => Ok(Self::BenchmarkMeshingUpload),
            [flag, name] if flag == "--benchmark" && name == "meshing-upload" => {
                Ok(Self::BenchmarkMeshingUpload)
            }
            _ => anyhow::bail!(
                "unrecognized arguments {:?}\nusage: decaf [--benchmark [meshing-upload]]",
                args
            ),
        }
    }
}
