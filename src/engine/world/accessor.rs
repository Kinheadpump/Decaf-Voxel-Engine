use crate::engine::{
    core::{types::CHUNK_SIZE_I32, math::IVec3},
    world::{coord::ChunkCoord, storage::World, voxel::Voxel},
};

pub struct VoxelAccessor<'a> {
    pub world: &'a World,
}

impl<'a> VoxelAccessor<'a> {
    #[inline]
    pub fn get_world_voxel(&self, p: IVec3) -> Voxel {
        let c = ChunkCoord(IVec3::new(
            p.x.div_euclid(CHUNK_SIZE_I32),
            p.y.div_euclid(CHUNK_SIZE_I32),
            p.z.div_euclid(CHUNK_SIZE_I32),
        ));

        let lx = p.x.rem_euclid(CHUNK_SIZE_I32) as usize;
        let ly = p.y.rem_euclid(CHUNK_SIZE_I32) as usize;
        let lz = p.z.rem_euclid(CHUNK_SIZE_I32) as usize;

        self.world
            .chunks
            .get(&c)
            .map(|chunk| chunk.get(lx, ly, lz))
            .unwrap_or(Voxel::AIR)
    }
}
