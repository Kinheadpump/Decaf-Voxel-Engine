use bytemuck::{Pod, Zeroable};

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct PackedFace(pub u32);

impl PackedFace {
    // bits:
    // 0..=4   x
    // 5..=9   y
    // 10..=14 z
    // 15..=21 block id (7 bits)
    // 22..=26 width_minus_1
    // 27..=31 height_minus_1
    pub fn pack(x: u32, y: u32, z: u32, block_id: u32, wm1: u32, hm1: u32) -> Self {
        debug_assert!(x < 32);
        debug_assert!(y < 32);
        debug_assert!(z < 32);
        debug_assert!(block_id < 128);
        debug_assert!(wm1 < 32);
        debug_assert!(hm1 < 32);

        Self(
            (x << 0) |
            (y << 5) |
            (z << 10) |
            (block_id << 15) |
            (wm1 << 22) |
            (hm1 << 27)
        )
    }
}

pub struct ChunkMeshCpu {
    pub faces: [Vec<PackedFace>; 6],
    pub source_generation: u32,
}

impl ChunkMeshCpu {
    pub fn new() -> Self {
        Self {
            faces: std::array::from_fn(|_| Vec::new()),
            source_generation: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DrawMeta {
    pub chunk_origin: [i32; 4],
    pub face_dir: u32,
    pub face_offset: u32,
    pub face_count: u32,
    pub _pad0: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DrawIndirectArgs {
    pub vertex_count: u32,
    pub instance_count: u32,
    pub first_vertex: u32,
    pub first_instance: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct BaseQuadVertex {
    pub uv: [f32; 2],
}