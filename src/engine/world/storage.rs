use ahash::{AHashMap, AHashSet};

use crate::engine::{
    core::{
        math::{IVec3, UVec3},
        types::CHUNK_SIZE,
    },
    world::{block::id::BlockId, chunk::Chunk, coord::ChunkCoord, voxel::Voxel},
};

pub struct World {
    pub chunks: AHashMap<ChunkCoord, Chunk>,
    dirty_set: AHashSet<ChunkCoord>,
    dirty_queue: Vec<ChunkCoord>,
}

impl World {
    pub fn new() -> Self {
        Self { chunks: AHashMap::new(), dirty_set: AHashSet::new(), dirty_queue: Vec::new() }
    }

    pub fn mark_dirty(&mut self, coord: ChunkCoord) {
        if self.dirty_set.insert(coord) {
            self.dirty_queue.push(coord);
        }
    }

    pub fn insert_chunk(&mut self, coord: ChunkCoord, chunk: Chunk) {
        self.chunks.insert(coord, chunk);
        self.mark_dirty(coord);
    }

    pub fn contains_chunk(&self, coord: ChunkCoord) -> bool {
        self.chunks.contains_key(&coord)
    }

    pub fn remove_chunk(&mut self, coord: ChunkCoord) -> Option<Chunk> {
        self.dirty_set.remove(&coord);
        self.dirty_queue.retain(|queued| *queued != coord);
        self.chunks.remove(&coord)
    }

    pub fn set_block_world(&mut self, p: IVec3, block_id: BlockId) -> bool {
        self.set_voxel_world(p, Voxel::from_block_id(block_id))
    }

    fn set_voxel_world(&mut self, p: IVec3, voxel: Voxel) -> bool {
        let coord = ChunkCoord::from_world_voxel(p);
        let local = coord.local_voxel(p);

        {
            let Some(chunk) = self.chunks.get_mut(&coord) else {
                return false;
            };

            let current = chunk.get(local.x as usize, local.y as usize, local.z as usize);
            if current == voxel {
                return false;
            }

            chunk.set(local.x as usize, local.y as usize, local.z as usize, voxel);
        }

        self.mark_dirty(coord);
        self.mark_border_neighbors_dirty(coord, local);
        true
    }

    pub fn take_dirty(&mut self) -> Vec<ChunkCoord> {
        let dirty_chunks = std::mem::take(&mut self.dirty_queue);
        self.dirty_set.clear();
        dirty_chunks
    }

    fn mark_border_neighbors_dirty(&mut self, coord: ChunkCoord, local: UVec3) {
        let maybe_mark = |delta: IVec3, this: &mut Self| {
            let neighbor = coord.offset(delta);
            if this.chunks.contains_key(&neighbor) {
                this.mark_dirty(neighbor);
            }
        };

        if local.x == 0 {
            maybe_mark(IVec3::new(-1, 0, 0), self);
        }
        if local.x as usize == CHUNK_SIZE - 1 {
            maybe_mark(IVec3::new(1, 0, 0), self);
        }
        if local.y == 0 {
            maybe_mark(IVec3::new(0, -1, 0), self);
        }
        if local.y as usize == CHUNK_SIZE - 1 {
            maybe_mark(IVec3::new(0, 1, 0), self);
        }
        if local.z == 0 {
            maybe_mark(IVec3::new(0, 0, -1), self);
        }
        if local.z as usize == CHUNK_SIZE - 1 {
            maybe_mark(IVec3::new(0, 0, 1), self);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_border_voxel_marks_neighbor_chunk_dirty() {
        let mut world = World::new();
        let center = ChunkCoord(IVec3::new(0, 0, 0));
        let east = ChunkCoord(IVec3::new(1, 0, 0));

        world.insert_chunk(center, Chunk::new());
        world.insert_chunk(east, Chunk::new());
        let _ = world.take_dirty();

        assert!(world.set_block_world(IVec3::new((CHUNK_SIZE - 1) as i32, 0, 0), BlockId(1)));

        let dirty = world.take_dirty();
        assert!(dirty.contains(&center));
        assert!(dirty.contains(&east));
    }

    #[test]
    fn editing_interior_voxel_only_marks_own_chunk_dirty() {
        let mut world = World::new();
        let center = ChunkCoord(IVec3::new(0, 0, 0));

        world.insert_chunk(center, Chunk::new());
        let _ = world.take_dirty();

        assert!(world.set_block_world(IVec3::new(4, 4, 4), BlockId(1)));

        let dirty = world.take_dirty();
        assert_eq!(dirty, vec![center]);
    }

    #[test]
    fn removing_chunk_clears_it_from_dirty_queue() {
        let mut world = World::new();
        let center = ChunkCoord(IVec3::new(0, 0, 0));

        world.insert_chunk(center, Chunk::new());
        assert!(world.remove_chunk(center).is_some());

        assert!(world.take_dirty().is_empty());
        assert!(!world.contains_chunk(center));
    }
}
