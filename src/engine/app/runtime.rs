use std::sync::Arc;

use winit::{
    event::{DeviceEvent, WindowEvent},
    event_loop::EventLoopWindowTarget,
    keyboard::KeyCode,
    window::{CursorGrabMode, Window},
};

use crate::{
    config::{Config, RenderConfig, SimulationConfig},
    engine::{
        input::{InputState, SimulationInput, SimulationInputBuffer},
        player::{
            editing::{HOTBAR_SLOT_COUNT, PlayerEditState},
            state::{Player, camera_from_player},
        },
        render::{
            gpu_types::{DebugOverlayInput, DebugViewMode},
            materials::{HudTextureRegistry, create_hud_texture_registry, create_texture_registry},
            renderer::Renderer,
        },
        world::{
            biome::BiomeTable,
            block::{create_default_block_registry, resolved::ResolvedBlockRegistry},
            generator::{ChunkGenerator, StagedGenerator},
            persistence,
            storage::World,
        },
    },
    logging,
};

use super::{
    fps::FpsCounter,
    game_session::{GameRules, GameSession, default_hotbar_slots},
    services::{
        BackgroundServices, GenerationService, PersistenceService, resolve_background_worker_counts,
    },
    spawn::spawn_position_near_world_origin,
    streaming::{WorldStreamer, meshing_focus_from_player},
};

pub(super) struct AppRuntime {
    window: Arc<Window>,
    render_config: RenderConfig,
    render_radius_xz: i32,
    render_radius_y: i32,
    generation_budget_per_frame: usize,
    completed_generation_budget_per_frame: usize,
    background: BackgroundServices,
    renderer: Renderer,
    session: GameSession,
    hud_texture_registry: HudTextureRegistry,
    input: InputState,
    pending_simulation_input: SimulationInputBuffer,
    simulation_clock: FixedSimulationClock,
    fps_counter: FpsCounter,
    show_debug_overlay: bool,
}

impl AppRuntime {
    pub(super) async fn new(window: Arc<Window>, config: Config) -> anyhow::Result<Self> {
        let render_config = config.render;
        let simulation_config = config.simulation;
        let player_config = config.player;
        let world_config = config.world;
        let save_file = persistence::save_path(&world_config.save_file);
        let save_context = persistence::WorldSaveContext::from_world_config(&world_config);

        let render_radius_xz = render_config.render_radius_xz.max(0);
        let render_radius_y = render_config.render_radius_y.max(0);
        let startup_preload_radius_xz =
            render_config.startup_preload_radius_xz.clamp(0, render_radius_xz);
        let startup_preload_radius_y =
            render_config.startup_preload_radius_y.clamp(0, render_radius_y);
        let generation_budget_per_frame = render_config.stream_generation_budget;
        let completed_generation_budget_per_frame = render_config.stream_completed_chunk_budget;
        let stream_max_inflight_generations = render_config.stream_max_inflight_generations;
        let (generation_worker_count, meshing_worker_count) =
            resolve_background_worker_counts(&render_config);

        let block_registry = create_default_block_registry();
        let water_block_id = block_registry.must_get_id("water");
        let inventory = PlayerEditState::new(default_hotbar_slots(&block_registry));
        let texture_registry = create_texture_registry(&block_registry);
        let hud_texture_registry = create_hud_texture_registry(&block_registry);
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
        let persistent_edits =
            persistence::load_block_edits(&save_file, &save_context, &block_registry)?;
        let persistence = PersistenceService::new(
            save_file.clone(),
            save_context.clone(),
            &block_registry,
            persistent_edits.clone(),
        )?;
        for (position, block_id) in persistent_edits.iter().copied() {
            world.load_persistent_edit_world(position, block_id);
        }
        if !persistent_edits.is_empty() {
            crate::log_info!(
                "Loaded {} persisted world edits from {}",
                persistent_edits.len(),
                save_file.display()
            );
        }

        streamer.finish_generation(
            &mut world,
            initial_focus,
            startup_preload_radius_xz,
            startup_preload_radius_y,
            0,
        )?;

        let mut renderer = Renderer::new(
            window.clone(),
            resolved_blocks,
            &texture_registry,
            &hud_texture_registry,
            &render_config,
            meshing_worker_count,
        )
        .await?;
        renderer.finish_meshing(&mut world, initial_focus)?;

        let session = GameSession::new(
            player,
            world,
            inventory,
            GameRules::new(player_config, water_block_id),
        );
        let background = BackgroundServices {
            generation: GenerationService { staged_generator, streamer },
            persistence,
        };

        Ok(Self {
            window,
            render_config,
            render_radius_xz,
            render_radius_y,
            generation_budget_per_frame,
            completed_generation_budget_per_frame,
            background,
            renderer,
            session,
            hud_texture_registry,
            input: InputState::new(),
            pending_simulation_input: SimulationInputBuffer::default(),
            simulation_clock: FixedSimulationClock::new(simulation_config),
            fps_counter: FpsCounter::new(),
            show_debug_overlay: false,
        })
    }

