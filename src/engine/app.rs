use std::sync::Arc;

use ahash::AHashSet;
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
            gpu_types::{DebugOverlayInput, DebugViewMode},
            materials::create_texture_registry,
            meshing::{MeshingFocus, sort_chunk_coords_by_priority},
            renderer::Renderer,
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

struct FpsCounter {
    accumulated_time: f32,
    accumulated_frames: u32,
    displayed_fps: u32,
}

impl FpsCounter {
    fn new() -> Self {
        Self { accumulated_time: 0.0, accumulated_frames: 0, displayed_fps: 0 }
    }

    fn sample(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }

        self.accumulated_time += dt;
        self.accumulated_frames += 1;
        self.displayed_fps = (self.accumulated_frames as f32
            / self.accumulated_time.max(f32::EPSILON))
        .round() as u32;

        if self.accumulated_time >= 0.25 {
            self.accumulated_time = 0.0;
            self.accumulated_frames = 0;
        }
    }

    fn displayed_fps(&self) -> u32 {
        self.displayed_fps
    }
}

pub async fn run(config: Config) -> anyhow::Result<()> {
    let player_config = config.player.clone();
    let render_radius_xz = config.render.render_radius_xz.max(0);
    let render_radius_y = config.render.render_radius_y.max(0);
    let stream_generation_budget = config.render.stream_generation_budget;
    let enable_hiz_occlusion = config.render.enable_hiz_occlusion;
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

    let mut player = Player::from_config(&player_config);
    let initial_focus = focus_from_player(&player);
    let mut world = World::new();

    stream_chunks_around_player(
        &mut world,
        None,
        &generator,
        initial_focus,
        render_radius_xz,
        render_radius_y,
        0,
    );

    let mut renderer =
        Renderer::new(window.clone(), resolved_blocks, &texture_registry, enable_hiz_occlusion)
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
                let player_voxel = player.position.floor().as_ivec3();
                let player_chunk = ChunkCoord::from_world_voxel(player_voxel).0;
                renderer.set_debug_overlay(show_fps_overlay.then_some(DebugOverlayInput {
                    fps: fps_counter.displayed_fps(),
                    loaded_chunks: world.chunks.len() as u32,
                    player_voxel: [player_voxel.x, player_voxel.y, player_voxel.z],
                    player_chunk: [player_chunk.x, player_chunk.y, player_chunk.z],
                }));

                renderer.render(&camera).unwrap();
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
            }

            let focus = focus_from_player(&player);
            stream_chunks_around_player(
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

fn focus_from_player(player: &Player) -> MeshingFocus {
    MeshingFocus::new(
        ChunkCoord::from_world_voxel(player.position.floor().as_ivec3()),
        player.forward_3d(),
    )
}

fn stream_chunks_around_player(
    world: &mut World,
    mut renderer: Option<&mut Renderer>,
    generator: &FlatGenerator,
    focus: MeshingFocus,
    render_radius_xz: i32,
    render_radius_y: i32,
    generation_budget: usize,
) {
    let mut desired = desired_chunk_coords(focus, render_radius_xz, render_radius_y);
    let desired_set: AHashSet<_> = desired.iter().copied().collect();

    let to_unload: Vec<_> =
        world.chunks.keys().copied().filter(|coord| !desired_set.contains(coord)).collect();
    let unloaded = to_unload.len();

    for coord in to_unload {
        world.remove_chunk(coord);
        if let Some(renderer) = renderer.as_deref_mut() {
            renderer.remove_chunk_mesh(coord);
        }
    }

    let mut generated = 0usize;
    let budget = if generation_budget == 0 { usize::MAX } else { generation_budget };

    desired.retain(|coord| !world.contains_chunk(*coord));
    for coord in desired.into_iter().take(budget) {
        let mut chunk = Chunk::new();
        generator.generate(coord, &mut chunk);
        world.insert_chunk(coord, chunk);
        generated += 1;
    }

    if generated > 0 || unloaded > 0 {
        crate::log_debug!(
            "Streaming world around {:?}: generated {}, unloaded {}, loaded {}",
            focus.center.0,
            generated,
            unloaded,
            world.chunks.len()
        );
    }
}

fn desired_chunk_coords(
    focus: MeshingFocus,
    render_radius_xz: i32,
    render_radius_y: i32,
) -> Vec<ChunkCoord> {
    let mut coords = Vec::new();

    for cz in -render_radius_xz..=render_radius_xz {
        for cy in -render_radius_y..=render_radius_y {
            for cx in -render_radius_xz..=render_radius_xz {
                coords.push(ChunkCoord(focus.center.0 + IVec3::new(cx, cy, cz)));
            }
        }
    }

    sort_chunk_coords_by_priority(&mut coords, focus);
    coords
}
