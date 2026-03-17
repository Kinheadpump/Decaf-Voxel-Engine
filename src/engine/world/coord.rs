use crate::engine::core::{
    math::{IVec3, UVec3},
    types::CHUNK_SIZE_I32,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChunkCoord(pub IVec3);

impl ChunkCoord {
    #[inline]
    pub fn from_world_voxel(p: IVec3) -> Self {
        Self(IVec3::new(
            p.x.div_euclid(CHUNK_SIZE_I32),
            p.y.div_euclid(CHUNK_SIZE_I32),
            p.z.div_euclid(CHUNK_SIZE_I32),
        ))
    }

    #[inline]
    pub fn world_origin(self) -> IVec3 {
        self.0 * CHUNK_SIZE_I32
    }

    #[inline]
    pub fn local_voxel(self, p: IVec3) -> UVec3 {
        debug_assert_eq!(Self::from_world_voxel(p), self);

        UVec3::new(
            p.x.rem_euclid(CHUNK_SIZE_I32) as u32,
            p.y.rem_euclid(CHUNK_SIZE_I32) as u32,
            p.z.rem_euclid(CHUNK_SIZE_I32) as u32,
        )
    }

    #[inline]
    pub fn offset(self, delta: IVec3) -> Self {
        Self(self.0 + delta)
    }
}
