use ahash::{AHashMap, AHashSet};
use crate::engine::world::{chunk::Chunk, coord::ChunkCoord};

pub struct World {
    pub chunks: AHashMap<ChunkCoord, Chunk>,
    dirty_set: AHashSet<ChunkCoord>,
    dirty_queue: Vec<ChunkCoord>,
}

impl World {
    pub fn new() -> Self {
        Self {
            chunks: AHashMap::new(),
            dirty_set: AHashSet::new(),
            dirty_queue: Vec::new(),
        }
    }

    pub fn mark_dirty(&mut self, coord: ChunkCoord) {
        if self.dirty_set.insert(coord) {
            self.dirty_queue.push(coord);
        }
    }

    pub fn take_dirty(&mut self) -> Vec<ChunkCoord> {
        let dirty_chunks = std::mem::take(&mut self.dirty_queue);
        self.dirty_set.clear();
        dirty_chunks
    }
}