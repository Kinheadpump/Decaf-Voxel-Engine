use crate::engine::{core::types::CHUNK_SIZE_I32, render::gpu_types::DrawRef};

pub(super) fn transparent_batch_center(origin: glam::IVec3, face_dir: u32) -> glam::Vec3 {
    let min = origin.as_vec3();
    let half = CHUNK_SIZE_I32 as f32 * 0.5;
    let center = min + glam::Vec3::splat(half);

    match face_dir {
        0 => center + glam::Vec3::new(half, 0.0, 0.0),
        1 => center - glam::Vec3::new(half, 0.0, 0.0),
        2 => center + glam::Vec3::new(0.0, half, 0.0),
        3 => center - glam::Vec3::new(0.0, half, 0.0),
        4 => center + glam::Vec3::new(0.0, 0.0, half),
        _ => center - glam::Vec3::new(0.0, 0.0, half),
    }
}

pub(super) fn build_draw_ref_bytes(max_draws: usize, draw_ref_stride: usize) -> Vec<u8> {
    let mut draw_ref_bytes = vec![0u8; max_draws * draw_ref_stride];

    for draw_index in 0..max_draws {
        let offset = draw_index * draw_ref_stride;
        let draw_ref = DrawRef { draw_meta_index: draw_index as u32, _pad: [0; 3] };
        let draw_ref_size = std::mem::size_of::<DrawRef>();
        draw_ref_bytes[offset..offset + draw_ref_size]
            .copy_from_slice(bytemuck::bytes_of(&draw_ref));
    }

    draw_ref_bytes
}

pub(super) fn next_face_capacity(current_capacity: u32, required_faces: u32) -> u32 {
    if current_capacity == 0 {
        return required_faces.max(1);
    }

    let mut capacity = current_capacity;

    while capacity < required_faces {
        capacity = capacity.saturating_mul(2);
        if capacity == u32::MAX {
            break;
        }
    }

    capacity.max(required_faces)
}

#[cfg(test)]
mod tests {
    use super::{build_draw_ref_bytes, next_face_capacity};
    use crate::engine::render::gpu_types::DrawRef;

    #[test]
    fn capacity_stays_when_requirement_fits() {
        assert_eq!(next_face_capacity(1024, 768), 1024);
    }

    #[test]
    fn capacity_grows_by_doubling_until_requirement_fits() {
        assert_eq!(next_face_capacity(1024, 1500), 2048);
    }

    #[test]
    fn zero_capacity_grows_to_requirement() {
        assert_eq!(next_face_capacity(0, 300), 300);
    }

    #[test]
    fn draw_ref_bytes_encode_sequential_draw_indices() {
        let bytes = build_draw_ref_bytes(3, 256);

        let first = bytemuck::from_bytes::<DrawRef>(&bytes[0..std::mem::size_of::<DrawRef>()]);
        let second =
            bytemuck::from_bytes::<DrawRef>(&bytes[256..256 + std::mem::size_of::<DrawRef>()]);
        let third =
            bytemuck::from_bytes::<DrawRef>(&bytes[512..512 + std::mem::size_of::<DrawRef>()]);

        assert_eq!(first.draw_meta_index, 0);
        assert_eq!(second.draw_meta_index, 1);
        assert_eq!(third.draw_meta_index, 2);
    }
}
