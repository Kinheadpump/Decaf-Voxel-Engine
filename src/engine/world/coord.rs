use crate::engine::core::{types::CHUNK_SIZE_I32, math::IVec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChunkCoord(pub IVec3);

impl ChunkCoord {
    #[inline]
    pub fn world_origin(self) -> IVec3 {
        self.0 * CHUNK_SIZE_I32
    }
}