use serde::Deserialize;
use std::fs;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub debug: DebugConfig,
    pub window: WindowConfig,
    pub render: RenderConfig,
    pub player: PlayerConfig,
    pub world: WorldConfig,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct DebugConfig {
    pub enable_profiler: bool,
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self { enable_profiler: true }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self { width: 1280, height: 720 }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RenderConfig {
    pub render_radius_xz: i32,
    pub render_radius_y: i32,
    pub stream_generation_budget: usize,
    pub stream_max_inflight_generations: usize,
    pub generation_worker_count: usize,
    pub meshing_worker_count: usize,
    pub enable_hiz_occlusion: bool,
    pub initial_face_capacity: u32,
    pub max_visible_draws: usize,
    pub camera: CameraConfig,
    pub clear_color: ClearColorConfig,
    pub overlay: OverlayConfig,
    pub hiz: HiZOcclusionConfig,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            render_radius_xz: 4,
            render_radius_y: 1,
            stream_generation_budget: 16,
            stream_max_inflight_generations: 64,
            generation_worker_count: 0,
            meshing_worker_count: 0,
            enable_hiz_occlusion: false,
            initial_face_capacity: 4_000_000,
            max_visible_draws: 32_768,
            camera: CameraConfig::default(),
            clear_color: ClearColorConfig::default(),
            overlay: OverlayConfig::default(),
            hiz: HiZOcclusionConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct CameraConfig {
    pub fov_y_degrees: f32,
    pub zoom_fov_y_degrees: f32,
    pub near_plane: f32,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self { fov_y_degrees: 70.0, zoom_fov_y_degrees: 30.0, near_plane: 0.1 }
    }
}

impl CameraConfig {
    #[inline]
    pub fn for_zoom_state(&self, zoom_active: bool) -> Self {
        Self {
            fov_y_degrees: if zoom_active { self.zoom_fov_y_degrees } else { self.fov_y_degrees },
            ..*self
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct ClearColorConfig {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

impl Default for ClearColorConfig {
    fn default() -> Self {
        Self { r: 0.45, g: 0.70, b: 0.95 }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    pub max_glyphs: usize,
    pub glyph_width: f32,
    pub glyph_height: f32,
    pub padding_x: f32,
    pub padding_y: f32,
    pub glyph_advance_x: f32,
    pub line_advance_y: f32,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            max_glyphs: 384,
            glyph_width: 12.0,
            glyph_height: 15.0,
            padding_x: 8.0,
            padding_y: 8.0,
            glyph_advance_x: 13.0,
            line_advance_y: 18.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct HiZOcclusionConfig {
    pub max_base_dimension: u32,
    pub readback_slots: usize,
    pub max_passes: usize,
    pub max_camera_reuse_distance: f32,
    pub min_camera_reuse_forward_dot: f32,
    pub near_chunk_skip_distance: f32,
    pub occlusion_depth_bias: f32,
}

impl Default for HiZOcclusionConfig {
    fn default() -> Self {
        Self {
            max_base_dimension: 256,
            readback_slots: 3,
            max_passes: 16,
            max_camera_reuse_distance: 8.0,
            min_camera_reuse_forward_dot: 0.97,
            near_chunk_skip_distance: 48.0,
            occlusion_depth_bias: 1.0e-4,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WorldConfig {
    pub seed: u64,
    pub biomes_file: String,
    pub terrain: TerrainConfig,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            seed: 12345,
            biomes_file: "biomes.toml".to_string(),
            terrain: TerrainConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct TerrainConfig {
    pub sea_level: i32,
    pub dirt_depth: u8,
    pub continentalness: NoiseConfig,
    pub continentalness_contrast: f32,
    pub detail: NoiseConfig,
    pub detail_amplitude: f32,
    pub biome_blend: f32,
    pub climate_contrast: f32,
    pub temperature: NoiseConfig,
    pub humidity: NoiseConfig,
    pub mountain_start_roughness: f32,
    pub mountain_peak_boost: f32,
    pub mountain_peak_sharpness: f32,
    pub continental_regions: ContinentalRegionsConfig,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            sea_level: 0,
            dirt_depth: 2,
            continentalness: NoiseConfig {
                scale: 0.0022,
                octaves: 2,
                persistence: 0.55,
                lacunarity: 2.0,
            },
            continentalness_contrast: 1.3,
            detail: NoiseConfig { scale: 0.0095, octaves: 5, persistence: 0.52, lacunarity: 2.0 },
            detail_amplitude: 56.0,
            biome_blend: 0.10,
            climate_contrast: 1.35,
            temperature: NoiseConfig {
                scale: 0.0048,
                octaves: 2,
                persistence: 0.5,
                lacunarity: 2.0,
            },
            humidity: NoiseConfig { scale: 0.0048, octaves: 2, persistence: 0.5, lacunarity: 2.0 },
            mountain_start_roughness: 1.25,
            mountain_peak_boost: 128.0,
            mountain_peak_sharpness: 1.85,
            continental_regions: ContinentalRegionsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct NoiseConfig {
    pub scale: f32,
    pub octaves: u32,
    pub persistence: f32,
    pub lacunarity: f32,
}

impl Default for NoiseConfig {
    fn default() -> Self {
        Self { scale: 0.001, octaves: 4, persistence: 0.5, lacunarity: 2.0 }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct ContinentalRegionsConfig {
    pub deep_ocean: ContinentalRegionConfig,
    pub ocean: ContinentalRegionConfig,
    pub coast: ContinentalRegionConfig,
    pub plains: ContinentalRegionConfig,
    pub highlands: ContinentalRegionConfig,
    pub mountains: ContinentalRegionConfig,
}

impl Default for ContinentalRegionsConfig {
    fn default() -> Self {
        Self {
            deep_ocean: ContinentalRegionConfig {
                max_value: 0.16,
                base_height: -48.0,
                roughness: 0.12,
            },
            ocean: ContinentalRegionConfig { max_value: 0.30, base_height: -28.0, roughness: 0.22 },
            coast: ContinentalRegionConfig { max_value: 0.42, base_height: -10.0, roughness: 0.40 },
            plains: ContinentalRegionConfig { max_value: 0.62, base_height: 14.0, roughness: 0.85 },
            highlands: ContinentalRegionConfig {
                max_value: 0.80,
                base_height: 42.0,
                roughness: 1.95,
            },
            mountains: ContinentalRegionConfig {
                max_value: 1.0,
                base_height: 124.0,
                roughness: 3.60,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct ContinentalRegionConfig {
    pub max_value: f32,
    pub base_height: f32,
    pub roughness: f32,
}

impl Default for ContinentalRegionConfig {
    fn default() -> Self {
        ContinentalRegionsConfig::default().plains
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct PlayerConfig {
    pub reach_distance: f32,
    pub mouse_sensitivity: f32,
    pub zoom_mouse_sensitivity_multiplier: f32,
    pub eye_height: f32,
    pub radius: f32,
    pub height: f32,
    pub double_tap_window: f32,
    pub collision_steps: usize,
    pub walk_speed: f32,
    pub walk_sprint_multiplier: f32,
    pub walk_accel: f32,
    pub air_accel: f32,
    pub ground_friction: f32,
    pub air_friction: f32,
    pub jump_speed: f32,
    pub gravity: f32,
    pub fly_speed: f32,
    pub fly_sprint_multiplier: f32,
    pub fly_accel: f32,
    pub fly_friction: f32,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            reach_distance: 6.0,
            mouse_sensitivity: 0.0022,
            zoom_mouse_sensitivity_multiplier: 0.45,
            eye_height: 1.62,
            radius: 0.3,
            height: 1.8,
            double_tap_window: 0.25,
            collision_steps: 3,
            walk_speed: 5.0,
            walk_sprint_multiplier: 1.8,
            walk_accel: 45.0,
            air_accel: 10.0,
            ground_friction: 14.0,
            air_friction: 1.0,
            jump_speed: 8.5,
            gravity: 24.0,
            fly_speed: 7.0,
            fly_sprint_multiplier: 2.5,
            fly_accel: 24.0,
            fly_friction: 10.0,
        }
    }
}

impl PlayerConfig {
    #[inline]
    pub fn look_sensitivity(&self, zoom_active: bool) -> f32 {
        if zoom_active {
            self.mouse_sensitivity * self.zoom_mouse_sensitivity_multiplier
        } else {
            self.mouse_sensitivity
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let config_str = fs::read_to_string("config.toml")
            .expect("Failed to read config.toml! Ensure the file exists in the project root.");
        toml::from_str(&config_str).expect("Failed to parse config.toml!")
    }
}
