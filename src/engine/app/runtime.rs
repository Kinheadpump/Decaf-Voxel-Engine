use std::{path::PathBuf, sync::Arc};

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
            editing::{HOTBAR_SLOT_COUNT, PlayerEditState},
            interaction::{
                PlaceBlockOutcome, RemoveBlockOutcome, place_block_in_front_detailed,
                preview_block_in_front, remove_block_in_front_detailed,
            },
            physics::update_player,
            state::{Player, camera_from_player},
        },
        render::{
            gpu_types::{DebugOverlayInput, DebugViewMode},
            materials::create_texture_registry,
            renderer::Renderer,
        },
        world::{
            accessor::VoxelAccessor,
            biome::BiomeTable,
            block::{
                create_default_block_registry, id::BlockId, registry::BlockRegistry,
                resolved::ResolvedBlockRegistry,
            },
            generator::{ChunkGenerator, StagedGenerator},
            persistence,
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
    completed_generation_budget_per_frame: usize,
    staged_generator: Arc<StagedGenerator>,
    streamer: WorldStreamer,
    renderer: Renderer,
    player: Player,
    world: World,
    block_registry: BlockRegistry,
    save_file: PathBuf,
    world_saver: persistence::AsyncWorldSaver,
    edit_state: PlayerEditState,
    input: InputState,
    total_time: f32,
    fps_counter: FpsCounter,
    show_debug_overlay: bool,
    water_block_id: BlockId,
}

