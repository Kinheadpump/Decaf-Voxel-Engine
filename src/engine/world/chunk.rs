use crate::engine::{
    core::types::{CHUNK_SIZE, CHUNK_VOLUME},
    world::{coord::LocalVoxelPos, voxel::Voxel},
};

pub const CHUNK_COLUMN_COUNT: usize = CHUNK_SIZE * CHUNK_SIZE;
pub const DEFAULT_BIOME_TINT: [u8; 3] = [255, 255, 255];

#[inline]
pub fn voxel_index(x: usize, y: usize, z: usize) -> usize {
    debug_assert!(x < CHUNK_SIZE);
    debug_assert!(y < CHUNK_SIZE);
    debug_assert!(z < CHUNK_SIZE);
    x + z * CHUNK_SIZE + y * CHUNK_SIZE * CHUNK_SIZE
}

#[inline]
pub fn column_index(x: usize, z: usize) -> usize {
    debug_assert!(x < CHUNK_SIZE);
    debug_assert!(z < CHUNK_SIZE);
    x + z * CHUNK_SIZE
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnBiomeTints {
    pub grass: [u8; 3],
    pub foliage: [u8; 3],
}

impl Default for ColumnBiomeTints {
    fn default() -> Self {
        Self { grass: DEFAULT_BIOME_TINT, foliage: DEFAULT_BIOME_TINT }
    }
}

#[derive(Clone)]
pub struct Chunk {
    pub voxels: Box<[Voxel; CHUNK_VOLUME]>,
    pub column_biome_tints: Box<[ColumnBiomeTints; CHUNK_COLUMN_COUNT]>,
    pub generation: u32,
}

impl Chunk {
    pub fn new() -> Self {
        Self {
            voxels: Box::new([Voxel::AIR; CHUNK_VOLUME]),
            column_biome_tints: Box::new([ColumnBiomeTints::default(); CHUNK_COLUMN_COUNT]),
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
        self.generation = self.generation.wrapping_add(1);
    }

    /// Reads a voxel using a chunk-local position that is already known to be
    /// within `0..CHUNK_SIZE` on each axis.
    #[inline]
    pub fn get_local(&self, local: impl Into<LocalVoxelPos>) -> Voxel {
        let local = local.into();
        self.get(local.x(), local.y(), local.z())
    }

    /// Writes a voxel using a chunk-local position that is already known to be
    /// within `0..CHUNK_SIZE` on each axis.
    #[inline]
    pub fn set_local(&mut self, local: impl Into<LocalVoxelPos>, voxel: Voxel) {
        let local = local.into();
        self.set(local.x(), local.y(), local.z(), voxel);
    }

    #[inline]
    pub fn bump_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    #[inline]
    pub fn biome_tints(&self, x: usize, z: usize) -> ColumnBiomeTints {
        self.column_biome_tints[column_index(x, z)]
    }

    #[inline]
    pub fn set_biome_tints(&mut self, x: usize, z: usize, tints: ColumnBiomeTints) {
        self.column_biome_tints[column_index(x, z)] = tints;
    }
}
