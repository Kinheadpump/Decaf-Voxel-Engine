use std::{collections::VecDeque, sync::Arc};

use crate::engine::{
    core::math::IVec3,
    world::{
        accessor::VoxelAccessor,
        block::{id::BlockId, registry::BlockRegistry},
        storage::World,
    },
};

pub const HOTBAR_SLOT_COUNT: usize = 9;
const UNDO_HISTORY_LIMIT: usize = 32;
const FEEDBACK_TTL_SECONDS: f32 = 2.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockEditRecord {
    pub position: IVec3,
    pub before: BlockId,
    pub after: BlockId,
}

#[derive(Clone, Debug)]
struct EditFeedback {
    ttl_seconds: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditHudState {
    pub hotbar_line: Arc<str>,
}

pub struct PlayerEditState {
    hotbar: [BlockId; HOTBAR_SLOT_COUNT],
    selected_slot: usize,
    undo_history: VecDeque<BlockEditRecord>,
    feedback: Option<EditFeedback>,
}

impl PlayerEditState {
    pub fn new(hotbar: [BlockId; HOTBAR_SLOT_COUNT]) -> Self {
        Self {
            hotbar,
            selected_slot: 0,
            undo_history: VecDeque::with_capacity(UNDO_HISTORY_LIMIT),
            feedback: None,
        }
    }

    #[inline]
    pub fn selected_block(&self) -> BlockId {
        self.hotbar[self.selected_slot]
    }

    #[inline]
    pub fn selected_slot(&self) -> usize {
        self.selected_slot
    }

    pub fn tick(&mut self, dt: f32) {
        let Some(feedback) = self.feedback.as_mut() else {
            return;
        };

        feedback.ttl_seconds -= dt;
        if feedback.ttl_seconds <= 0.0 {
            self.feedback = None;
        }
    }

    pub fn select_slot(&mut self, slot: usize, block_registry: &BlockRegistry) -> bool {
        if slot >= HOTBAR_SLOT_COUNT || slot == self.selected_slot {
            return false;
        }

        self.selected_slot = slot;
        let name = block_name(block_registry, self.selected_block());
        self.set_feedback(format!("Holding {name}"));
        true
    }

    pub fn cycle_selection(&mut self, delta: i32, block_registry: &BlockRegistry) -> bool {
        if delta == 0 {
            return false;
        }

        let next_slot =
            (self.selected_slot as i32 - delta).rem_euclid(HOTBAR_SLOT_COUNT as i32) as usize;
        self.select_slot(next_slot, block_registry)
    }

    pub fn pick_block(&mut self, block_id: BlockId, block_registry: &BlockRegistry) -> bool {
        if block_id == BlockId::AIR {
            self.set_feedback("Cannot pick air".to_string());
            return false;
        }

        if let Some(existing_slot) =
            self.hotbar.iter().position(|&slot_block| slot_block == block_id)
        {
            self.selected_slot = existing_slot;
            let name = block_name(block_registry, block_id);
            self.set_feedback(format!("Picked {name}"));
            return true;
        }

        self.hotbar[self.selected_slot] = block_id;
        let name = block_name(block_registry, block_id);
        self.set_feedback(format!("Picked {name}"));
        true
    }

    pub fn record_edit(
        &mut self,
        edit: BlockEditRecord,
        block_registry: &BlockRegistry,
        feedback_block: BlockId,
        verb: &str,
    ) {
        if self.undo_history.len() == UNDO_HISTORY_LIMIT {
            self.undo_history.pop_front();
        }
        self.undo_history.push_back(edit);
        let block_name = block_name(block_registry, feedback_block);
        self.set_feedback(format!("{verb} {block_name}"));
    }

    pub fn undo_last_edit(
        &mut self,
        world: &mut World,
        block_registry: &BlockRegistry,
    ) -> Option<BlockEditRecord> {
        let Some(edit) = self.undo_history.pop_back() else {
            self.set_feedback("Nothing to undo".to_string());
            return None;
        };

        if world.set_block_world(edit.position, edit.before) {
            let restored_name = block_name(block_registry, edit.before);
            self.set_feedback(format!("Restored {restored_name}"));
            Some(BlockEditRecord {
                position: edit.position,
                before: edit.after,
                after: edit.before,
            })
        } else {
            self.undo_history.push_back(edit);
            self.set_feedback("Undo failed".to_string());
            None
        }
    }

