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
    pub near_plane: f32,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self { fov_y_degrees: 70.0, near_plane: 0.1 }
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
            max_glyphs: 256,
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

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct WorldConfig {
    pub seed: u64,
    pub surface_level: i32,
    pub soil_depth: i32,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self { seed: 12345, surface_level: 0, soil_depth: 4 }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct PlayerConfig {
    pub spawn_x: f32,
    pub spawn_y: f32,
    pub spawn_z: f32,
    pub reach_distance: f32,
    pub mouse_sensitivity: f32,
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
            spawn_x: 0.0,
            spawn_y: 10.0,
            spawn_z: 0.0,
            reach_distance: 6.0,
            mouse_sensitivity: 0.0022,
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

impl Config {
    pub fn load() -> Self {
        let config_str = fs::read_to_string("config.toml")
            .expect("Failed to read config.toml! Ensure the file exists in the project root.");
        toml::from_str(&config_str).expect("Failed to parse config.toml!")
    }
}
