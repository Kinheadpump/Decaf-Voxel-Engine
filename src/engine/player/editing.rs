use std::collections::VecDeque;

use crate::engine::{
    core::math::IVec3,
    world::{block::id::BlockId, storage::World},
};

pub const HOTBAR_SLOT_COUNT: usize = 9;
const UNDO_HISTORY_LIMIT: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockEditRecord {
    pub position: IVec3,
    pub before: BlockId,
    pub after: BlockId,
}

pub struct PlayerEditState {
    hotbar: [BlockId; HOTBAR_SLOT_COUNT],
    selected_slot: usize,
    undo_history: VecDeque<BlockEditRecord>,
}

impl PlayerEditState {
    pub fn new(hotbar: [BlockId; HOTBAR_SLOT_COUNT]) -> Self {
        Self { hotbar, selected_slot: 0, undo_history: VecDeque::with_capacity(UNDO_HISTORY_LIMIT) }
    }

    #[inline]
    pub fn selected_block(&self) -> BlockId {
        self.hotbar[self.selected_slot]
    }

    #[inline]
    pub fn selected_slot(&self) -> usize {
        self.selected_slot
    }

    #[inline]
    pub fn hotbar_slots(&self) -> [BlockId; HOTBAR_SLOT_COUNT] {
        self.hotbar
    }

    pub fn select_slot(&mut self, slot: usize) -> bool {
        if slot >= HOTBAR_SLOT_COUNT || slot == self.selected_slot {
            return false;
        }

        self.selected_slot = slot;
        true
    }

    pub fn cycle_selection(&mut self, delta: i32) -> bool {
        if delta == 0 {
            return false;
        }

        let next_slot =
            (self.selected_slot as i32 - delta).rem_euclid(HOTBAR_SLOT_COUNT as i32) as usize;
        self.select_slot(next_slot)
    }

    pub fn pick_block(&mut self, block_id: BlockId) -> bool {
        if block_id == BlockId::AIR {
            return false;
        }

        if let Some(existing_slot) =
            self.hotbar.iter().position(|&slot_block| slot_block == block_id)
        {
            self.selected_slot = existing_slot;
            return true;
        }

        self.hotbar[self.selected_slot] = block_id;
        true
    }

    pub fn record_edit(&mut self, edit: BlockEditRecord) {
        if self.undo_history.len() == UNDO_HISTORY_LIMIT {
            self.undo_history.pop_front();
        }
        self.undo_history.push_back(edit);
    }

    pub fn undo_last_edit(&mut self, world: &mut World) -> Option<BlockEditRecord> {
        let Some(edit) = self.undo_history.pop_back() else {
            return None;
        };

        if world.set_block_world(edit.position, edit.before) {
            Some(BlockEditRecord {
                position: edit.position,
                before: edit.after,
                after: edit.before,
            })
        } else {
            self.undo_history.push_back(edit);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::world::{
        block::{create_default_block_registry, registry::BlockRegistry},
        chunk::Chunk,
        coord::ChunkCoord,
    };

    #[test]
    fn selecting_slot_updates_selected_block() {
        let registry = create_default_block_registry();
        let mut state = PlayerEditState::new(default_hotbar(&registry));

        assert!(state.select_slot(2));
        assert_eq!(state.selected_slot(), 2);
        assert_eq!(state.selected_block(), registry.must_get_id("grass"));
    }

    #[test]
    fn pick_selects_existing_hotbar_slot() {
        let registry = create_default_block_registry();
        let mut state = PlayerEditState::new(default_hotbar(&registry));
        let sand = registry.must_get_id("sand");

        assert!(state.pick_block(sand));
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
        state.record_edit(BlockEditRecord {
            position: IVec3::new(1, 2, 3),
            before: BlockId::AIR,
            after: stone,
        });
        assert!(world.set_block_world(IVec3::new(1, 2, 3), oak));
        state.record_edit(BlockEditRecord {
            position: IVec3::new(1, 2, 3),
            before: stone,
            after: oak,
        });

        let undone = state.undo_last_edit(&mut world).expect("undo should succeed");
        assert_eq!(undone.after, stone);
        let chunk =
            world.chunks.get(&ChunkCoord(IVec3::ZERO)).expect("edited chunk should still exist");
        assert_eq!(chunk.get(1, 2, 3).block_id(), stone);
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
