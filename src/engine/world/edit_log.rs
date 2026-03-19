use ahash::AHashMap;

use crate::engine::world::{
    block::id::BlockId,
    chunk::Chunk,
    coord::{ChunkCoord, LocalVoxelPos, WorldVoxelPos},
    voxel::Voxel,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PersistentBlockEdit {
    local: LocalVoxelPos,
    block_id: BlockId,
}

#[derive(Default)]
pub struct PersistentEditLog {
    edits: AHashMap<ChunkCoord, Vec<PersistentBlockEdit>>,
}

impl PersistentEditLog {
    pub fn record_world(&mut self, position: impl Into<WorldVoxelPos>, block_id: BlockId) {
        let position = position.into();
        let coord = ChunkCoord::from_world_voxel(position);
        self.record_local(coord, coord.local_voxel(position), block_id);
    }

    pub fn iter_world(&self) -> impl Iterator<Item = (WorldVoxelPos, BlockId)> + '_ {
        self.edits.iter().flat_map(|(coord, edits)| {
            edits.iter().map(move |edit| (coord.world_voxel(edit.local), edit.block_id))
        })
    }

    pub fn apply_to_chunk(&self, coord: ChunkCoord, chunk: &mut Chunk) {
        let Some(edits) = self.edits.get(&coord) else {
            return;
        };

        for edit in edits {
            chunk.set_local(edit.local, Voxel::from_block_id(edit.block_id));
        }
    }

    fn record_local(&mut self, coord: ChunkCoord, local: LocalVoxelPos, block_id: BlockId) {
        let edits = self.edits.entry(coord).or_default();
        if let Some(existing) = edits.iter_mut().find(|edit| edit.local == local) {
            existing.block_id = block_id;
        } else {
            edits.push(PersistentBlockEdit { local, block_id });
        }
    }
}
