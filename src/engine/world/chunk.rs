use std::sync::Arc;

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
    pub voxels: Arc<[Voxel; CHUNK_VOLUME]>,
    pub column_biome_tints: Arc<[ColumnBiomeTints; CHUNK_COLUMN_COUNT]>,
    pub generation: u32,
}

impl Chunk {
    pub fn new() -> Self {
        Self {
            voxels: Arc::new([Voxel::AIR; CHUNK_VOLUME]),
            column_biome_tints: Arc::new([ColumnBiomeTints::default(); CHUNK_COLUMN_COUNT]),
            generation: 0,
        }
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> Voxel {
        self.voxels[voxel_index(x, y, z)]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, voxel: Voxel) {
        self.voxels_mut()[voxel_index(x, y, z)] = voxel;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Returns mutable access to both storage arrays. Meshing snapshots clone
    /// chunks cheaply by sharing these `Arc` buffers until a later mutation
    /// forces clone-on-write.
    #[inline]
    pub fn storage_mut(
        &mut self,
    ) -> (&mut [Voxel; CHUNK_VOLUME], &mut [ColumnBiomeTints; CHUNK_COLUMN_COUNT]) {
        let voxels = Arc::make_mut(&mut self.voxels);
        let column_biome_tints = Arc::make_mut(&mut self.column_biome_tints);
        (voxels, column_biome_tints)
    }

    #[inline]
    pub fn voxels_mut(&mut self) -> &mut [Voxel; CHUNK_VOLUME] {
        Arc::make_mut(&mut self.voxels)
    }

    #[inline]
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn column_biome_tints_mut(&mut self) -> &mut [ColumnBiomeTints; CHUNK_COLUMN_COUNT] {
        Arc::make_mut(&mut self.column_biome_tints)
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
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_biome_tints(&mut self, x: usize, z: usize, tints: ColumnBiomeTints) {
        self.column_biome_tints_mut()[column_index(x, z)] = tints;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::engine::world::block::id::BlockId;

    use super::*;

    #[test]
    fn chunk_clone_shares_storage_until_write() {
        let mut chunk = Chunk::new();
        chunk.set(1, 2, 3, Voxel::from_block_id(BlockId(7)));
        chunk.set_biome_tints(
            1,
            3,
            ColumnBiomeTints { grass: [10, 20, 30], foliage: [40, 50, 60] },
        );

        let mut clone = chunk.clone();

        assert!(Arc::ptr_eq(&chunk.voxels, &clone.voxels));
        assert!(Arc::ptr_eq(&chunk.column_biome_tints, &clone.column_biome_tints));

        clone.set(1, 2, 3, Voxel::from_block_id(BlockId(9)));
        clone.set_biome_tints(
            1,
            3,
            ColumnBiomeTints { grass: [90, 80, 70], foliage: [60, 50, 40] },
        );

        assert!(!Arc::ptr_eq(&chunk.voxels, &clone.voxels));
        assert!(!Arc::ptr_eq(&chunk.column_biome_tints, &clone.column_biome_tints));
        assert_eq!(chunk.get(1, 2, 3), Voxel::from_block_id(BlockId(7)));
        assert_eq!(
            chunk.biome_tints(1, 3),
            ColumnBiomeTints { grass: [10, 20, 30], foliage: [40, 50, 60] }
        );
    }
}
