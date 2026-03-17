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
    TopBottomSides { top: TextureRef, bottom: TextureRef, sides: TextureRef },
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

    pub fn visit_refs(&self, mut visit: impl FnMut(&TextureRef)) {
        match self {
            BlockTextures::All(tex) => visit(tex),
            BlockTextures::TopBottomSides { top, bottom, sides } => {
                visit(top);
                visit(bottom);
                visit(sides);
            }
        }
    }
}
