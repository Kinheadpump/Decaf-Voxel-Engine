use bytemuck::{Pod, Zeroable};

use crate::{
    config::CameraConfig,
    engine::core::math::{Mat4, Vec3},
};

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub position: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    pub aspect: f32,
    pub fov_y_radians: f32,
    pub near_plane: f32,
}

impl Camera {
    pub fn from_config(position: Vec3, forward: Vec3, aspect: f32, config: &CameraConfig) -> Self {
        Self {
            position,
            forward: forward.normalize(),
            up: Vec3::Y,
            aspect,
            fov_y_radians: config.fov_y_degrees.to_radians(),
            near_plane: config.near_plane,
        }
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_to_rh(self.position, self.forward, self.up)
    }

    pub fn proj(&self) -> Mat4 {
        perspective_reverse_infinite_rh(self.fov_y_radians, self.aspect, self.near_plane)
    }

    pub fn view_proj(&self) -> Mat4 {
        self.proj() * self.view()
    }

    pub fn build_uniform(&self) -> CameraUniform {
        let view = self.view();
        let proj = self.proj();
        let view_proj = proj * view;

        CameraUniform {
            view: view.to_cols_array_2d(),
            proj: proj.to_cols_array_2d(),
            view_proj: view_proj.to_cols_array_2d(),
            inv_view: view.inverse().to_cols_array_2d(),
            inv_proj: proj.inverse().to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            camera_pos: [self.position.x, self.position.y, self.position.z, 1.0],
            near_plane: self.near_plane,
            _pad: [0.0; 3],
        }
    }
}

pub fn perspective_reverse_infinite_rh(fovy: f32, aspect: f32, z_near: f32) -> Mat4 {
    let f = 1.0 / (fovy * 0.5).tan();

    Mat4::from_cols_array(&[
        f / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        -1.0,
        0.0,
        0.0,
        z_near,
        0.0,
    ])
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct CameraUniform {
    pub view: [[f32; 4]; 4],
    pub proj: [[f32; 4]; 4],
    pub view_proj: [[f32; 4]; 4],
    pub inv_view: [[f32; 4]; 4],
    pub inv_proj: [[f32; 4]; 4],
    pub inv_view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],
    pub near_plane: f32,
    pub _pad: [f32; 3],
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec4;

    #[test]
    fn reverse_z_maps_near_plane_to_one() {
        let config = CameraConfig::default();
        let near = config.near_plane;
        let proj =
            perspective_reverse_infinite_rh(config.fov_y_degrees.to_radians(), 16.0 / 9.0, near);
        let clip = proj * Vec4::new(0.0, 0.0, -near, 1.0);
        let ndc_z = clip.z / clip.w;

        assert!((ndc_z - 1.0).abs() < 1.0e-5, "expected near plane at 1.0, got {ndc_z}");
    }

    #[test]
    fn reverse_z_pushes_far_points_towards_zero() {
        let config = CameraConfig::default();
        let proj = perspective_reverse_infinite_rh(
            config.fov_y_degrees.to_radians(),
            16.0 / 9.0,
            config.near_plane,
        );
        let clip = proj * Vec4::new(0.0, 0.0, -10_000.0, 1.0);
        let ndc_z = clip.z / clip.w;

        assert!(ndc_z > 0.0);
        assert!(ndc_z < 0.001, "expected distant depth near zero, got {ndc_z}");
    }
}
