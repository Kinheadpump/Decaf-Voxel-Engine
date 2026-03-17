use crate::engine::{
    core::types::{CHUNK_SIZE, CHUNK_VOLUME},
    world::voxel::Voxel,
};

#[inline]
pub fn voxel_index(x: usize, y: usize, z: usize) -> usize {
    x + z * CHUNK_SIZE + y * CHUNK_SIZE * CHUNK_SIZE
}

pub struct Chunk {
    pub voxels: Box<[Voxel; CHUNK_VOLUME]>,
    pub dirty: bool,
    pub generation: u32,
}

impl Chunk {
    pub fn new() -> Self {
        Self {
            voxels: Box::new([Voxel::AIR; CHUNK_VOLUME]),
            dirty: true,
            generation: 0,
        }
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> Voxel {
        self.voxels[voxel_index(x, y, z)]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, voxel: Voxel) {
        self.voxels[voxel_index(x, y, z)] = voxel;
        self.dirty = true;
        self.generation = self.generation.wrapping_add(1);
    }
}