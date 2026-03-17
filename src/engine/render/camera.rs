use bytemuck::{Pod, Zeroable};

use crate::engine::core::math::{Mat4, Vec3};

pub struct Camera {
    pub position: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    pub aspect: f32,
    pub fov_y_radians: f32,
    pub near_plane: f32,
}

impl Camera {
    pub fn new(position: Vec3, forward: Vec3, aspect: f32) -> Self {
        Self {
            position,
            forward: forward.normalize(),
            up: Vec3::Y,
            aspect,
            fov_y_radians: 70.0_f32.to_radians(),
            near_plane: 0.1,
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
        f / aspect, 0.0, 0.0, 0.0,
        0.0, f, 0.0, 0.0,
        0.0, 0.0, 0.0, -1.0,
        0.0, 0.0, z_near, 0.0,
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