use winit::{event::MouseButton, keyboard::KeyCode};

use crate::{
    config::PlayerConfig,
    engine::{
        core::math::IVec3,
        input::SimulationInput,
        player::{
            editing::{BlockEditRecord, HOTBAR_SLOT_COUNT, PlayerEditState},
            interaction::{
                PlaceBlockOutcome, RemoveBlockOutcome, place_block_in_front_detailed,
                preview_block_in_front, remove_block_in_front_detailed,
            },
            physics::update_player,
            state::Player,
        },
        world::{
            accessor::VoxelAccessor,
            block::{id::BlockId, registry::BlockRegistry, resolved::ResolvedBlockRegistry},
            storage::World,
        },
    },
};

#[derive(Debug, Clone)]
pub(super) struct GameRules {
    pub player_config: PlayerConfig,
    pub water_block_id: BlockId,
}

impl GameRules {
    pub fn new(player_config: PlayerConfig, water_block_id: BlockId) -> Self {
        Self { player_config, water_block_id }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WorldSaveRecord {
    pub position: IVec3,
    pub block_id: BlockId,
}

pub(super) struct GameSession {
    player: Player,
    world: World,
    inventory: PlayerEditState,
    rules: GameRules,
    simulation_time: f32,
}

impl GameSession {
    pub fn new(player: Player, world: World, inventory: PlayerEditState, rules: GameRules) -> Self {
        Self { player, world, inventory, rules, simulation_time: 0.0 }
    }

    #[inline]
    pub fn player(&self) -> &Player {
        &self.player
    }

    #[inline]
    pub fn world(&self) -> &World {
        &self.world
    }

    #[inline]
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    #[inline]
    pub fn simulation_time(&self) -> f32 {
        self.simulation_time
    }

    #[inline]
    pub fn hotbar_slots(&self) -> [BlockId; HOTBAR_SLOT_COUNT] {
        self.inventory.hotbar_slots()
    }

    #[inline]
    pub fn selected_hotbar_slot(&self) -> usize {
        self.inventory.selected_slot()
    }

    pub fn select_hotbar_slot(&mut self, slot: usize) -> bool {
        self.inventory.select_slot(slot)
    }

    pub fn tick(
        &mut self,
        input: &SimulationInput<'_>,
        resolved_blocks: &ResolvedBlockRegistry,
        zoom_active: bool,
        mut queue_save: impl FnMut(WorldSaveRecord),
    ) {
        self.simulation_time += input.dt();

        update_player(
            &mut self.player,
            input,
            &self.world,
            resolved_blocks,
            self.simulation_time,
            &self.rules.player_config,
            zoom_active,
        );

        let scroll_steps = input.mouse_scroll_lines().round() as i32;
        if scroll_steps != 0 {
            let _ = self.inventory.cycle_selection(scroll_steps);
        }

        let interaction_origin = self.player.eye_position();
        let interaction_direction = self.player.forward_3d();
        let reach_distance = self.rules.player_config.reach_distance;

        if input.mouse_pressed(MouseButton::Middle) {
            if let Some(preview) = preview_block_in_front(
                &self.world,
                resolved_blocks,
                &self.player,
                interaction_origin,
                interaction_direction,
                reach_distance,
            ) {
                self.inventory.pick_block(preview.target_block_id);
            }
        }

        if input.key_pressed(KeyCode::KeyZ) {
            if let Some(change) = self.inventory.undo_last_edit(&mut self.world) {
                queue_save(WorldSaveRecord { position: change.position, block_id: change.after });
            }
        }

        if input.mouse_pressed(MouseButton::Left) {
            match remove_block_in_front_detailed(
                &mut self.world,
                resolved_blocks,
                interaction_origin,
                interaction_direction,
                reach_distance,
            ) {
                RemoveBlockOutcome::Removed(change) => {
                    self.inventory.record_edit(BlockEditRecord {
                        position: change.position,
                        before: change.before,
                        after: change.after,
                    });
                    queue_save(WorldSaveRecord {
                        position: change.position,
                        block_id: change.after,
                    });
                    crate::log_debug!("Removed block");
                }
                RemoveBlockOutcome::NoTarget => {}
            }
        }

        if input.mouse_pressed(MouseButton::Right) {
            match place_block_in_front_detailed(
                &mut self.world,
                resolved_blocks,
                &self.player,
                interaction_origin,
                interaction_direction,
                reach_distance,
                self.inventory.selected_block(),
            ) {
                PlaceBlockOutcome::Placed(change) => {
                    self.inventory.record_edit(BlockEditRecord {
                        position: change.position,
                        before: change.before,
                        after: change.after,
                    });
                    queue_save(WorldSaveRecord {
                        position: change.position,
                        block_id: change.after,
                    });
                    crate::log_debug!("Placed block");
                }
                PlaceBlockOutcome::NoTarget
                | PlaceBlockOutcome::NoPlacement
                | PlaceBlockOutcome::Occupied
                | PlaceBlockOutcome::BlockedByPlayer => {}
            }
        }
    }

    pub fn player_eye_in_water(&self) -> bool {
        let eye_voxel = self.player.eye_position().floor().as_ivec3();
        VoxelAccessor { world: &self.world }.get_world_voxel(eye_voxel).block_id()
            == self.rules.water_block_id
    }
}

pub(super) fn default_hotbar_slots(block_registry: &BlockRegistry) -> [BlockId; HOTBAR_SLOT_COUNT] {
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

#[cfg(test)]
mod tests {
    use glam::IVec3;

    use super::*;
    use crate::engine::{
        input::InputState,
        render::materials::create_texture_registry,
        world::{
            block::{create_default_block_registry, resolved::ResolvedBlockRegistry},
            chunk::Chunk,
            coord::ChunkCoord,
        },
    };

    #[test]
    fn simulation_time_advances_through_session_state() {
        let registry = create_default_block_registry();
        let rules = GameRules::new(PlayerConfig::default(), registry.must_get_id("water"));
        let mut session = GameSession::new(
            Player::from_config(&PlayerConfig::default()),
            World::new(),
            PlayerEditState::new(default_hotbar_slots(&registry)),
            rules,
        );
        let texture_registry = create_texture_registry(&registry);
        let resolved_blocks = ResolvedBlockRegistry::build(&registry, texture_registry.layer_map());
        let input_state = InputState::new();

        session.tick(
            &SimulationInput::continuous(&input_state, 0.5),
            &resolved_blocks,
            false,
            |_| {},
        );
        session.tick(
            &SimulationInput::continuous(&input_state, 0.25),
            &resolved_blocks,
            false,
            |_| {},
        );

        assert_eq!(session.simulation_time(), 0.75);
    }

    #[test]
    fn eye_in_water_uses_game_rules_block_id() {
        let registry = create_default_block_registry();
        let water = registry.must_get_id("water");
        let rules = GameRules::new(PlayerConfig::default(), water);
        let mut player = Player::from_config(&PlayerConfig::default());
        let mut world = World::new();
        world.insert_chunk(ChunkCoord(IVec3::ZERO), Chunk::new());
        let _ = world.take_dirty();
        assert!(world.set_block_world(IVec3::new(0, 1, 0), water));
        player.position.y = 0.0;

        let session = GameSession::new(
            player,
            world,
            PlayerEditState::new(default_hotbar_slots(&registry)),
            rules,
        );

        assert!(session.player_eye_in_water());
    }
}
