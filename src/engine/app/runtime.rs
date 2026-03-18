use std::sync::Arc;

use winit::{
    event::{DeviceEvent, MouseButton, WindowEvent},
    event_loop::EventLoopWindowTarget,
    keyboard::KeyCode,
    window::{CursorGrabMode, Window},
};

use crate::{
    config::{Config, PlayerConfig, RenderConfig},
    engine::{
        input::InputState,
        player::{
            interaction::{place_block_in_front, remove_block_in_front},
            physics::update_player,
            state::{Player, camera_from_player},
        },
        render::{
            gpu_types::{DebugOverlayInput, DebugViewMode},
            materials::create_texture_registry,
            renderer::Renderer,
        },
        world::{
            biome::BiomeTable,
            block::{create_default_block_registry, id::BlockId, resolved::ResolvedBlockRegistry},
            generator::{ChunkGenerator, StagedGenerator},
            storage::World,
        },
    },
    logging,
};

use super::{
    fps::FpsCounter,
    spawn::spawn_position_near_world_origin,
    streaming::{WorldStreamer, meshing_focus_from_player},
};

pub(super) struct AppRuntime {
    render_config: RenderConfig,
    player_config: PlayerConfig,
    render_radius_xz: i32,
    render_radius_y: i32,
    generation_budget_per_frame: usize,
    staged_generator: Arc<StagedGenerator>,
    streamer: WorldStreamer,
    renderer: Renderer,
    player: Player,
    world: World,
    input: InputState,
    total_time: f32,
    fps_counter: FpsCounter,
    show_debug_overlay: bool,
    placement_block_id: BlockId,
}

impl AppRuntime {
    pub(super) async fn new(window: Arc<Window>, config: Config) -> anyhow::Result<Self> {
        let render_config = config.render;
        let player_config = config.player;
        let world_config = config.world;

        let render_radius_xz = render_config.render_radius_xz.max(0);
        let render_radius_y = render_config.render_radius_y.max(0);
        let generation_budget_per_frame = render_config.stream_generation_budget;
        let stream_max_inflight_generations = render_config.stream_max_inflight_generations;
        let (generation_worker_count, meshing_worker_count) =
            resolve_background_worker_counts(&render_config);

        let block_registry = create_default_block_registry();
        let placement_block_id = block_registry.must_get_id("stone");
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
        let spawn_position =
            spawn_position_near_world_origin(&staged_generator, &world_config, &player_config);
        let chunk_generator: Arc<dyn ChunkGenerator> = staged_generator.clone();
        let mut streamer = WorldStreamer::new(
            chunk_generator,
            generation_worker_count,
            stream_max_inflight_generations,
        );
        crate::log_info!(
            "Background workers: generation {}, meshing {}",
            generation_worker_count,
            meshing_worker_count
        );

        let mut player = Player::from_config(&player_config);
        player.position = spawn_position;
        let initial_focus = meshing_focus_from_player(&player);
        let mut world = World::new();

        streamer.finish_generation(
            &mut world,
            initial_focus,
            render_radius_xz,
            render_radius_y,
            0,
        )?;

        let mut renderer = Renderer::new(
            window,
            resolved_blocks,
            &texture_registry,
            &render_config,
            meshing_worker_count,
        )
        .await?;
        renderer.finish_meshing(&mut world, initial_focus)?;

        Ok(Self {
            render_config,
            player_config,
            render_radius_xz,
            render_radius_y,
            generation_budget_per_frame,
            staged_generator,
            streamer,
            renderer,
            player,
            world,
            input: InputState::new(),
            total_time: 0.0,
            fps_counter: FpsCounter::new(),
            show_debug_overlay: false,
            placement_block_id,
        })
    }

    pub(super) fn capture_cursor(&mut self, window: &Window) {
        let _ = window
            .set_cursor_grab(CursorGrabMode::Locked)
            .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
        window.set_cursor_visible(false);
        self.input.cursor_grabbed = true;
    }

    pub(super) fn begin_frame(&mut self) {
        self.input.begin_frame();
        self.total_time += self.input.dt;
        self.fps_counter.sample(self.input.dt);
    }

    pub(super) fn handle_device_event(&mut self, event: &DeviceEvent) {
        self.input.handle_device_event(event);
    }

    pub(super) fn handle_window_event(
        &mut self,
        window: &Window,
        event: WindowEvent,
        event_loop_target: &EventLoopWindowTarget<()>,
    ) {
        match &event {
            WindowEvent::CloseRequested => event_loop_target.exit(),
            WindowEvent::Resized(size) => {
                self.renderer.resize(size.width.max(1), size.height.max(1));
            }
            WindowEvent::Focused(false) => {
                self.release_cursor(window);
            }
            _ => {}
        }

        self.input.handle_window_event(&event);

        if matches!(event, WindowEvent::KeyboardInput { .. }) {
            self.handle_keyboard_input(window);
        }

        if matches!(event, WindowEvent::RedrawRequested) {
            self.render_frame(event_loop_target);
        }
    }

    pub(super) fn handle_about_to_wait(
        &mut self,
        window: &Window,
        event_loop_target: &EventLoopWindowTarget<()>,
    ) {
        if self.input.cursor_grabbed {
            self.update_player_and_interactions();
        }

        if let Err(err) = self.update_world_streaming() {
            crate::log_error!("failed to advance world state: {err:#}");
            event_loop_target.exit();
            return;
        }

        window.request_redraw();
    }

