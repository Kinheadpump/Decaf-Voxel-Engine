use serde::Deserialize;
use std::fs;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub debug: DebugConfig,
    pub render: RenderConfig,
    pub player: PlayerConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DebugConfig {
    pub enable_profiler: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RenderConfig {
    pub render_radius_xz: i32,
    pub render_radius_y: i32,
    pub stream_generation_budget: usize,
    pub enable_hiz_occlusion: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlayerConfig {
    pub spawn_x: f32,
    pub spawn_y: f32,
    pub spawn_z: f32,
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

impl Config {
    pub fn load() -> Self {
        let config_str = fs::read_to_string("config.toml")
            .expect("Failed to read config.toml! Ensure the file exists in the project root.");
        toml::from_str(&config_str).expect("Failed to parse config.toml!")
    }
}
