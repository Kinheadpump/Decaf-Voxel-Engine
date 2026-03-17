use crate::engine::world::{block::id::BlockId, chunk::Chunk, coord::ChunkCoord, voxel::Voxel};

pub trait ChunkGenerator {
    fn generate(&self, coord: ChunkCoord, chunk: &mut Chunk);
}

#[derive(Clone, Copy, Debug)]
pub struct FlatGenerator {
    pub surface_block: BlockId,
    pub soil_block: BlockId,
    pub deep_block: BlockId,
}

impl FlatGenerator {
    pub fn new(surface_block: BlockId, soil_block: BlockId, deep_block: BlockId) -> Self {
        Self { surface_block, soil_block, deep_block }
    }
}

impl ChunkGenerator for FlatGenerator {
    fn generate(&self, coord: ChunkCoord, chunk: &mut Chunk) {
        let origin = coord.world_origin();

        chunk.fill_with(|_, y, _| {
            let world_y = origin.y + y as i32;
            if world_y < -4 {
                Voxel::from_block_id(self.deep_block)
            } else if world_y < 0 {
                Voxel::from_block_id(self.soil_block)
            } else if world_y == 0 {
                Voxel::from_block_id(self.surface_block)
            } else {
                Voxel::AIR
            }
        });
    }
}
