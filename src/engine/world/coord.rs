//! Coordinate types for voxel-space world access.
//!
//! `WorldVoxelPos` is an absolute voxel position in world space.
//! `LocalVoxelPos` is guaranteed to stay inside a single chunk, with each
//! component in `0..CHUNK_SIZE`.
//! `ChunkCoord` is the chunk-space origin owner for a `WorldVoxelPos`.

use std::ops::{Add, Sub};

use crate::engine::core::{
    math::{IVec3, UVec3, Vec3},
    types::{CHUNK_SIZE_I32, CHUNK_SIZE_U32},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorldVoxelPos(pub IVec3);

impl WorldVoxelPos {
    pub const ZERO: Self = Self(IVec3::ZERO);

    #[inline]
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self(IVec3::new(x, y, z))
    }

    #[inline]
    pub fn as_ivec3(self) -> IVec3 {
        self.0
    }

    #[inline]
    pub fn as_vec3(self) -> Vec3 {
        self.0.as_vec3()
    }

    #[inline]
    pub fn x(self) -> i32 {
        self.0.x
    }

    #[inline]
    pub fn y(self) -> i32 {
        self.0.y
    }

    #[inline]
    pub fn z(self) -> i32 {
        self.0.z
    }
}

impl From<IVec3> for WorldVoxelPos {
    #[inline]
    fn from(value: IVec3) -> Self {
        Self(value)
    }
}

impl From<WorldVoxelPos> for IVec3 {
    #[inline]
    fn from(value: WorldVoxelPos) -> Self {
        value.0
    }
}

impl Add<IVec3> for WorldVoxelPos {
    type Output = Self;

    #[inline]
    fn add(self, rhs: IVec3) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl Sub<IVec3> for WorldVoxelPos {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: IVec3) -> Self::Output {
        Self(self.0 - rhs)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LocalVoxelPos(pub UVec3);

impl LocalVoxelPos {
    #[inline]
    pub fn new(x: u32, y: u32, z: u32) -> Self {
        debug_assert!(x < CHUNK_SIZE_U32);
        debug_assert!(y < CHUNK_SIZE_U32);
        debug_assert!(z < CHUNK_SIZE_U32);
        Self(UVec3::new(x, y, z))
    }

    #[inline]
    pub fn as_uvec3(self) -> UVec3 {
        self.0
    }

    #[inline]
    pub fn as_ivec3(self) -> IVec3 {
        self.0.as_ivec3()
    }

    #[inline]
    pub fn x(self) -> usize {
        self.0.x as usize
    }

    #[inline]
    pub fn y(self) -> usize {
        self.0.y as usize
    }

    #[inline]
    pub fn z(self) -> usize {
        self.0.z as usize
    }
}

impl From<UVec3> for LocalVoxelPos {
    #[inline]
    fn from(value: UVec3) -> Self {
        Self::new(value.x, value.y, value.z)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChunkCoord(pub IVec3);

impl ChunkCoord {
    #[inline]
    pub fn from_world_voxel(p: impl Into<WorldVoxelPos>) -> Self {
        let p = p.into().0;
        Self(IVec3::new(
            p.x.div_euclid(CHUNK_SIZE_I32),
            p.y.div_euclid(CHUNK_SIZE_I32),
            p.z.div_euclid(CHUNK_SIZE_I32),
        ))
    }

    #[inline]
    pub fn world_origin(self) -> WorldVoxelPos {
        WorldVoxelPos(self.0 * CHUNK_SIZE_I32)
    }

    #[inline]
    pub fn local_voxel(self, p: impl Into<WorldVoxelPos>) -> LocalVoxelPos {
        let p = p.into().0;
        debug_assert_eq!(Self::from_world_voxel(p), self);

        LocalVoxelPos::new(
            p.x.rem_euclid(CHUNK_SIZE_I32) as u32,
            p.y.rem_euclid(CHUNK_SIZE_I32) as u32,
            p.z.rem_euclid(CHUNK_SIZE_I32) as u32,
        )
    }

    #[inline]
    pub fn world_voxel(self, local: impl Into<LocalVoxelPos>) -> WorldVoxelPos {
        self.world_origin() + local.into().as_ivec3()
    }

    #[inline]
    pub fn offset(self, delta: IVec3) -> Self {
        Self(self.0 + delta)
    }
}
