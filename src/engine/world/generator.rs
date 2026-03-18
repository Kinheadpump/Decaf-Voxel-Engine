use crate::engine::world::{block::id::BlockId, chunk::Chunk, coord::ChunkCoord, voxel::Voxel};

pub trait ChunkGenerator {
    fn generate(&self, coord: ChunkCoord, chunk: &mut Chunk);
}

#[derive(Clone, Copy, Debug)]
pub struct FlatGenerator {
    pub surface_block: BlockId,
    pub soil_block: BlockId,
    pub deep_block: BlockId,
    pub surface_level: i32,
    pub soil_depth: i32,
}

impl FlatGenerator {
    pub fn new(
        surface_block: BlockId,
        soil_block: BlockId,
        deep_block: BlockId,
        surface_level: i32,
        soil_depth: i32,
    ) -> Self {
        Self { surface_block, soil_block, deep_block, surface_level, soil_depth }
    }
}

impl ChunkGenerator for FlatGenerator {
    fn generate(&self, coord: ChunkCoord, chunk: &mut Chunk) {
        let origin = coord.world_origin();
        let soil_floor = self.surface_level - self.soil_depth;

        chunk.fill_with(|_, y, _| {
            let world_y = origin.y + y as i32;

            if world_y < soil_floor {
                Voxel::from_block_id(self.deep_block)
            } else if world_y < self.surface_level {
                Voxel::from_block_id(self.soil_block)
            } else if world_y == self.surface_level {
                Voxel::from_block_id(self.surface_block)
            } else {
                Voxel::AIR
            }
        });
    }
}
