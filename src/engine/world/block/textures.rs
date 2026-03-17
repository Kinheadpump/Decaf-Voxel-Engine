use crate::engine::core::types::FaceDir;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextureRef(pub String);

impl From<&str> for TextureRef {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[derive(Clone, Debug)]
pub enum BlockTextures {
    All(TextureRef),
    TopBottomSides {
        top: TextureRef,
        bottom: TextureRef,
        sides: TextureRef,
    },
    Explicit {
        pos_x: TextureRef,
        neg_x: TextureRef,
        pos_y: TextureRef,
        neg_y: TextureRef,
        pos_z: TextureRef,
        neg_z: TextureRef,
    },
}

impl BlockTextures {
    pub fn all(name: impl Into<TextureRef>) -> Self {
        Self::All(name.into())
    }

    pub fn top_bottom_sides(
        top: impl Into<TextureRef>,
        bottom: impl Into<TextureRef>,
        sides: impl Into<TextureRef>,
    ) -> Self {
        Self::TopBottomSides { top: top.into(), bottom: bottom.into(), sides: sides.into() }
    }

    pub fn explicit(
        pos_x: impl Into<TextureRef>,
        neg_x: impl Into<TextureRef>,
        pos_y: impl Into<TextureRef>,
        neg_y: impl Into<TextureRef>,
        pos_z: impl Into<TextureRef>,
        neg_z: impl Into<TextureRef>,
    ) -> Self {
        Self::Explicit {
            pos_x: pos_x.into(),
            neg_x: neg_x.into(),
            pos_y: pos_y.into(),
            neg_y: neg_y.into(),
            pos_z: pos_z.into(),
            neg_z: neg_z.into(),
        }
    }

    pub fn texture_for_face(&self, dir: FaceDir) -> &TextureRef {
        match self {
            BlockTextures::All(tex) => tex,
            BlockTextures::TopBottomSides { top, bottom, sides } => match dir {
                FaceDir::PosY => top,
                FaceDir::NegY => bottom,
                _ => sides,
            },
            BlockTextures::Explicit { pos_x, neg_x, pos_y, neg_y, pos_z, neg_z } => match dir {
                FaceDir::PosX => pos_x,
                FaceDir::NegX => neg_x,
                FaceDir::PosY => pos_y,
                FaceDir::NegY => neg_y,
                FaceDir::PosZ => pos_z,
                FaceDir::NegZ => neg_z,
            },
        }
    }

    pub fn visit_refs(&self, mut visit: impl FnMut(&TextureRef)) {
        match self {
            BlockTextures::All(tex) => visit(tex),
            BlockTextures::TopBottomSides { top, bottom, sides } => {
                visit(top);
                visit(bottom);
                visit(sides);
            }
            BlockTextures::Explicit { pos_x, neg_x, pos_y, neg_y, pos_z, neg_z } => {
                visit(pos_x);
                visit(neg_x);
                visit(pos_y);
                visit(neg_y);
                visit(pos_z);
                visit(neg_z);
            }
        }
    }
}