    pub fn set_feedback(&mut self, _message: String) {
        self.feedback = Some(EditFeedback { ttl_seconds: FEEDBACK_TTL_SECONDS });
    }

    pub fn build_hud_state(
        &self,
        block_registry: &BlockRegistry,
    ) -> EditHudState {
        let hotbar_line = Arc::<str>::from(self.build_hotbar_line(block_registry));
        EditHudState { hotbar_line }
    }

    fn build_hotbar_line(&self, block_registry: &BlockRegistry) -> String {
        let mut line = String::new();
        for (slot, &block_id) in self.hotbar.iter().enumerate() {
            if !line.is_empty() {
                line.push(' ');
            }

            let label = hotbar_label(block_registry, block_id);
            if slot == self.selected_slot {
                line.push_str(&format!("<{} {}>", slot + 1, label));
            } else {
                line.push_str(&format!("[{} {}]", slot + 1, label));
            }
        }

        line
    }
}

fn block_name(block_registry: &BlockRegistry, block_id: BlockId) -> String {
    block_registry
        .get(block_id)
        .map(|definition| humanize_block_name(&definition.name))
        .unwrap_or_else(|| "Unknown".to_string())
}

fn hotbar_label(block_registry: &BlockRegistry, block_id: BlockId) -> String {
    let name = block_name(block_registry, block_id);
    let compact = name.split(' ').next().unwrap_or(name.as_str());
    if compact.len() <= 5 { compact.to_string() } else { compact.chars().take(5).collect() }
}

fn humanize_block_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper = true;

    for ch in name.chars() {
        if ch == '_' {
            out.push(' ');
            upper = true;
        } else if upper {
            out.push(ch.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }

    out
}

pub fn current_block_at(world: &World, position: IVec3) -> BlockId {
    VoxelAccessor { world }.get_world_voxel(position).block_id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::world::{
        block::create_default_block_registry, chunk::Chunk, coord::ChunkCoord,
    };

    #[test]
    fn selecting_slot_updates_selected_block() {
        let registry = create_default_block_registry();
        let mut state = PlayerEditState::new(default_hotbar(&registry));

        assert!(state.select_slot(2, &registry));
        assert_eq!(state.selected_slot(), 2);
        assert_eq!(state.selected_block(), registry.must_get_id("grass"));
    }

    #[test]
    fn pick_selects_existing_hotbar_slot() {
        let registry = create_default_block_registry();
        let mut state = PlayerEditState::new(default_hotbar(&registry));
        let sand = registry.must_get_id("sand");

        assert!(state.pick_block(sand, &registry));
        assert_eq!(state.selected_block(), sand);
        assert_eq!(state.selected_slot(), 7);
    }

    #[test]
    fn undo_restores_previous_block() {
        let registry = create_default_block_registry();
        let stone = registry.must_get_id("stone");
        let oak = registry.must_get_id("oak_planks");
        let mut state = PlayerEditState::new(default_hotbar(&registry));
        let mut world = World::new();
        world.insert_chunk(ChunkCoord(IVec3::ZERO), Chunk::new());
        let _ = world.take_dirty();

        assert!(world.set_block_world(IVec3::new(1, 2, 3), stone));
        state.record_edit(
            BlockEditRecord { position: IVec3::new(1, 2, 3), before: BlockId::AIR, after: stone },
            &registry,
            stone,
            "Placed",
        );
        assert!(world.set_block_world(IVec3::new(1, 2, 3), oak));
        state.record_edit(
            BlockEditRecord { position: IVec3::new(1, 2, 3), before: stone, after: oak },
            &registry,
            oak,
            "Placed",
        );

        let undone = state.undo_last_edit(&mut world, &registry).expect("undo should succeed");
        assert_eq!(undone.after, stone);
        assert_eq!(current_block_at(&world, IVec3::new(1, 2, 3)), stone);
    }

    fn default_hotbar(registry: &BlockRegistry) -> [BlockId; HOTBAR_SLOT_COUNT] {
        [
            registry.must_get_id("stone"),
            registry.must_get_id("dirt"),
            registry.must_get_id("grass"),
            registry.must_get_id("oak_planks"),
            registry.must_get_id("log"),
            registry.must_get_id("glass"),
            registry.must_get_id("leaves"),
            registry.must_get_id("sand"),
            registry.must_get_id("water"),
        ]
    }
}
