use crate::engine::world::block::{flags::BlockFlags, id::BlockId, textures::BlockTextures};

#[derive(Clone, Debug)]
pub struct BlockDefinition {
    pub id: BlockId,
    pub name: String,
    pub flags: BlockFlags,
    pub textures: BlockTextures,
}