    pub(super) fn capture_cursor(&mut self) {
        let _ = self
            .window
            .set_cursor_grab(CursorGrabMode::Locked)
            .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined));
        self.window.set_cursor_visible(false);
        self.input.cursor_grabbed = true;
    }

    pub(super) fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub(super) fn begin_frame(&mut self) {
        self.input.begin_frame();
        self.fps_counter.sample(self.input.dt);
    }

    pub(super) fn handle_device_event(&mut self, event: &DeviceEvent) {
        self.input.handle_device_event(event);
    }

    pub(super) fn handle_window_event(
        &mut self,
        event: WindowEvent,
        event_loop_target: &EventLoopWindowTarget<()>,
    ) {
        match &event {
            WindowEvent::CloseRequested => {
                if let Err(err) = self.flush_pending_world_saves() {
                    crate::log_warn!(
                        "failed to flush world edits to {} before exit: {err:#}",
                        self.background.persistence.save_file().display()
                    );
                }
                event_loop_target.exit();
            }
            WindowEvent::Resized(size) => {
                self.renderer.resize(size.width.max(1), size.height.max(1));
            }
            WindowEvent::Focused(false) => {
                self.release_cursor();
            }
            _ => {}
        }

        self.input.handle_window_event(&event);

        if matches!(event, WindowEvent::KeyboardInput { .. }) {
            self.handle_keyboard_input();
        }

        if matches!(event, WindowEvent::RedrawRequested) {
            self.render_frame(event_loop_target);
        }
    }

    pub(super) fn handle_about_to_wait(&mut self, event_loop_target: &EventLoopWindowTarget<()>) {
        if self.input.cursor_grabbed {
            self.run_simulation_ticks();
        } else {
            self.pending_simulation_input.clear();
            self.simulation_clock.reset();
        }

        if let Err(err) = self.update_world_streaming() {
            crate::log_error!("failed to advance world state: {err:#}");
            event_loop_target.exit();
            return;
        }

        self.window.request_redraw();
    }

    fn release_cursor(&mut self) {
        let _ = self.window.set_cursor_grab(CursorGrabMode::None);
        self.window.set_cursor_visible(true);
        self.input.cursor_grabbed = false;
        self.pending_simulation_input.clear();
        self.simulation_clock.reset();
    }

    fn handle_keyboard_input(&mut self) {
        if self.input.key_pressed(KeyCode::Escape) {
            if self.input.cursor_grabbed {
                self.release_cursor();
            } else {
                self.capture_cursor();
            }
        }

        if self.input.key_pressed(KeyCode::F3) {
            self.show_debug_overlay = !self.show_debug_overlay;
            self.window.request_redraw();
        }

        let debug_modifier =
            self.input.key_held(KeyCode::ShiftLeft) || self.input.key_held(KeyCode::ShiftRight);

        if self.input.cursor_grabbed && !debug_modifier {
            for (slot, key) in hotbar_digit_keys().into_iter().enumerate() {
                if self.input.key_pressed(key) && self.session.select_hotbar_slot(slot) {
                    self.window.request_redraw();
                }
            }
        }

        if let Some(debug_view_mode) = selected_debug_view_mode(&self.input) {
            self.renderer.set_debug_view_mode(debug_view_mode);
            crate::log_info!("Render debug view: {}", debug_view_mode.label());
            self.window.request_redraw();
        }
    }

    fn render_frame(&mut self, event_loop_target: &EventLoopWindowTarget<()>) {
        let aspect_ratio =
            self.renderer.surface_config.width as f32 / self.renderer.surface_config.height as f32;
        let camera = camera_from_player(
            self.session.player(),
            aspect_ratio,
            &self.render_config.camera,
            self.zoom_active(),
        );
        let debug_overlay = Some(self.build_debug_overlay_input());

        self.renderer.set_debug_overlay(debug_overlay);
        self.renderer.set_underwater_tint_active(self.session.player_eye_in_water());

        if let Err(err) = self.renderer.render(&camera, self.session.simulation_time()) {
            crate::log_error!("render failed: {err:#}");
            event_loop_target.exit();
            return;
        }

        logging::frame_mark();
    }

    fn build_debug_overlay_input(&self) -> DebugOverlayInput {
        let player = self.session.player();
        let player_voxel = player.position.floor().as_ivec3();
        let player_chunk =
            crate::engine::world::coord::ChunkCoord::from_world_voxel(player_voxel).0;
        let terrain_debug = self
            .background
            .generation
            .staged_generator
            .debug_sample_at(player_voxel.x, player_voxel.z);
        let hotbar_icon_layers = self
            .session
            .hotbar_slots()
            .map(|block_id| self.hud_texture_registry.block_icon_layer(block_id));

        DebugOverlayInput {
            show_debug: self.show_debug_overlay,
            show_game_hud: self.input.cursor_grabbed,
            fps: self.fps_counter.displayed_fps(),
            loaded_chunks: self.session.world().chunks.len() as u32,
            player_voxel: [player_voxel.x, player_voxel.y, player_voxel.z],
            player_chunk: [player_chunk.x, player_chunk.y, player_chunk.z],
            player_facing: player.cardinal_facing(),
            biome_name: terrain_debug.biome_name,
            region_name: terrain_debug.region_name,
            ground_y: terrain_debug.ground_y,
            temperature_percent: terrain_debug.temperature_percent,
            humidity_percent: terrain_debug.humidity_percent,
            continentalness_percent: terrain_debug.continentalness_percent,
            hotbar_icon_layers,
            selected_hotbar_slot: self.session.selected_hotbar_slot() as u32,
        }
    }

    fn run_simulation_ticks(&mut self) {
        self.input.accumulate_simulation_input(&mut self.pending_simulation_input);
        let tick_count = self.simulation_clock.consume_frame(self.input.dt);
        if tick_count == 0 {
            return;
        }

        let persistence = &self.background.persistence;
        let tick_dt = self.simulation_clock.tick_dt_seconds();
        let zoom_active = self.zoom_active();

        for tick_index in 0..tick_count {
            let tick_input = if tick_index == 0 {
                self.pending_simulation_input.drain_for_tick(&self.input, tick_dt)
            } else {
                SimulationInput::continuous(&self.input, tick_dt)
            };

            self.session.tick(&tick_input, &self.renderer.resolved_blocks, zoom_active, |edit| {
                persistence.queue_world_edit(edit.position, edit.block_id)
            });
        }
    }

    fn update_world_streaming(&mut self) -> anyhow::Result<()> {
        let meshing_focus = meshing_focus_from_player(self.session.player());
        self.background.generation.streamer.pump(
            self.session.world_mut(),
            Some(&mut self.renderer),
            meshing_focus,
            self.render_radius_xz,
            self.render_radius_y,
            self.generation_budget_per_frame,
            self.completed_generation_budget_per_frame,
        )?;
        self.renderer.pump_meshing(self.session.world_mut(), meshing_focus)?;
        Ok(())
    }

    fn zoom_active(&self) -> bool {
        self.input.cursor_grabbed && self.input.key_held(KeyCode::KeyC)
    }

    fn flush_pending_world_saves(&self) -> anyhow::Result<()> {
        self.background.persistence.flush()
    }
}

