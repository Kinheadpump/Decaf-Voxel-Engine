use ahash::AHashMap;

use crate::engine::{
    core::{
        math::{IVec3, UVec3},
        types::{CHUNK_SIZE, FaceDir},
    },
    world::{
        block::id::BlockId, chunk::Chunk, coord::ChunkCoord, mesher::ChunkMeshDirtyRegion,
        voxel::Voxel,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyChunkEntry {
    pub coord: ChunkCoord,
    pub region: ChunkMeshDirtyRegion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PersistentBlockEdit {
    local: UVec3,
    block_id: BlockId,
}

pub struct World {
    pub chunks: AHashMap<ChunkCoord, Chunk>,
    dirty_regions: AHashMap<ChunkCoord, ChunkMeshDirtyRegion>,
    dirty_queue: Vec<ChunkCoord>,
    persistent_edits: AHashMap<ChunkCoord, Vec<PersistentBlockEdit>>,
}

impl World {
    pub fn new() -> Self {
        Self {
            chunks: AHashMap::new(),
            dirty_regions: AHashMap::new(),
            dirty_queue: Vec::new(),
            persistent_edits: AHashMap::new(),
        }
    }

    pub fn mark_dirty(&mut self, coord: ChunkCoord) {
        self.mark_dirty_region(coord, ChunkMeshDirtyRegion::full());
    }

    pub fn mark_dirty_region(&mut self, coord: ChunkCoord, region: ChunkMeshDirtyRegion) {
        if region.is_empty() {
            return;
        }

        if let Some(existing) = self.dirty_regions.get_mut(&coord) {
            existing.merge(region);
        } else {
            self.dirty_regions.insert(coord, region);
            self.dirty_queue.push(coord);
        }
    }

    pub fn insert_chunk(&mut self, coord: ChunkCoord, chunk: Chunk) {
        self.chunks.insert(coord, chunk);
        self.apply_persistent_edits(coord);
        self.mark_dirty(coord);
        self.mark_adjacent_chunk_borders_dirty(coord);
    }

    pub fn contains_chunk(&self, coord: ChunkCoord) -> bool {
        self.chunks.contains_key(&coord)
    }

    pub fn remove_chunk(&mut self, coord: ChunkCoord) -> Option<Chunk> {
        self.dirty_regions.remove(&coord);
        self.dirty_queue.retain(|queued| *queued != coord);
        let removed = self.chunks.remove(&coord)?;
        self.mark_adjacent_chunk_borders_dirty(coord);
        Some(removed)
    }

    pub fn set_block_world(&mut self, p: IVec3, block_id: BlockId) -> bool {
        self.set_voxel_world(p, Voxel::from_block_id(block_id))
    }

    pub fn load_persistent_edit_world(&mut self, p: IVec3, block_id: BlockId) {
        let coord = ChunkCoord::from_world_voxel(p);
        self.record_persistent_edit(coord, coord.local_voxel(p), block_id);
    }

    pub fn iter_persistent_edits(&self) -> impl Iterator<Item = (IVec3, BlockId)> + '_ {
        self.persistent_edits.iter().flat_map(|(coord, edits)| {
            let origin = coord.world_origin();
            edits.iter().map(move |edit| (origin + edit.local.as_ivec3(), edit.block_id))
        })
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

        self.record_persistent_edit(coord, local, voxel.block_id());
        self.mark_dirty_region(coord, ChunkMeshDirtyRegion::from_local_voxel(local));
        self.mark_border_neighbors_dirty(coord, local);
        true
    }

    pub fn take_dirty(&mut self) -> Vec<DirtyChunkEntry> {
        let dirty_chunks = std::mem::take(&mut self.dirty_queue);
        let mut dirty = Vec::with_capacity(dirty_chunks.len());

        for coord in dirty_chunks {
            if let Some(region) = self.dirty_regions.remove(&coord) {
                dirty.push(DirtyChunkEntry { coord, region });
            }
        }

        dirty
    }

    fn mark_border_neighbors_dirty(&mut self, coord: ChunkCoord, local: UVec3) {
        let edge = (CHUNK_SIZE - 1) as u32;
        let mark_if_loaded = |delta: IVec3, neighbor_local: UVec3, world: &mut Self| {
            let neighbor = coord.offset(delta);
            if world.chunks.contains_key(&neighbor) {
                world.mark_dirty_region(
                    neighbor,
                    ChunkMeshDirtyRegion::from_local_voxel(neighbor_local),
                );
            }
        };

        if local.x == 0 {
            mark_if_loaded(IVec3::new(-1, 0, 0), UVec3::new(edge, local.y, local.z), self);
        }
        if local.x as usize == CHUNK_SIZE - 1 {
            mark_if_loaded(IVec3::new(1, 0, 0), UVec3::new(0, local.y, local.z), self);
        }
        if local.y == 0 {
            mark_if_loaded(IVec3::new(0, -1, 0), UVec3::new(local.x, edge, local.z), self);
        }
        if local.y as usize == CHUNK_SIZE - 1 {
            mark_if_loaded(IVec3::new(0, 1, 0), UVec3::new(local.x, 0, local.z), self);
        }
        if local.z == 0 {
            mark_if_loaded(IVec3::new(0, 0, -1), UVec3::new(local.x, local.y, edge), self);
        }
        if local.z as usize == CHUNK_SIZE - 1 {
            mark_if_loaded(IVec3::new(0, 0, 1), UVec3::new(local.x, local.y, 0), self);
        }
    }

    fn mark_adjacent_chunk_borders_dirty(&mut self, coord: ChunkCoord) {
        let edge = CHUNK_SIZE - 1;
        let adjacent_faces = [
            (IVec3::new(1, 0, 0), FaceDir::NegX, 0),
            (IVec3::new(-1, 0, 0), FaceDir::PosX, edge),
            (IVec3::new(0, 1, 0), FaceDir::NegY, 0),
            (IVec3::new(0, -1, 0), FaceDir::PosY, edge),
            (IVec3::new(0, 0, 1), FaceDir::NegZ, 0),
            (IVec3::new(0, 0, -1), FaceDir::PosZ, edge),
        ];

        for (delta, neighbor_face, depth) in adjacent_faces {
            let neighbor = coord.offset(delta);
            if self.chunks.contains_key(&neighbor) {
                self.mark_dirty_region(
                    neighbor,
                    ChunkMeshDirtyRegion::from_face_slice(neighbor_face, depth),
                );
            }
        }
    }

    fn record_persistent_edit(&mut self, coord: ChunkCoord, local: UVec3, block_id: BlockId) {
        let edits = self.persistent_edits.entry(coord).or_default();
        if let Some(existing) = edits.iter_mut().find(|edit| edit.local == local) {
            existing.block_id = block_id;
        } else {
            edits.push(PersistentBlockEdit { local, block_id });
        }
    }

    fn apply_persistent_edits(&mut self, coord: ChunkCoord) {
        let Some(edits) = self.persistent_edits.get(&coord) else {
            return;
        };
        let Some(chunk) = self.chunks.get_mut(&coord) else {
            return;
        };

        for edit in edits {
            chunk.set(
                edit.local.x as usize,
                edit.local.y as usize,
                edit.local.z as usize,
                Voxel::from_block_id(edit.block_id),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::world::voxel::Voxel;

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
        assert!(dirty.iter().any(|entry| entry.coord == center));
        assert!(dirty.iter().any(|entry| entry.coord == east));
    }

    #[test]
    fn inserting_chunk_marks_loaded_neighbor_border_dirty() {
        let mut world = World::new();
        let center = ChunkCoord(IVec3::new(0, 0, 0));
        let east = ChunkCoord(IVec3::new(1, 0, 0));

        world.insert_chunk(center, Chunk::new());
        let _ = world.take_dirty();

        world.insert_chunk(east, Chunk::new());

        let dirty = world.take_dirty();
        let center_entry = dirty
            .iter()
            .find(|entry| entry.coord == center)
            .copied()
            .expect("center chunk should be dirtied when east neighbor appears");
        let east_entry = dirty
            .iter()
            .find(|entry| entry.coord == east)
            .copied()
            .expect("new chunk should be dirty after insertion");

        assert_eq!(east_entry.coord, east);
        assert!(center_entry.region.touches(FaceDir::PosX, CHUNK_SIZE - 1));
    }

    #[test]
    fn editing_interior_voxel_only_marks_own_chunk_dirty() {
        let mut world = World::new();
        let center = ChunkCoord(IVec3::new(0, 0, 0));

        world.insert_chunk(center, Chunk::new());
        let _ = world.take_dirty();

        assert!(world.set_block_world(IVec3::new(4, 4, 4), BlockId(1)));

        let dirty = world.take_dirty();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].coord, center);
        assert!(!dirty[0].region.is_full());
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

    #[test]
    fn removing_chunk_marks_loaded_neighbor_border_dirty() {
        let mut world = World::new();
        let center = ChunkCoord(IVec3::new(0, 0, 0));
        let east = ChunkCoord(IVec3::new(1, 0, 0));

        world.insert_chunk(center, Chunk::new());
        world.insert_chunk(east, Chunk::new());
        let _ = world.take_dirty();

        assert!(world.remove_chunk(east).is_some());

        let dirty = world.take_dirty();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].coord, center);
        assert!(dirty[0].region.touches(FaceDir::PosX, CHUNK_SIZE - 1));
    }

    #[test]
    fn persisted_edits_are_reapplied_when_chunk_reloads() {
        let mut world = World::new();
        let coord = ChunkCoord(IVec3::ZERO);
        let edited_voxel = IVec3::new(3, 4, 5);

        world.insert_chunk(coord, Chunk::new());
        let _ = world.take_dirty();
        assert!(world.set_block_world(edited_voxel, BlockId(7)));

        assert!(world.remove_chunk(coord).is_some());
        world.insert_chunk(coord, Chunk::new());

        let chunk = world.chunks.get(&coord).expect("reloaded chunk should exist");
        assert_eq!(chunk.get(3, 4, 5), Voxel::from_block_id(BlockId(7)));
    }

    #[test]
    fn loaded_persistent_edits_apply_to_future_chunk_inserts() {
        let mut world = World::new();
        let coord = ChunkCoord(IVec3::ZERO);

        world.load_persistent_edit_world(IVec3::new(1, 2, 3), BlockId(9));
        world.insert_chunk(coord, Chunk::new());

        let chunk = world.chunks.get(&coord).expect("chunk should exist");
        assert_eq!(chunk.get(1, 2, 3), Voxel::from_block_id(BlockId(9)));
    }
}
