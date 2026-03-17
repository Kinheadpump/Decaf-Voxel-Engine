use bytemuck::{Pod, Zeroable};

use crate::engine::core::types::{CHUNK_SIZE_U32, MAX_TEXTURE_LAYERS};

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct PackedFace(pub u32);

impl PackedFace {
    // bits:
    // 0..=4   x
    // 5..=9   y
    // 10..=14 z
    // 15..=21 texture layer id (7 bits)
    // 22..=26 width_minus_1
    // 27..=31 height_minus_1
    pub fn pack(x: u32, y: u32, z: u32, texture_id: u32, wm1: u32, hm1: u32) -> Self {
        debug_assert!(x < 32);
        debug_assert!(y < 32);
        debug_assert!(z < 32);
        debug_assert!(texture_id < MAX_TEXTURE_LAYERS);
        debug_assert!(wm1 < 32);
        debug_assert!(hm1 < 32);

        Self((x << 0) | (y << 5) | (z << 10) | (texture_id << 15) | (wm1 << 22) | (hm1 << 27))
    }
}

#[repr(usize)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderBucket {
    #[default]
    Opaque = 0,
    Transparent = 1,
}

impl RenderBucket {
    pub const ALL: [Self; 2] = [Self::Opaque, Self::Transparent];
}

pub struct ChunkMeshCpu {
    pub faces: [[Vec<PackedFace>; 6]; 2],
    pub source_generation: u32,
}

impl ChunkMeshCpu {
    pub fn new() -> Self {
        Self {
            faces: std::array::from_fn(|_| std::array::from_fn(|_| Vec::new())),
            source_generation: 0,
        }
    }

    pub fn face_count(&self) -> u32 {
        self.faces.iter().flat_map(|dirs| dirs.iter()).map(|faces| faces.len() as u32).sum()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DrawMeta {
    pub chunk_origin: [i32; 4],
    pub face_dir: u32,
    pub face_offset: u32,
    pub face_count: u32,
    pub draw_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct BaseQuadVertex {
    pub uv: [f32; 2],
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DebugViewMode {
    #[default]
    Shaded = 0,
    FaceDir = 1,
    ChunkCoord = 2,
    DrawId = 3,
}

impl DebugViewMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Shaded => "Shaded",
            Self::FaceDir => "FaceDir",
            Self::ChunkCoord => "ChunkCoord",
            Self::DrawId => "DrawId",
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct RenderSettingsUniform {
    pub debug_view_mode: u32,
    pub chunk_size: u32,
    pub _pad0: u32,
    pub _pad1: u32,
}

impl RenderSettingsUniform {
    pub fn new(debug_view_mode: DebugViewMode) -> Self {
        Self {
            debug_view_mode: debug_view_mode as u32,
            chunk_size: CHUNK_SIZE_U32,
            _pad0: 0,
            _pad1: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct TextOverlayUniform {
    pub screen_size: [f32; 2],
    pub _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct TextGlyphInstance {
    pub origin_px: [f32; 2],
    pub size_px: [f32; 2],
    pub glyph_code: u32,
    pub _pad: [u32; 3],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DebugOverlayInput {
    pub fps: u32,
    pub loaded_chunks: u32,
    pub player_voxel: [i32; 3],
    pub player_chunk: [i32; 3],
    pub player_facing: &'static str,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub loaded_chunks: u32,
    pub gpu_chunks: u32,
    pub drawn_chunks: u32,
    pub frustum_culled_chunks: u32,
    pub occlusion_culled_chunks: u32,
    pub opaque_draws: u32,
    pub transparent_draws: u32,
    pub meshing_pending_chunks: u32,
    pub hiz_enabled: bool,
}
