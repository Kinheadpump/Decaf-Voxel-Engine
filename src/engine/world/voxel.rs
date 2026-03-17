#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Voxel(pub u32);

impl Voxel {
    pub const AIR: Self = Self(0);

    #[inline]
    pub fn is_air(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub fn is_solid(self) -> bool {
        self.0 != 0
    }

    #[inline]
    pub fn block_id(self) -> u32 {
        self.0
    }
}