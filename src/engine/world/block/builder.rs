use crate::engine::world::block::{
    definition::BlockDefinition, flags::BlockFlags, id::BlockId, textures::BlockTextures,
    tint::BlockTint,
};

pub struct BlockBuilder {
    name: String,
    flags: BlockFlags,
    textures: Option<BlockTextures>,
    tint: BlockTint,
}

impl BlockBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), flags: BlockFlags::NONE, textures: None, tint: BlockTint::None }
    }

    pub fn solid(mut self) -> Self {
        self.flags |= BlockFlags::SOLID;
        self
    }

    pub fn opaque(mut self) -> Self {
        self.flags |= BlockFlags::OPAQUE;
        self.flags.remove(BlockFlags::TRANSPARENT);
        self
    }

    pub fn transparent(mut self) -> Self {
        self.flags |= BlockFlags::TRANSPARENT;
        self.flags.remove(BlockFlags::OPAQUE);
        self
    }

    pub fn no_cull(mut self) -> Self {
        self.flags |= BlockFlags::NO_CULL;
        self
    }

    pub fn replaceable(mut self) -> Self {
        self.flags |= BlockFlags::REPLACEABLE;
        self
    }

    pub fn liquid(mut self) -> Self {
        self.flags |= BlockFlags::LIQUID;
        self
    }

    pub fn raycast_through(mut self) -> Self {
        self.flags |= BlockFlags::RAYCAST_THROUGH;
        self
    }

    pub fn textures(mut self, textures: BlockTextures) -> Self {
        self.textures = Some(textures);
        self
    }

    #[allow(dead_code)]
    pub fn tint(mut self, tint: BlockTint) -> Self {
        self.tint = tint;
        self
    }

    pub fn build(self, id: BlockId) -> BlockDefinition {
        BlockDefinition {
            id,
            name: self.name,
            flags: self.flags,
            textures: self.textures.unwrap_or_else(|| BlockTextures::all("missing")),
            tint: self.tint,
        }
    }
}
