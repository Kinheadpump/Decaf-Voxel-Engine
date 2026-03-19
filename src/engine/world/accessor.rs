use crate::engine::core::types::FaceDir;
use crate::engine::world::{
    chunk::Chunk,
    coord::{ChunkCoord, WorldVoxelPos},
    storage::World,
    voxel::Voxel,
};

pub trait WorldVoxelReader {
    fn get_world_voxel<P: Into<WorldVoxelPos>>(&self, p: P) -> Voxel;
}

pub trait ChunkNeighborReader {
    fn get_chunk_neighbor(&self, center: ChunkCoord, dir: FaceDir) -> Option<&Chunk>;
}

pub struct VoxelAccessor<'a> {
    pub world: &'a World,
}

impl<'a> VoxelAccessor<'a> {
    #[inline]
    pub fn get_world_voxel<P: Into<WorldVoxelPos>>(&self, p: P) -> Voxel {
        WorldVoxelReader::get_world_voxel(self, p)
    }
}

impl WorldVoxelReader for VoxelAccessor<'_> {
    #[inline]
    fn get_world_voxel<P: Into<WorldVoxelPos>>(&self, p: P) -> Voxel {
        let p = p.into();
        let chunk_coord = ChunkCoord::from_world_voxel(p);
        let local = chunk_coord.local_voxel(p);

        self.world
            .chunks
            .get(&chunk_coord)
            .map(|chunk| chunk.get_local(local))
            .unwrap_or(Voxel::AIR)
    }
}

impl ChunkNeighborReader for VoxelAccessor<'_> {
    #[inline]
    fn get_chunk_neighbor(&self, center: ChunkCoord, dir: FaceDir) -> Option<&Chunk> {
        self.world.chunks.get(&center.offset(dir.normal()))
    }
}
