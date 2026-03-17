use crate::{
    config::PlayerConfig,
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
            position: Vec3::new(config.spawn_x, config.spawn_y, config.spawn_z),
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
    pub fn forward_flat(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, -self.yaw.cos()).normalize_or_zero()
    }

    #[inline]
    pub fn right_flat(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin()).normalize_or_zero()
    }

    #[inline]
    pub fn forward_3d(&self) -> Vec3 {
        let cp = self.pitch.cos();
        let sp = self.pitch.sin();
        let sy = self.yaw.sin();
        let cy = self.yaw.cos();

        Vec3::new(sy * cp, sp, -cy * cp).normalize_or_zero()
    }
}

pub fn camera_from_player(player: &Player, aspect: f32) -> Camera {
    Camera::new(player.eye_position(), player.forward_3d(), aspect)
}
