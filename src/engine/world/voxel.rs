use crate::engine::world::block::id::BlockId;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Voxel(pub u32);

impl Voxel {
    pub const AIR: Self = Self(BlockId::AIR.0 as u32);

    #[inline]
    pub const fn from_block_id(block_id: BlockId) -> Self {
        Self(block_id.0 as u32)
    }

    #[inline]
    pub fn block_id(self) -> BlockId {
        BlockId(self.0 as u16)
    }
}
