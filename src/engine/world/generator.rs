use crate::engine::{
    core::types::CHUNK_SIZE,
    world::{chunk::Chunk, coord::ChunkCoord, voxel::Voxel},
};

pub trait ChunkGenerator {
    fn generate(&self, coord: ChunkCoord, chunk: &mut Chunk);
}

pub struct FlatGenerator;

impl ChunkGenerator for FlatGenerator {
    fn generate(&self, coord: ChunkCoord, chunk: &mut Chunk) {
        let origin = coord.world_origin();

        for y in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    let world_y = origin.y + y as i32;
                    let voxel = if world_y < 0 {
                        Voxel(1)
                    } else if world_y == 0 {
                        Voxel(2)
                    } else {
                        Voxel::AIR
                    };
                    chunk.set(x, y, z, voxel);
                }
            }
        }

        chunk.dirty = true;
    }
}
