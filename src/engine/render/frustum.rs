use crate::engine::{core::math::Vec3, render::camera::Camera};

#[derive(Clone, Copy, Debug)]
struct Plane {
    normal: Vec3,
    distance: f32,
}

impl Plane {
    fn from_point_normal(point: Vec3, normal: Vec3) -> Self {
        let normal = normal.normalize();
        Self { normal, distance: -normal.dot(point) }
    }

    fn signed_distance(self, point: Vec3) -> f32 {
        self.normal.dot(point) + self.distance
    }
}

#[derive(Clone, Copy)]
pub struct Frustum {
    planes: [Plane; 5],
}

impl Frustum {
    pub fn from_camera(camera: &Camera) -> Self {
        let forward = camera.forward.normalize();
        let right = forward.cross(camera.up).normalize();
        let up = right.cross(forward).normalize();

        let tan_half_y = (camera.fov_y_radians * 0.5).tan();
        let tan_half_x = tan_half_y * camera.aspect;

        let left_ray = (forward - right * tan_half_x).normalize();
        let right_ray = (forward + right * tan_half_x).normalize();
        let top_ray = (forward + up * tan_half_y).normalize();
        let bottom_ray = (forward - up * tan_half_y).normalize();

        let near_center = camera.position + forward * camera.near_plane;

        Self {
            planes: [
                Plane::from_point_normal(near_center, forward),
                Plane::from_point_normal(camera.position, left_ray.cross(up)),
                Plane::from_point_normal(camera.position, up.cross(right_ray)),
                Plane::from_point_normal(camera.position, right.cross(bottom_ray)),
                Plane::from_point_normal(camera.position, top_ray.cross(right)),
            ],
        }
    }

    pub fn test_aabb(&self, min: Vec3, max: Vec3) -> bool {
        self.planes.iter().all(|plane| {
            let p = Vec3::new(
                if plane.normal.x >= 0.0 { max.x } else { min.x },
                if plane.normal.y >= 0.0 { max.y } else { min.y },
                if plane.normal.z >= 0.0 { max.z } else { min.z },
            );

            plane.signed_distance(p) >= 0.0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aabb_inside_frustum_is_visible() {
        let camera = Camera::new(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), 1.0);
        let frustum = Frustum::from_camera(&camera);

        assert!(frustum.test_aabb(Vec3::new(-1.0, -1.0, -6.0), Vec3::new(1.0, 1.0, -4.0),));
    }

    #[test]
    fn aabb_behind_camera_is_rejected() {
        let camera = Camera::new(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), 1.0);
        let frustum = Frustum::from_camera(&camera);

        assert!(!frustum.test_aabb(Vec3::new(-1.0, -1.0, 2.0), Vec3::new(1.0, 1.0, 4.0),));
    }

    #[test]
    fn aabb_outside_side_plane_is_rejected() {
        let camera = Camera::new(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), 1.0);
        let frustum = Frustum::from_camera(&camera);

        assert!(!frustum.test_aabb(Vec3::new(8.0, -1.0, -6.0), Vec3::new(10.0, 1.0, -4.0),));
    }
}
