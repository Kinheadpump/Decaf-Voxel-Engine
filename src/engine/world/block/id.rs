#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u16);

impl BlockId {
    pub const AIR: Self = Self(0);
}
