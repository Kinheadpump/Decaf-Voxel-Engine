mod fps;
mod streaming;

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
        input::InputState,
        player::{
            controller::{Player, camera_from_player},
            interaction::{place_block_in_front, remove_block_in_front},
            physics::update_player,
        },
        render::{
            gpu_types::{DebugOverlayInput, DebugViewMode},
            materials::create_texture_registry,
            renderer::Renderer,
        },
        world::{
            block::{create_default_block_registry, resolved::ResolvedBlockRegistry},
            generator::FlatGenerator,
            storage::World,
        },
    },
    logging,
};

use self::{
    fps::FpsCounter,
    streaming::{meshing_focus_from_player, stream_chunks_around_focus},
};

pub async fn run(config: Config) -> anyhow::Result<()> {
    let window_config = config.window;
    let render_config = config.render.clone();
    let player_config = config.player;
    let world_config = config.world;

    let render_radius_xz = render_config.render_radius_xz.max(0);
    let render_radius_y = render_config.render_radius_y.max(0);
    let stream_generation_budget = render_config.stream_generation_budget;

    let block_registry = create_default_block_registry();
    let stone_block_id = block_registry.must_get_id("stone");
    let texture_registry = create_texture_registry(&block_registry);
    let resolved_blocks =
        ResolvedBlockRegistry::build(&block_registry, texture_registry.layer_map());
    let generator = FlatGenerator::new(
        block_registry.must_get_id("grass"),
        block_registry.must_get_id("dirt"),
        block_registry.must_get_id("stone"),
        world_config.surface_level,
        world_config.soil_depth,
    );

    let event_loop = EventLoop::new()?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("Decaf")
            .with_inner_size(PhysicalSize::new(window_config.width, window_config.height))
            .build(&event_loop)?,
    );

    let mut player = Player::from_config(&player_config);
    let initial_focus = meshing_focus_from_player(&player);
    let mut world = World::new();

    stream_chunks_around_focus(
        &mut world,
        None,
        &generator,
        initial_focus,
        render_radius_xz,
        render_radius_y,
        0,
    );

    let mut renderer =
        Renderer::new(window.clone(), resolved_blocks, &texture_registry, &render_config).await?;
    renderer.finish_meshing(&mut world, initial_focus)?;

    let mut input = InputState::new();
    let mut total_time = 0.0f32;
    let mut fps_counter = FpsCounter::new();
    let mut show_fps_overlay = false;

    grab_cursor(&window, &mut input);
    window.request_redraw();

    event_loop.run(move |event, elwt| match event {
        Event::NewEvents(_) => {
            input.begin_frame();
            total_time += input.dt;
            fps_counter.sample(input.dt);
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

                if input.key_pressed(KeyCode::F3) {
                    show_fps_overlay = !show_fps_overlay;
                    window.request_redraw();
                }

                let debug_mode = if input.key_pressed(KeyCode::Digit1) {
                    Some(DebugViewMode::Shaded)
                } else if input.key_pressed(KeyCode::Digit2) {
                    Some(DebugViewMode::FaceDir)
                } else if input.key_pressed(KeyCode::Digit3) {
                    Some(DebugViewMode::ChunkCoord)
                } else if input.key_pressed(KeyCode::Digit4) {
                    Some(DebugViewMode::DrawId)
                } else if input.key_pressed(KeyCode::Digit5) {
                    Some(DebugViewMode::Wireframe)
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
                let aspect =
                    renderer.surface_config.width as f32 / renderer.surface_config.height as f32;
                let camera = camera_from_player(&player, aspect, &render_config.camera);
                let player_voxel = player.position.floor().as_ivec3();
                let player_chunk =
                    crate::engine::world::coord::ChunkCoord::from_world_voxel(player_voxel).0;
                renderer.set_debug_overlay(show_fps_overlay.then_some(DebugOverlayInput {
                    fps: fps_counter.displayed_fps(),
                    loaded_chunks: world.chunks.len() as u32,
                    player_voxel: [player_voxel.x, player_voxel.y, player_voxel.z],
                    player_chunk: [player_chunk.x, player_chunk.y, player_chunk.z],
                    player_facing: player.cardinal_facing(),
                }));

                if let Err(err) = renderer.render(&camera) {
                    crate::log_error!("render failed: {err:#}");
                    elwt.exit();
                    return;
                }

                logging::frame_mark();
            }
        }

        Event::AboutToWait => {
            if input.cursor_grabbed {
                update_player(
                    &mut player,
                    &input,
                    &world,
                    &renderer.resolved_blocks,
                    total_time,
                    &player_config,
                );

                let interaction_origin = player.eye_position();
                let interaction_direction = player.forward_3d();

                if input.mouse_pressed(MouseButton::Left)
                    && remove_block_in_front(
                        &mut world,
                        &renderer.resolved_blocks,
                        interaction_origin,
                        interaction_direction,
                        player_config.reach_distance,
                    )
                {
                    crate::log_debug!("Removed block");
                }

                if input.mouse_pressed(MouseButton::Right)
                    && place_block_in_front(
                        &mut world,
                        &renderer.resolved_blocks,
                        &player,
                        interaction_origin,
                        interaction_direction,
                        player_config.reach_distance,
                        stone_block_id,
                    )
                {
                    crate::log_debug!("Placed stone block");
                }
            }

            let focus = meshing_focus_from_player(&player);
            stream_chunks_around_focus(
                &mut world,
                Some(&mut renderer),
                &generator,
                focus,
                render_radius_xz,
                render_radius_y,
                stream_generation_budget,
            );

            if let Err(err) = renderer.pump_meshing(&mut world, focus) {
                crate::log_error!("failed to process chunk meshing jobs: {err:#}");
                elwt.exit();
                return;
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
