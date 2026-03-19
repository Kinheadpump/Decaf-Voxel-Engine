use ahash::AHashMap;

use crate::engine::world::coord::ChunkCoord;

/// The mesh stage for a loaded chunk's current content generation.
///
/// Transitions are monotonic within a generation:
/// `Dirty -> Queued -> Meshing -> Meshed -> Uploaded`.
/// If the chunk contents change, the lifecycle resets back to `Dirty` for the
/// newer generation and stale transitions are ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkMeshState {
    Dirty,
    Queued,
    Meshing,
    Meshed,
    Uploaded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkLifecycle {
    pub content_generation: u32,
    pub mesh_state: ChunkMeshState,
}

impl ChunkLifecycle {
    #[inline]
    pub fn dirty(content_generation: u32) -> Self {
        Self { content_generation, mesh_state: ChunkMeshState::Dirty }
    }
}

#[derive(Default)]
pub struct ChunkLifecycleTracker {
    chunks: AHashMap<ChunkCoord, ChunkLifecycle>,
}

impl ChunkLifecycleTracker {
    pub fn insert_generated(&mut self, coord: ChunkCoord, generation: u32) {
        self.chunks.insert(coord, ChunkLifecycle::dirty(generation));
    }

    pub fn remove(&mut self, coord: ChunkCoord) -> Option<ChunkLifecycle> {
        self.chunks.remove(&coord)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn get(&self, coord: ChunkCoord) -> Option<ChunkLifecycle> {
        self.chunks.get(&coord).copied()
    }

    pub fn mark_dirty(&mut self, coord: ChunkCoord, generation: u32) {
        self.chunks.insert(coord, ChunkLifecycle::dirty(generation));
    }

    pub fn mark_queued(&mut self, coord: ChunkCoord, generation: u32) {
        self.transition(coord, generation, ChunkMeshState::Queued);
    }

    pub fn mark_meshing(&mut self, coord: ChunkCoord, generation: u32) {
        self.transition(coord, generation, ChunkMeshState::Meshing);
    }

    pub fn mark_meshed(&mut self, coord: ChunkCoord, generation: u32) {
        self.transition(coord, generation, ChunkMeshState::Meshed);
    }

    pub fn mark_uploaded(&mut self, coord: ChunkCoord, generation: u32) {
        self.transition(coord, generation, ChunkMeshState::Uploaded);
    }

    fn transition(&mut self, coord: ChunkCoord, generation: u32, mesh_state: ChunkMeshState) {
        let entry = self.chunks.entry(coord).or_insert_with(|| ChunkLifecycle::dirty(generation));
        if generation < entry.content_generation {
            return;
        }

        if generation > entry.content_generation {
            *entry = ChunkLifecycle::dirty(generation);
        }
        entry.mesh_state = mesh_state;
    }
}

#[cfg(test)]
mod tests {
    use glam::IVec3;

    use super::*;

    #[test]
    fn newer_dirty_generation_rejects_stale_mesh_transitions() {
        let mut tracker = ChunkLifecycleTracker::default();
        let coord = ChunkCoord(IVec3::new(1, 2, 3));

        tracker.insert_generated(coord, 4);
        tracker.mark_meshing(coord, 4);
        tracker.mark_dirty(coord, 5);
        tracker.mark_uploaded(coord, 4);

        assert_eq!(
            tracker.get(coord),
            Some(ChunkLifecycle { content_generation: 5, mesh_state: ChunkMeshState::Dirty })
        );
    }

    #[test]
    fn lifecycle_advances_within_same_generation() {
        let mut tracker = ChunkLifecycleTracker::default();
        let coord = ChunkCoord(IVec3::ZERO);

        tracker.insert_generated(coord, 7);
        tracker.mark_queued(coord, 7);
        tracker.mark_meshing(coord, 7);
        tracker.mark_meshed(coord, 7);
        tracker.mark_uploaded(coord, 7);

        assert_eq!(
            tracker.get(coord),
            Some(ChunkLifecycle { content_generation: 7, mesh_state: ChunkMeshState::Uploaded })
        );
    }
}
