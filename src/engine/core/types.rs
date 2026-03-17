use crate::engine::core::math::IVec3;

pub const CHUNK_SIZE: usize = 32;
pub const CHUNK_SIZE_U32: u32 = 32;
pub const CHUNK_SIZE_I32: i32 = 32;
pub const CHUNK_VOLUME: usize = CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE;

pub const MAX_BLOCK_IDS: u32 = 128;
pub const MAX_VISIBLE_DRAWS: usize = 32_768;
pub const INITIAL_FACE_CAPACITY: usize = 4_000_000;

pub const WINDOW_WIDTH: u32 = 1280;
pub const WINDOW_HEIGHT: u32 = 720;

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceDir {
    PosX = 0,
    NegX = 1,
    PosY = 2,
    NegY = 3,
    PosZ = 4,
    NegZ = 5,
}

impl FaceDir {
    pub const ALL: [FaceDir; 6] =
        [FaceDir::PosX, FaceDir::NegX, FaceDir::PosY, FaceDir::NegY, FaceDir::PosZ, FaceDir::NegZ];

    #[inline]
    pub fn normal(self) -> IVec3 {
        match self {
            FaceDir::PosX => IVec3::new(1, 0, 0),
            FaceDir::NegX => IVec3::new(-1, 0, 0),
            FaceDir::PosY => IVec3::new(0, 1, 0),
            FaceDir::NegY => IVec3::new(0, -1, 0),
            FaceDir::PosZ => IVec3::new(0, 0, 1),
            FaceDir::NegZ => IVec3::new(0, 0, -1),
        }
    }
}
