use std::sync::Arc;

use bytemuck::{Pod, Zeroable};

use crate::{
    config::SkyConfig,
    engine::core::types::{CHUNK_SIZE_U32, CHUNK_VOLUME, MAX_TEXTURE_LAYERS},
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct PackedFace {
    pub value: u32,
    pub tint: u32,
}

impl PackedFace {
    // bits:
    // 0..=4   x
    // 5..=9   y
    // 10..=14 z
    // 15..=21 texture layer id (7 bits)
    // 22..=26 width_minus_1
    // 27..=31 height_minus_1
    pub fn pack(
        x: u32,
        y: u32,
        z: u32,
        texture_id: u32,
        width_minus_one: u32,
        height_minus_one: u32,
        tint: u32,
    ) -> Self {
        debug_assert!(x < CHUNK_SIZE_U32);
        debug_assert!(y < CHUNK_SIZE_U32);
        debug_assert!(z < CHUNK_SIZE_U32);
        debug_assert!(texture_id < MAX_TEXTURE_LAYERS);
        debug_assert!(width_minus_one < CHUNK_SIZE_U32);
        debug_assert!(height_minus_one < CHUNK_SIZE_U32);

        Self {
            value: x
                | (y << 5)
                | (z << 10)
                | (texture_id << 15)
                | (width_minus_one << 22)
                | (height_minus_one << 27),
            tint,
        }
    }
}

pub const FACE_TINT_MODE_NONE: u32 = 0;
pub const FACE_TINT_MODE_GRASS: u32 = 1;
pub const FACE_TINT_MODE_MULTIPLY: u32 = 2;
pub const DEFAULT_FACE_TINT: u32 = pack_face_tint(FACE_TINT_MODE_NONE, [255, 255, 255]);

#[inline]
pub const fn pack_rgb8(color: [u8; 3]) -> u32 {
    (color[0] as u32) | ((color[1] as u32) << 8) | ((color[2] as u32) << 16)
}

#[inline]
pub const fn pack_face_tint(mode: u32, color: [u8; 3]) -> u32 {
    pack_rgb8(color) | ((mode & 0xff) << 24)
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
pub struct DrawRef {
    pub draw_meta_index: u32,
    pub _pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct GpuDrawIndirect {
    pub vertex_count: u32,
    pub instance_count: u32,
    pub first_vertex: u32,
    pub first_instance: u32,
}

impl GpuDrawIndirect {
    pub fn for_draw(draw_index: u32, instance_count: u32) -> Self {
        Self {
            vertex_count: 4,
            instance_count,
            first_vertex: 0,
            first_instance: draw_index * CHUNK_VOLUME as u32,
        }
    }
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
    Wireframe = 4,
}

impl DebugViewMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Shaded => "Shaded",
            Self::FaceDir => "FaceDir",
            Self::ChunkCoord => "ChunkCoord",
            Self::DrawId => "DrawId",
            Self::Wireframe => "Wireframe",
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct RenderSettingsUniform {
    pub debug_view_mode: u32,
    pub chunk_size: u32,
    pub draw_index_mode: u32,
    pub time_seconds: f32,
}

impl RenderSettingsUniform {
    pub fn new(debug_view_mode: DebugViewMode, draw_index_mode: u32, time_seconds: f32) -> Self {
        Self {
            debug_view_mode: debug_view_mode as u32,
            chunk_size: CHUNK_SIZE_U32,
            draw_index_mode,
            time_seconds,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct SkyUniform {
    pub zenith_color: [f32; 4],
    pub horizon_color: [f32; 4],
    pub cloud_color: [f32; 4],
    pub sun_color: [f32; 4],
    pub sun_direction: [f32; 4],
    pub sky_params: [f32; 4],
    pub cloud_params: [f32; 4],
    pub cloud_style: [f32; 4],
}

impl SkyUniform {
    pub fn from_config(config: SkyConfig) -> Self {
        let azimuth = config.sun_azimuth_degrees.to_radians();
        let elevation = config.sun_elevation_degrees.to_radians();
        let sun_direction = glam::Vec3::new(
            elevation.cos() * azimuth.cos(),
            elevation.sin(),
            elevation.cos() * azimuth.sin(),
        )
        .normalize_or_zero();
        let sun_disc_cos = (config.sun_disc_size_degrees.to_radians()).cos();

        Self {
            zenith_color: [
                config.zenith_color[0],
                config.zenith_color[1],
                config.zenith_color[2],
                1.0,
            ],
            horizon_color: [
                config.horizon_color[0],
                config.horizon_color[1],
                config.horizon_color[2],
                1.0,
            ],
            cloud_color: [config.cloud_color[0], config.cloud_color[1], config.cloud_color[2], 1.0],
            sun_color: [config.sun_color[0], config.sun_color[1], config.sun_color[2], 1.0],
            sun_direction: [sun_direction.x, sun_direction.y, sun_direction.z, 0.0],
            sky_params: [
                sun_disc_cos,
                config.sun_glow_power,
                config.sun_glow_intensity,
                f32::from(config.enabled),
            ],
            cloud_params: [
                config.cloud_scale,
                config.cloud_height,
                config.cloud_speed,
                config.cloud_coverage,
            ],
            cloud_style: [config.cloud_softness, config.cloud_opacity, 0.0, 0.0],
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

#[derive(Clone, Debug)]
pub struct DebugOverlayInput {
    pub show_debug: bool,
    pub show_game_hud: bool,
    pub fps: u32,
    pub loaded_chunks: u32,
    pub player_voxel: [i32; 3],
    pub player_chunk: [i32; 3],
    pub player_facing: &'static str,
    pub biome_name: Arc<str>,
    pub biome_priority: i32,
    pub region_name: &'static str,
    pub ground_y: i32,
    pub biome_altitude_y: i32,
    pub temperature_percent: u8,
    pub humidity_percent: u8,
    pub continentalness_percent: u8,
    pub biome_temperature_min_percent: u8,
    pub biome_temperature_max_percent: u8,
    pub biome_humidity_min_percent: u8,
    pub biome_humidity_max_percent: u8,
    pub biome_altitude_min: Option<i32>,
    pub biome_altitude_max: Option<i32>,
    pub biome_continentalness_min_percent: Option<u8>,
    pub biome_continentalness_max_percent: Option<u8>,
    pub hotbar_line: Arc<str>,
}

impl Default for DebugOverlayInput {
    fn default() -> Self {
        Self {
            show_debug: false,
            show_game_hud: true,
            fps: 0,
            loaded_chunks: 0,
            player_voxel: [0; 3],
            player_chunk: [0; 3],
            player_facing: "",
            biome_name: Arc::<str>::from(""),
            biome_priority: 0,
            region_name: "",
            ground_y: 0,
            biome_altitude_y: 0,
            temperature_percent: 0,
            humidity_percent: 0,
            continentalness_percent: 0,
            biome_temperature_min_percent: 0,
            biome_temperature_max_percent: 100,
            biome_humidity_min_percent: 0,
            biome_humidity_max_percent: 100,
            biome_altitude_min: None,
            biome_altitude_max: None,
            biome_continentalness_min_percent: None,
            biome_continentalness_max_percent: None,
            hotbar_line: Arc::<str>::from(""),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub gpu_chunks: u32,
    pub drawn_chunks: u32,
    pub frustum_culled_chunks: u32,
    pub occlusion_culled_chunks: u32,
    pub directional_culled_draws: u32,
    pub opaque_draws: u32,
    pub transparent_draws: u32,
    pub meshing_pending_chunks: u32,
    pub meshing_faces_uploaded: u32,
    pub meshing_slice_buffer_growths: u32,
    pub hiz_enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::{RenderSettingsUniform, SkyUniform};
    use crate::config::SkyConfig;

    #[test]
    fn render_settings_uniform_stays_16_bytes() {
        assert_eq!(std::mem::size_of::<RenderSettingsUniform>(), 16);
    }

    #[test]
    fn sky_uniform_stays_vec4_aligned() {
        assert_eq!(std::mem::size_of::<SkyUniform>(), 128);
    }

    #[test]
    fn sky_uniform_sun_direction_is_normalized() {
        let uniform = SkyUniform::from_config(SkyConfig::default());
        let direction = glam::Vec3::from_array([
            uniform.sun_direction[0],
            uniform.sun_direction[1],
            uniform.sun_direction[2],
        ]);

        assert!((direction.length() - 1.0).abs() < 1.0e-5);
    }
}
