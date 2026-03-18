use crate::engine::{
    core::types::{CHUNK_SIZE, CHUNK_VOLUME},
    world::voxel::Voxel,
};

#[inline]
pub fn voxel_index(x: usize, y: usize, z: usize) -> usize {
    x + z * CHUNK_SIZE + y * CHUNK_SIZE * CHUNK_SIZE
}

#[derive(Clone)]
pub struct Chunk {
    pub voxels: Box<[Voxel; CHUNK_VOLUME]>,
    pub dirty: bool,
    pub generation: u32,
}

impl Chunk {
    pub fn new() -> Self {
        Self { voxels: Box::new([Voxel::AIR; CHUNK_VOLUME]), dirty: true, generation: 0 }
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

    pub fn fill_with(&mut self, mut fill: impl FnMut(usize, usize, usize) -> Voxel) {
        for y in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    self.voxels[voxel_index(x, y, z)] = fill(x, y, z);
                }
            }
        }

        self.dirty = true;
        self.generation = self.generation.wrapping_add(1);
    }
}
