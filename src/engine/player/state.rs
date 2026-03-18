use crate::{
    config::{CameraConfig, PlayerConfig},
    engine::{core::math::Vec3, render::camera::Camera},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementMode {
    Walking,
    Flying,
}

#[derive(Debug, Clone)]
pub struct Player {
    pub position: Vec3,
    pub velocity: Vec3,

    pub yaw: f32,
    pub pitch: f32,

    pub on_ground: bool,
    pub movement_mode: MovementMode,

    pub wants_jump_hold: bool,
    pub last_space_press_time: f32,
    pub space_press_count: u8,

    pub eye_height: f32,
    pub radius: f32,
    pub height: f32,
}

impl Player {
    pub fn from_config(config: &PlayerConfig) -> Self {
        Self {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            movement_mode: MovementMode::Walking,
            wants_jump_hold: false,
            last_space_press_time: -1000.0,
            space_press_count: 0,
            eye_height: config.eye_height,
            radius: config.radius,
            height: config.height,
        }
    }

    #[inline]
    pub fn eye_position(&self) -> Vec3 {
        self.position + Vec3::new(0.0, self.eye_height, 0.0)
    }

    #[inline]
    pub fn aabb(&self) -> (Vec3, Vec3) {
        let min = Vec3::new(
            self.position.x - self.radius,
            self.position.y,
            self.position.z - self.radius,
        );
        let max = Vec3::new(
            self.position.x + self.radius,
            self.position.y + self.height,
            self.position.z + self.radius,
        );
        (min, max)
    }

    #[inline]
    pub fn forward_flat(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, -self.yaw.cos()).normalize_or_zero()
    }

    #[inline]
    pub fn right_flat(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin()).normalize_or_zero()
    }

    #[inline]
    pub fn forward_3d(&self) -> Vec3 {
        let cosine_pitch = self.pitch.cos();
        let sine_pitch = self.pitch.sin();
        let sine_yaw = self.yaw.sin();
        let cosine_yaw = self.yaw.cos();

        Vec3::new(sine_yaw * cosine_pitch, sine_pitch, -cosine_yaw * cosine_pitch)
            .normalize_or_zero()
    }

    #[inline]
    pub fn cardinal_facing(&self) -> &'static str {
        let forward = self.forward_flat();

        if forward.z.abs() >= forward.x.abs() {
            if forward.z <= 0.0 { "NORTH" } else { "SOUTH" }
        } else if forward.x >= 0.0 {
            "EAST"
        } else {
            "WEST"
        }
    }
}

pub fn camera_from_player(
    player: &Player,
    aspect: f32,
    camera_config: &CameraConfig,
    zoom_active: bool,
) -> Camera {
    let camera_config = camera_config.for_zoom_state(zoom_active);
    Camera::from_config(player.eye_position(), player.forward_3d(), aspect, &camera_config)
}
