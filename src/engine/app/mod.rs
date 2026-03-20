mod benchmark;
mod fps;
mod game_session;
mod runtime;
mod services;
mod spawn;
mod streaming;

use winit::{dpi::PhysicalSize, event::Event, event_loop::EventLoop, window::WindowBuilder};

use crate::config::Config;

use self::runtime::AppRuntime;

pub async fn run(config: Config) -> anyhow::Result<()> {
    let window_config = config.window;
    let event_loop = EventLoop::new()?;
    let mut runtime = AppRuntime::new(
        WindowBuilder::new()
            .with_title("Decaf")
            .with_inner_size(PhysicalSize::new(window_config.width, window_config.height))
            .build(&event_loop)?
            .into(),
        config,
    )
    .await?;
    runtime.capture_cursor();
    runtime.request_redraw();

    event_loop.run(move |event, event_loop_target| match event {
        Event::NewEvents(_) => {
            runtime.begin_frame();
        }
        Event::DeviceEvent { event, .. } => {
            runtime.handle_device_event(&event);
        }
        Event::WindowEvent { event, .. } => {
            runtime.handle_window_event(event, event_loop_target);
        }
        Event::AboutToWait => {
            runtime.handle_about_to_wait(event_loop_target);
        }
        _ => {}
    })?;

    #[allow(unreachable_code)]
    Ok(())
}

pub async fn run_meshing_upload_benchmark(config: Config) -> anyhow::Result<()> {
    benchmark::run_meshing_upload(config).await
}
