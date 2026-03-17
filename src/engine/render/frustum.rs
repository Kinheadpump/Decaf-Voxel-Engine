use crate::engine::core::math::{Mat4, Vec3};

#[derive(Clone, Copy)]
pub struct Frustum {
    pub view_proj: Mat4,
}

impl Frustum {
    pub fn from_view_proj(view_proj: Mat4) -> Self {
        Self { view_proj }
    }

    // TODO: this is a very naive implementation that just tests the AABB against the frustum planes.
    pub fn test_aabb(&self, _min: Vec3, _max: Vec3) -> bool {
        true
    }
}