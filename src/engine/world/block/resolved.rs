use crate::engine::core::types::FaceDir;
use crate::engine::world::block::{
    flags::BlockFlags, id::BlockId, registry::BlockRegistry, textures::TextureRef,
    tint::ResolvedFaceTints,
};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub struct ResolvedFaceTextures {
    pub pos_x: u16,
    pub neg_x: u16,
    pub pos_y: u16,
    pub neg_y: u16,
    pub pos_z: u16,
    pub neg_z: u16,
}

impl ResolvedFaceTextures {
    pub fn get(&self, dir: FaceDir) -> u16 {
        use FaceDir::*;
        match dir {
            PosX => self.pos_x,
            NegX => self.neg_x,
            PosY => self.pos_y,
            NegY => self.neg_y,
            PosZ => self.pos_z,
            NegZ => self.neg_z,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ResolvedBlock {
    pub id: BlockId,
    pub flags: BlockFlags,
    pub textures: ResolvedFaceTextures,
    pub tints: ResolvedFaceTints,
}

impl ResolvedBlock {
    #[inline]
    pub fn is_air(&self) -> bool {
        self.id == BlockId::AIR
    }

    #[inline]
    pub fn is_solid(&self) -> bool {
        self.flags.contains(BlockFlags::SOLID)
    }

    #[inline]
    pub fn is_opaque(&self) -> bool {
        self.flags.contains(BlockFlags::OPAQUE)
    }

    #[inline]
    pub fn is_transparent(&self) -> bool {
        self.flags.contains(BlockFlags::TRANSPARENT)
    }

    #[inline]
    pub fn is_no_cull(&self) -> bool {
        self.flags.contains(BlockFlags::NO_CULL)
    }

    #[inline]
    pub fn is_replaceable(&self) -> bool {
        self.flags.contains(BlockFlags::REPLACEABLE)
    }

    #[inline]
    pub fn is_raycast_through(&self) -> bool {
        self.flags.contains(BlockFlags::RAYCAST_THROUGH)
    }
}

#[derive(Clone)]
pub struct ResolvedBlockRegistry {
    blocks: Vec<ResolvedBlock>,
}

impl ResolvedBlockRegistry {
    #[inline]
    pub fn air(&self) -> ResolvedBlock {
        self.blocks[BlockId::AIR.0 as usize]
    }

    pub fn get(&self, id: BlockId) -> ResolvedBlock {
        self.blocks.get(id.0 as usize).copied().unwrap_or_else(|| self.air())
    }

    pub fn get_voxel(&self, voxel: crate::engine::world::voxel::Voxel) -> ResolvedBlock {
        self.get(voxel.block_id())
    }

    pub fn build(authoring: &BlockRegistry, texture_layers: &HashMap<String, u16>) -> Self {
        let mut blocks = Vec::with_capacity(authoring.len());

        for def in authoring.iter() {
            let resolve_tex = |name: &TextureRef| -> u16 {
                *texture_layers.get(&name.0).unwrap_or_else(|| {
                    panic!("Missing texture '{}' for block '{}'", name.0, def.name)
                })
            };

            let textures = match &def.textures {
                crate::engine::world::block::textures::BlockTextures::All(tex) => {
                    let id = resolve_tex(tex);
                    ResolvedFaceTextures {
                        pos_x: id,
                        neg_x: id,
                        pos_y: id,
                        neg_y: id,
                        pos_z: id,
                        neg_z: id,
                    }
                }
                crate::engine::world::block::textures::BlockTextures::TopBottomSides {
                    top,
                    bottom,
                    sides,
                } => ResolvedFaceTextures {
                    pos_x: resolve_tex(sides),
                    neg_x: resolve_tex(sides),
                    pos_y: resolve_tex(top),
                    neg_y: resolve_tex(bottom),
                    pos_z: resolve_tex(sides),
                    neg_z: resolve_tex(sides),
                },
            };

            blocks.push(ResolvedBlock {
                id: def.id,
                flags: def.flags,
                textures,
                tints: ResolvedFaceTints::from_block_tint(def.tint),
            });
        }

        Self { blocks }
    }
}