impl AppRuntime {
    pub(super) async fn new(window: Arc<Window>, config: Config) -> anyhow::Result<Self> {
        let render_config = config.render;
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
        let edit_state = PlayerEditState::new(default_hotbar_slots(&block_registry));
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
        let persistent_edits =
            persistence::load_block_edits(&save_file, &save_context, &block_registry)?;
        let world_saver = persistence::AsyncWorldSaver::new(
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
            completed_generation_budget_per_frame,
            staged_generator,
            streamer,
            renderer,
            player,
            world,
            block_registry,
            save_file,
            world_saver,
            edit_state,
            input: InputState::new(),
            total_time: 0.0,
            fps_counter: FpsCounter::new(),
            show_debug_overlay: false,
            water_block_id,
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
        self.edit_state.tick(self.input.dt);
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
            WindowEvent::CloseRequested => {
                if let Err(err) = self.flush_pending_world_saves() {
                    crate::log_warn!(
                        "failed to flush world edits to {} before exit: {err:#}",
                        self.save_file.display()
                    );
                }
                event_loop_target.exit();
            }
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

        let debug_modifier =
            self.input.key_held(KeyCode::ShiftLeft) || self.input.key_held(KeyCode::ShiftRight);

        if self.input.cursor_grabbed && !debug_modifier {
            for (slot, key) in hotbar_digit_keys().into_iter().enumerate() {
                if self.input.key_pressed(key)
                    && self.edit_state.select_slot(slot, &self.block_registry)
                {
                    window.request_redraw();
                }
            }
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
        let debug_overlay = Some(self.build_debug_overlay_input());
        let underwater_tint_active = self.player_eye_in_water();

        self.renderer.set_debug_overlay(debug_overlay);
        self.renderer.set_underwater_tint_active(underwater_tint_active);

        if let Err(err) = self.renderer.render(&camera, self.total_time) {
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
        let hud = self.edit_state.build_hud_state(&self.block_registry);

        DebugOverlayInput {
            show_debug: self.show_debug_overlay,
            show_game_hud: self.input.cursor_grabbed,
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
            hotbar_line: hud.hotbar_line,
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

        let scroll_steps = self.input.mouse_scroll_lines.round() as i32;
        if scroll_steps != 0 {
            let _ = self.edit_state.cycle_selection(scroll_steps, &self.block_registry);
        }

        let interaction_origin = self.player.eye_position();
        let interaction_direction = self.player.forward_3d();

        if self.input.mouse_pressed(MouseButton::Middle) {
            if let Some(preview) = preview_block_in_front(
                &self.world,
                &self.renderer.resolved_blocks,
                &self.player,
                interaction_origin,
                interaction_direction,
                self.player_config.reach_distance,
            ) {
                self.edit_state.pick_block(preview.target_block_id, &self.block_registry);
            } else {
                self.edit_state.set_feedback("No block to pick".to_string());
            }
        }

        if self.input.key_pressed(KeyCode::KeyZ) {
            if let Some(change) = self.edit_state.undo_last_edit(&mut self.world, &self.block_registry)
            {
                self.queue_world_edit_save(change.position, change.after);
            }
        }

        if self.input.mouse_pressed(MouseButton::Left) {
            match remove_block_in_front_detailed(
                &mut self.world,
                &self.renderer.resolved_blocks,
                interaction_origin,
                interaction_direction,
                self.player_config.reach_distance,
            ) {
                RemoveBlockOutcome::Removed(change) => {
                    self.edit_state.record_edit(
                        crate::engine::player::editing::BlockEditRecord {
                            position: change.position,
                            before: change.before,
                            after: change.after,
                        },
                        &self.block_registry,
                        change.before,
                        "Broke",
                    );
                    self.queue_world_edit_save(change.position, change.after);
                    crate::log_debug!("Removed block");
                }
                RemoveBlockOutcome::NoTarget => {
                    self.edit_state.set_feedback("No block to remove".to_string());
                }
            }
        }

        if self.input.mouse_pressed(MouseButton::Right) {
            let selected_block = self.edit_state.selected_block();
            match place_block_in_front_detailed(
                &mut self.world,
                &self.renderer.resolved_blocks,
                &self.player,
                interaction_origin,
                interaction_direction,
                self.player_config.reach_distance,
                selected_block,
            ) {
                PlaceBlockOutcome::Placed(change) => {
                    self.edit_state.record_edit(
                        crate::engine::player::editing::BlockEditRecord {
                            position: change.position,
                            before: change.before,
                            after: change.after,
                        },
                        &self.block_registry,
                        change.after,
                        "Placed",
                    );
                    self.queue_world_edit_save(change.position, change.after);
                    crate::log_debug!("Placed block");
                }
                PlaceBlockOutcome::NoTarget => {
                    self.edit_state.set_feedback("No block in reach".to_string());
                }
                PlaceBlockOutcome::NoPlacement => {
                    self.edit_state.set_feedback("No placement space".to_string());
                }
                PlaceBlockOutcome::Occupied => {
                    self.edit_state.set_feedback("Placement blocked".to_string());
                }
                PlaceBlockOutcome::BlockedByPlayer => {
                    self.edit_state.set_feedback("Cannot place inside player".to_string());
                }
            }
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
            self.completed_generation_budget_per_frame,
        )?;
        self.renderer.pump_meshing(&mut self.world, meshing_focus)?;
        Ok(())
    }

    fn zoom_active(&self) -> bool {
        self.input.cursor_grabbed && self.input.key_held(KeyCode::KeyC)
    }

    fn player_eye_in_water(&self) -> bool {
        let eye_voxel = self.player.eye_position().floor().as_ivec3();
        VoxelAccessor { world: &self.world }.get_world_voxel(eye_voxel).block_id()
            == self.water_block_id
    }

    fn queue_world_edit_save(&self, position: crate::engine::core::math::IVec3, block_id: BlockId) {
        if let Err(err) = self.world_saver.record_edit(position, block_id) {
            crate::log_warn!(
                "failed to queue world save update for {}: {err:#}",
                self.save_file.display()
            );
        }
    }

    fn flush_pending_world_saves(&self) -> anyhow::Result<()> {
        self.world_saver.flush()
    }
}

pub(super) fn resolve_background_worker_counts(render_config: &RenderConfig) -> (usize, usize) {
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

fn default_hotbar_slots(block_registry: &BlockRegistry) -> [BlockId; HOTBAR_SLOT_COUNT] {
    [
        block_registry.must_get_id("stone"),
        block_registry.must_get_id("dirt"),
        block_registry.must_get_id("grass"),
        block_registry.must_get_id("oak_planks"),
        block_registry.must_get_id("log"),
        block_registry.must_get_id("glass"),
        block_registry.must_get_id("leaves"),
        block_registry.must_get_id("sand"),
        block_registry.must_get_id("water"),
    ]
}
