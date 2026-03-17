use std::sync::Arc;

use winit::{
    dpi::PhysicalSize,
    event::*,
    event_loop::EventLoop,
    keyboard::KeyCode,
    window::{CursorGrabMode, Window, WindowBuilder},
};

use crate::{
    config::Config,
    engine::{
        core::{
            math::IVec3,
            types::{WINDOW_HEIGHT, WINDOW_WIDTH},
        },
        input::InputState,
        player::{
            controller::{Player, camera_from_player},
            physics::update_player,
        },
        render::{
            gpu_types::DebugViewMode, materials::create_texture_registry, renderer::Renderer,
        },
        world::{
            block::{create_default_block_registry, resolved::ResolvedBlockRegistry},
            chunk::Chunk,
            coord::ChunkCoord,
            generator::{ChunkGenerator, FlatGenerator},
            storage::World,
        },
    },
    logging,
};

pub async fn run(config: Config) -> anyhow::Result<()> {
    let player_config = config.player.clone();
    let render_radius_xz = config.render.render_radius_xz.max(0);
    let render_radius_y = config.render.render_radius_y.max(0);
    let block_registry = create_default_block_registry();
    let texture_registry = create_texture_registry(&block_registry);
    let resolved_blocks =
        ResolvedBlockRegistry::build(&block_registry, texture_registry.layer_map());
    let generator = FlatGenerator::new(
        block_registry.must_get_id("grass"),
        block_registry.must_get_id("dirt"),
        block_registry.must_get_id("stone"),
    );

    let event_loop = EventLoop::new()?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("Decaf")
            .with_inner_size(PhysicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT))
            .build(&event_loop)?,
    );

    let mut world = World::new();

    for cz in -render_radius_xz..=render_radius_xz {
        for cy in -render_radius_y..=render_radius_y {
            for cx in -render_radius_xz..=render_radius_xz {
                let coord = ChunkCoord(IVec3::new(cx, cy, cz));
                let mut chunk = Chunk::new();
                generator.generate(coord, &mut chunk);
                world.insert_chunk(coord, chunk);
            }
        }
    }

    let mut renderer = Renderer::new(window.clone(), resolved_blocks, &texture_registry).await?;
    renderer.finish_meshing(&mut world)?;

    let mut input = InputState::new();
    let mut player = Player::from_config(&player_config);
    let mut total_time = 0.0f32;

    grab_cursor(&window, &mut input);

    window.request_redraw();

    event_loop.run(move |event, elwt| match event {
        Event::NewEvents(_) => {
            input.begin_frame();
            total_time += input.dt;
        }

        Event::DeviceEvent { event, .. } => {
            input.handle_device_event(&event);
        }

        Event::WindowEvent { event, .. } => {
            match &event {
                WindowEvent::CloseRequested => elwt.exit(),

                WindowEvent::Resized(size) => {
                    renderer.resize(size.width.max(1), size.height.max(1));
                }

                WindowEvent::Focused(false) => {
                    release_cursor(&window, &mut input);
                }

                _ => {}
            }

            input.handle_window_event(&event);

            if let WindowEvent::KeyboardInput { .. } = &event {
                if input.key_pressed(KeyCode::Escape) {
                    if input.cursor_grabbed {
                        release_cursor(&window, &mut input);
                    } else {
                        grab_cursor(&window, &mut input);
                    }
                }

                let debug_mode = if input.key_pressed(KeyCode::Digit1) {
                    Some(DebugViewMode::Shaded)
                } else if input.key_pressed(KeyCode::Digit2) {
                    Some(DebugViewMode::FaceDir)
                } else if input.key_pressed(KeyCode::Digit3) {
                    Some(DebugViewMode::ChunkCoord)
                } else if input.key_pressed(KeyCode::Digit4) {
                    Some(DebugViewMode::DrawId)
                } else {
                    None
                };

                if let Some(debug_mode) = debug_mode {
                    renderer.set_debug_view_mode(debug_mode);
                    crate::log_info!("Render debug view: {}", debug_mode.label());
                    window.request_redraw();
                }
            }

            if let WindowEvent::RedrawRequested = event {
                let aspect = renderer.config.width as f32 / renderer.config.height as f32;
                let camera = camera_from_player(&player, aspect);

                renderer.render(&camera).unwrap();
                logging::frame_mark();
            }
        }

        Event::AboutToWait => {
            if let Err(err) = renderer.pump_meshing(&mut world) {
                crate::log_error!("failed to process chunk meshing jobs: {err:#}");
                elwt.exit();
                return;
            }

            if input.cursor_grabbed {
                update_player(
                    &mut player,
                    &input,
                    &world,
                    &renderer.resolved_blocks,
                    total_time,
                    &player_config,
                );
            }

            window.request_redraw();
        }

        _ => {}
    })?;

    #[allow(unreachable_code)]
    Ok(())
}

fn grab_cursor(window: &Window, input: &mut InputState) {
    let _ = window
        .set_cursor_grab(CursorGrabMode::Locked)
        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
    window.set_cursor_visible(false);
    input.cursor_grabbed = true;
}

fn release_cursor(window: &Window, input: &mut InputState) {
    let _ = window.set_cursor_grab(CursorGrabMode::None);
    window.set_cursor_visible(true);
    input.cursor_grabbed = false;
}
