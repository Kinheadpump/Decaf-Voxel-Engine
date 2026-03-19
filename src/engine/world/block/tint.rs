use crate::engine::core::types::FaceDir;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BiomeTint {
    #[default]
    None = 0,
    Grass = 1,
    Foliage = 2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlockTint {
    #[default]
    None,
    All(BiomeTint),
    TopBottomSides {
        top: BiomeTint,
        bottom: BiomeTint,
        sides: BiomeTint,
    },
}

impl BlockTint {
    pub fn all(tint: BiomeTint) -> Self {
        Self::All(tint)
    }

    pub fn top_bottom_sides(top: BiomeTint, bottom: BiomeTint, sides: BiomeTint) -> Self {
        Self::TopBottomSides { top, bottom, sides }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResolvedFaceTints {
    pub pos_x: BiomeTint,
    pub neg_x: BiomeTint,
    pub pos_y: BiomeTint,
    pub neg_y: BiomeTint,
    pub pos_z: BiomeTint,
    pub neg_z: BiomeTint,
}

impl ResolvedFaceTints {
    pub fn get(&self, dir: FaceDir) -> BiomeTint {
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

    pub fn from_block_tint(tint: BlockTint) -> Self {
        match tint {
            BlockTint::None => Self::default(),
            BlockTint::All(tint) => Self {
                pos_x: tint,
                neg_x: tint,
                pos_y: tint,
                neg_y: tint,
                pos_z: tint,
                neg_z: tint,
            },
            BlockTint::TopBottomSides { top, bottom, sides } => Self {
                pos_x: sides,
                neg_x: sides,
                pos_y: top,
                neg_y: bottom,
                pos_z: sides,
                neg_z: sides,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_bottom_side_tints_resolve_per_face() {
        let resolved = ResolvedFaceTints::from_block_tint(BlockTint::top_bottom_sides(
            BiomeTint::Grass,
            BiomeTint::None,
            BiomeTint::Foliage,
        ));

        assert_eq!(resolved.pos_y, BiomeTint::Grass);
        assert_eq!(resolved.neg_y, BiomeTint::None);
        assert_eq!(resolved.pos_x, BiomeTint::Foliage);
        assert_eq!(resolved.neg_x, BiomeTint::Foliage);
        assert_eq!(resolved.pos_z, BiomeTint::Foliage);
        assert_eq!(resolved.neg_z, BiomeTint::Foliage);
    }
}