#[derive(Debug)]
struct FixedSimulationClock {
    tick_dt_seconds: f32,
    max_ticks_per_frame: usize,
    accumulator_seconds: f32,
}

impl FixedSimulationClock {
    fn new(config: SimulationConfig) -> Self {
        let ticks_per_second = config.ticks_per_second.max(1);
        Self {
            tick_dt_seconds: 1.0 / ticks_per_second as f32,
            max_ticks_per_frame: config.max_ticks_per_frame.max(1),
            accumulator_seconds: 0.0,
        }
    }

    #[inline]
    fn tick_dt_seconds(&self) -> f32 {
        self.tick_dt_seconds
    }

    fn consume_frame(&mut self, frame_dt: f32) -> usize {
        let max_accumulator = self.tick_dt_seconds * self.max_ticks_per_frame as f32;
        self.accumulator_seconds = (self.accumulator_seconds + frame_dt).min(max_accumulator);

        let mut tick_count = 0;
        while self.accumulator_seconds + f32::EPSILON >= self.tick_dt_seconds
            && tick_count < self.max_ticks_per_frame
        {
            self.accumulator_seconds -= self.tick_dt_seconds;
            tick_count += 1;
        }
        self.accumulator_seconds = self.accumulator_seconds.max(0.0);
        tick_count
    }

    fn reset(&mut self) {
        self.accumulator_seconds = 0.0;
    }
}

fn selected_debug_view_mode(input: &InputState) -> Option<DebugViewMode> {
    let debug_modifier = input.key_held(KeyCode::ShiftLeft) || input.key_held(KeyCode::ShiftRight);
    if !debug_modifier {
        return None;
    }

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

fn hotbar_digit_keys() -> [KeyCode; HOTBAR_SLOT_COUNT] {
    [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_simulation_clock_accumulates_partial_frames() {
        let mut clock = FixedSimulationClock::new(SimulationConfig {
            ticks_per_second: 20,
            max_ticks_per_frame: 4,
        });

        assert_eq!(clock.consume_frame(0.02), 0);
        assert_eq!(clock.consume_frame(0.03), 1);
        assert_eq!(clock.consume_frame(0.05), 1);
    }

    #[test]
    fn fixed_simulation_clock_caps_catch_up_work() {
        let mut clock = FixedSimulationClock::new(SimulationConfig {
            ticks_per_second: 60,
            max_ticks_per_frame: 3,
        });

        assert_eq!(clock.consume_frame(1.0), 3);
        assert!(clock.accumulator_seconds <= clock.tick_dt_seconds());
    }
}
