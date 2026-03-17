use crate::engine::{
    core::math::IVec3,
    world::{coord::ChunkCoord, storage::World, voxel::Voxel},
};

pub trait WorldVoxelReader {
    fn get_world_voxel(&self, p: IVec3) -> Voxel;
}

pub struct VoxelAccessor<'a> {
    pub world: &'a World,
}

impl<'a> VoxelAccessor<'a> {
    #[inline]
    pub fn get_world_voxel(&self, p: IVec3) -> Voxel {
        WorldVoxelReader::get_world_voxel(self, p)
    }
}

impl WorldVoxelReader for VoxelAccessor<'_> {
    #[inline]
    fn get_world_voxel(&self, p: IVec3) -> Voxel {
        let chunk_coord = ChunkCoord::from_world_voxel(p);
        let local = chunk_coord.local_voxel(p);

        self.world
            .chunks
            .get(&chunk_coord)
            .map(|chunk| chunk.get(local.x as usize, local.y as usize, local.z as usize))
            .unwrap_or(Voxel::AIR)
    }
}