    fn release_cursor(&mut self, window: &Window) {
        let _ = window.set_cursor_grab(CursorGrabMode::None);
        window.set_cursor_visible(true);
        self.input.cursor_grabbed = false;
    }

    fn handle_keyboard_input(&mut self, window: &Window) {
        if self.input.key_pressed(KeyCode::Escape) {
            if self.input.cursor_grabbed {
                self.release_cursor(window);
            } else {
                self.capture_cursor(window);
            }
        }

        if self.input.key_pressed(KeyCode::F3) {
            self.show_debug_overlay = !self.show_debug_overlay;
            window.request_redraw();
        }

        if let Some(debug_view_mode) = selected_debug_view_mode(&self.input) {
            self.renderer.set_debug_view_mode(debug_view_mode);
            crate::log_info!("Render debug view: {}", debug_view_mode.label());
            window.request_redraw();
        }
    }

    fn render_frame(&mut self, event_loop_target: &EventLoopWindowTarget<()>) {
        let aspect_ratio =
            self.renderer.surface_config.width as f32 / self.renderer.surface_config.height as f32;
        let camera = camera_from_player(
            &self.player,
            aspect_ratio,
            &self.render_config.camera,
            self.zoom_active(),
        );
        let debug_overlay = self.show_debug_overlay.then(|| self.build_debug_overlay_input());

        self.renderer.set_debug_overlay(debug_overlay);

        if let Err(err) = self.renderer.render(&camera) {
            crate::log_error!("render failed: {err:#}");
            event_loop_target.exit();
            return;
        }

        logging::frame_mark();
    }

    fn build_debug_overlay_input(&self) -> DebugOverlayInput {
        let player_voxel = self.player.position.floor().as_ivec3();
        let player_chunk =
            crate::engine::world::coord::ChunkCoord::from_world_voxel(player_voxel).0;
        let terrain_debug = self.staged_generator.debug_sample_at(player_voxel.x, player_voxel.z);

        DebugOverlayInput {
            fps: self.fps_counter.displayed_fps(),
            loaded_chunks: self.world.chunks.len() as u32,
            player_voxel: [player_voxel.x, player_voxel.y, player_voxel.z],
            player_chunk: [player_chunk.x, player_chunk.y, player_chunk.z],
            player_facing: self.player.cardinal_facing(),
            biome_name: terrain_debug.biome_name,
            biome_priority: terrain_debug.biome_priority,
            region_name: terrain_debug.region_name,
            ground_y: terrain_debug.ground_y,
            biome_altitude_y: terrain_debug.biome_altitude_y,
            temperature_percent: terrain_debug.temperature_percent,
            humidity_percent: terrain_debug.humidity_percent,
            continentalness_percent: terrain_debug.continentalness_percent,
            biome_temperature_min_percent: terrain_debug.biome_temperature_min_percent,
            biome_temperature_max_percent: terrain_debug.biome_temperature_max_percent,
            biome_humidity_min_percent: terrain_debug.biome_humidity_min_percent,
            biome_humidity_max_percent: terrain_debug.biome_humidity_max_percent,
            biome_altitude_min: terrain_debug.biome_altitude_min,
            biome_altitude_max: terrain_debug.biome_altitude_max,
            biome_continentalness_min_percent: terrain_debug.biome_continentalness_min_percent,
            biome_continentalness_max_percent: terrain_debug.biome_continentalness_max_percent,
        }
    }

    fn update_player_and_interactions(&mut self) {
        let zoom_active = self.zoom_active();
        update_player(
            &mut self.player,
            &self.input,
            &self.world,
            &self.renderer.resolved_blocks,
            self.total_time,
            &self.player_config,
            zoom_active,
        );

        let interaction_origin = self.player.eye_position();
        let interaction_direction = self.player.forward_3d();

        if self.input.mouse_pressed(MouseButton::Left)
            && remove_block_in_front(
                &mut self.world,
                &self.renderer.resolved_blocks,
                interaction_origin,
                interaction_direction,
                self.player_config.reach_distance,
            )
        {
            crate::log_debug!("Removed block");
        }

        if self.input.mouse_pressed(MouseButton::Right)
            && place_block_in_front(
                &mut self.world,
                &self.renderer.resolved_blocks,
                &self.player,
                interaction_origin,
                interaction_direction,
                self.player_config.reach_distance,
                self.placement_block_id,
            )
        {
            crate::log_debug!("Placed stone block");
        }
    }

    fn update_world_streaming(&mut self) -> anyhow::Result<()> {
        let meshing_focus = meshing_focus_from_player(&self.player);
        self.streamer.pump(
            &mut self.world,
            Some(&mut self.renderer),
            meshing_focus,
            self.render_radius_xz,
            self.render_radius_y,
            self.generation_budget_per_frame,
        )?;
        self.renderer.pump_meshing(&mut self.world, meshing_focus)?;
        Ok(())
    }

    fn zoom_active(&self) -> bool {
        self.input.cursor_grabbed && self.input.key_held(KeyCode::KeyC)
    }
}

fn resolve_background_worker_counts(render_config: &RenderConfig) -> (usize, usize) {
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

fn selected_debug_view_mode(input: &InputState) -> Option<DebugViewMode> {
    if input.key_pressed(KeyCode::Digit1) {
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
    }
}
