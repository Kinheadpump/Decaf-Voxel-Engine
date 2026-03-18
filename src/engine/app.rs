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
    config::{Config, PlayerConfig, RenderConfig},
    engine::{
        core::math::Vec3,
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
            biome::BiomeTable,
            block::{create_default_block_registry, resolved::ResolvedBlockRegistry},
            generator::{ChunkGenerator, StagedGenerator},
            storage::World,
        },
    },
    logging,
};

use self::{
    fps::FpsCounter,
    streaming::{WorldStreamer, meshing_focus_from_player},
};

pub async fn run(config: Config) -> anyhow::Result<()> {
    let window_config = config.window;
    let render_config = config.render.clone();
    let player_config = config.player;
    let world_config = config.world;

    let render_radius_xz = render_config.render_radius_xz.max(0);
    let render_radius_y = render_config.render_radius_y.max(0);
    let stream_generation_budget = render_config.stream_generation_budget;
    let stream_max_inflight_generations = render_config.stream_max_inflight_generations;
    let (generation_worker_count, meshing_worker_count) = background_worker_counts(&render_config);

    let block_registry = create_default_block_registry();
    let stone_block_id = block_registry.must_get_id("stone");
    let water_block_id = block_registry.must_get_id("water");
    let texture_registry = create_texture_registry(&block_registry);
    let resolved_blocks =
        ResolvedBlockRegistry::build(&block_registry, texture_registry.layer_map());
    let biomes = BiomeTable::load_from_file(&world_config.biomes_file, &block_registry)?;
    let staged_generator = Arc::new(StagedGenerator::new(
        world_config.seed,
        water_block_id,
        world_config.terrain,
        biomes,
    ));
    let spawn_position = spawn_position_at_world_origin(&staged_generator, &player_config);
    let generator: Arc<dyn ChunkGenerator> = staged_generator.clone();
    let mut streamer =
        WorldStreamer::new(generator, generation_worker_count, stream_max_inflight_generations);
    crate::log_info!(
        "Background workers: generation {}, meshing {}",
        generation_worker_count,
        meshing_worker_count
    );

    let event_loop = EventLoop::new()?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("Decaf")
            .with_inner_size(PhysicalSize::new(window_config.width, window_config.height))
            .build(&event_loop)?,
    );

    let mut player = Player::from_config(&player_config);
    player.position = spawn_position;
    let initial_focus = meshing_focus_from_player(&player);
    let mut world = World::new();

    streamer.finish_generation(&mut world, initial_focus, render_radius_xz, render_radius_y, 0)?;

    let mut renderer = Renderer::new(
        window.clone(),
        resolved_blocks,
        &texture_registry,
        &render_config,
        meshing_worker_count,
    )
    .await?;
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
                renderer.set_debug_overlay(show_fps_overlay.then(|| {
                    let terrain_debug =
                        staged_generator.debug_sample_at(player_voxel.x, player_voxel.z);

                    DebugOverlayInput {
                        fps: fps_counter.displayed_fps(),
                        loaded_chunks: world.chunks.len() as u32,
                        player_voxel: [player_voxel.x, player_voxel.y, player_voxel.z],
                        player_chunk: [player_chunk.x, player_chunk.y, player_chunk.z],
                        player_facing: player.cardinal_facing(),
                        biome_name: terrain_debug.biome_name,
                        region_name: terrain_debug.region_name,
                        surface_y: terrain_debug.surface_y,
                        temperature_percent: terrain_debug.temperature_percent,
                        humidity_percent: terrain_debug.humidity_percent,
                    }
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
            if let Err(err) = streamer.pump(
                &mut world,
                Some(&mut renderer),
                focus,
                render_radius_xz,
                render_radius_y,
                stream_generation_budget,
            ) {
                crate::log_error!("failed to process chunk generation jobs: {err:#}");
                elwt.exit();
                return;
            }

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

fn background_worker_counts(render_config: &RenderConfig) -> (usize, usize) {
    let available_workers = std::thread::available_parallelism()
        .map(|count| count.get().saturating_sub(1).max(1))
        .unwrap_or(1);
    let generation_worker_count = if render_config.generation_worker_count == 0 {
        (available_workers / 3).max(1)
    } else {
        render_config.generation_worker_count.max(1)
    };
    let meshing_worker_count = if render_config.meshing_worker_count == 0 {
        available_workers.saturating_sub(generation_worker_count).max(1)
    } else {
        render_config.meshing_worker_count.max(1)
    };

    (generation_worker_count, meshing_worker_count)
}

fn spawn_position_at_world_origin(
    generator: &StagedGenerator,
    player_config: &PlayerConfig,
) -> Vec3 {
    let spawn_x = 0.0;
    let spawn_z = 0.0;
    let min_x = (spawn_x - player_config.radius).floor() as i32;
    let max_x = (spawn_x + player_config.radius).floor() as i32;
    let min_z = (spawn_z - player_config.radius).floor() as i32;
    let max_z = (spawn_z + player_config.radius).floor() as i32;

    let support_top = (min_z..=max_z)
        .flat_map(|world_z| {
            (min_x..=max_x).map(move |world_x| generator.top_occupied_y_at(world_x, world_z))
        })
        .max()
        .unwrap_or(generator.terrain.sea_level);

    Vec3::new(spawn_x, support_top as f32 + 1.0, spawn_z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TerrainConfig;
    use crate::engine::world::{biome::BiomeTable, block::id::BlockId};

    #[test]
    fn spawn_position_sits_above_highest_column_under_player() {
        let generator = StagedGenerator::new(
            12345,
            BlockId(4),
            TerrainConfig::default(),
            BiomeTable::single(BlockId(1), BlockId(2), BlockId(3)),
        );
        let player_config = PlayerConfig { radius: 0.3, ..PlayerConfig::default() };
        let spawn = spawn_position_at_world_origin(&generator, &player_config);

        let mut expected_support = i32::MIN;
        for world_z in -1..=0 {
            for world_x in -1..=0 {
                expected_support =
                    expected_support.max(generator.top_occupied_y_at(world_x, world_z));
            }
        }

        assert_eq!(spawn.x, 0.0);
        assert_eq!(spawn.z, 0.0);
        assert_eq!(spawn.y, expected_support as f32 + 1.0);
    }
}
