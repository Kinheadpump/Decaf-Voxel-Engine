use crate::engine::{
    core::{
        math::{IVec3, UVec3},
        types::{CHUNK_SIZE, FaceDir},
    },
    render::gpu_types::{ChunkMeshCpu, PackedFace, RenderBucket},
    world::{
        accessor::WorldVoxelReader,
        block::{
            id::BlockId,
            resolved::{ResolvedBlock, ResolvedBlockRegistry},
        },
        chunk::Chunk,
        coord::ChunkCoord,
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChunkMeshDirtyRegion {
    full: bool,
    slice_masks: [u32; 6],
}

impl ChunkMeshDirtyRegion {
    pub fn full() -> Self {
        Self { full: true, slice_masks: [0; 6] }
    }

    pub fn from_local_voxel(local: UVec3) -> Self {
        let mut region = Self::default();
        region.mark_local_voxel(local);
        region
    }

    pub fn is_full(self) -> bool {
        self.full
    }

    pub fn is_empty(self) -> bool {
        !self.full && self.slice_masks.iter().all(|mask| *mask == 0)
    }

    pub fn merge(&mut self, other: Self) {
        if self.full {
            return;
        }

        if other.full {
            *self = Self::full();
            return;
        }

        for (mask, other_mask) in self.slice_masks.iter_mut().zip(other.slice_masks) {
            *mask |= other_mask;
        }
    }

    pub fn touches(self, dir: FaceDir, depth: usize) -> bool {
        self.full || (self.slice_masks[dir as usize] & (1u32 << depth)) != 0
    }

    pub fn mark_local_voxel(&mut self, local: UVec3) {
        if self.full {
            return;
        }

        self.mark_axis_slices(local.x as usize, FaceDir::NegX, FaceDir::PosX);
        self.mark_axis_slices(local.y as usize, FaceDir::NegY, FaceDir::PosY);
        self.mark_axis_slices(local.z as usize, FaceDir::NegZ, FaceDir::PosZ);
    }

    fn mark_axis_slices(&mut self, depth: usize, neg_dir: FaceDir, pos_dir: FaceDir) {
        self.mark_slice(neg_dir, depth);
        self.mark_slice(pos_dir, depth);

        if depth > 0 {
            self.mark_slice(pos_dir, depth - 1);
        }

        if depth + 1 < CHUNK_SIZE {
            self.mark_slice(neg_dir, depth + 1);
        }
    }

    fn mark_slice(&mut self, dir: FaceDir, depth: usize) {
        debug_assert!(depth < CHUNK_SIZE);
        self.slice_masks[dir as usize] |= 1u32 << depth;
    }
}

#[derive(Clone)]
pub struct ChunkMeshSlices {
    pub faces: [[[Vec<PackedFace>; CHUNK_SIZE]; 6]; 2],
    pub source_generation: u32,
}

impl ChunkMeshSlices {
    pub fn new() -> Self {
        Self {
            faces: std::array::from_fn(|_| {
                std::array::from_fn(|_| std::array::from_fn(|_| Vec::new()))
            }),
            source_generation: 0,
        }
    }

    pub fn flatten(&self) -> ChunkMeshCpu {
        let mut out = ChunkMeshCpu::new();

        for bucket in RenderBucket::ALL {
            for dir in 0..6usize {
                let dst = &mut out.faces[bucket as usize][dir];
                for depth in 0..CHUNK_SIZE {
                    dst.extend_from_slice(&self.faces[bucket as usize][dir][depth]);
                }
            }
        }

        out.source_generation = self.source_generation;
        out
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MaskCell {
    visible: bool,
    block_id: BlockId,
    texture_id: u16,
    bucket: RenderBucket,
}

pub fn build_chunk_mesh_slices(
    coord: ChunkCoord,
    chunk: &Chunk,
    accessor: &(impl WorldVoxelReader + ?Sized),
    resolved_blocks: &ResolvedBlockRegistry,
) -> ChunkMeshSlices {
    let mut out = ChunkMeshSlices::new();
    rebuild_chunk_mesh_slices(
        ChunkMeshDirtyRegion::full(),
        coord,
        chunk,
        accessor,
        resolved_blocks,
        &mut out,
    );
    out
}

pub fn rebuild_chunk_mesh_slices(
    dirty_region: ChunkMeshDirtyRegion,
    coord: ChunkCoord,
    chunk: &Chunk,
    accessor: &(impl WorldVoxelReader + ?Sized),
    resolved_blocks: &ResolvedBlockRegistry,
    out: &mut ChunkMeshSlices,
) {
    let chunk_origin = coord.world_origin();

    for dir in FaceDir::ALL {
        for depth in 0..CHUNK_SIZE {
            if !dirty_region.touches(dir, depth) {
                continue;
            }

            let slice_faces =
                build_direction_slice(chunk_origin, chunk, accessor, resolved_blocks, dir, depth);

            for bucket in RenderBucket::ALL {
                out.faces[bucket as usize][dir as usize][depth] =
                    slice_faces[bucket as usize].clone();
            }
        }
    }

    out.source_generation = chunk.generation;
}

fn build_direction_slice(
    chunk_origin: IVec3,
    chunk: &Chunk,
    accessor: &(impl WorldVoxelReader + ?Sized),
    resolved_blocks: &ResolvedBlockRegistry,
    dir: FaceDir,
    depth: usize,
) -> [Vec<PackedFace>; 2] {
    let mut out = std::array::from_fn(|_| Vec::new());
    let mut mask = [[MaskCell::default(); CHUNK_SIZE]; CHUNK_SIZE];
    let mut used = [[false; CHUNK_SIZE]; CHUNK_SIZE];

    for v_index in 0..CHUNK_SIZE {
        for u_index in 0..CHUNK_SIZE {
            used[u_index][v_index] = false;

            let local = face_local_xyz(dir, depth, u_index, v_index);
            let voxel = chunk.get(local.x as usize, local.y as usize, local.z as usize);
            let block_id = voxel.block_id();
            let block = resolved_blocks.get_voxel(voxel);

            if block.is_air() {
                mask[u_index][v_index] = MaskCell::default();
                continue;
            }

            let world_voxel = chunk_origin + local;
            let neighbor_world = world_voxel + dir.normal();
            let neighbor_voxel = accessor.get_world_voxel(neighbor_world);
            let neighbor_id = neighbor_voxel.block_id();
            let neighbor = resolved_blocks.get_voxel(neighbor_voxel);

            let visible = face_visible_between(block_id, block, neighbor_id, neighbor);

            mask[u_index][v_index] = if visible {
                MaskCell {
                    visible: true,
                    block_id,
                    texture_id: block.textures.get(dir),
                    bucket: if block.is_transparent() {
                        RenderBucket::Transparent
                    } else {
                        RenderBucket::Opaque
                    },
                }
            } else {
                MaskCell::default()
            };
        }
    }

    // Greedy meshing on the mask
    for v_index in 0..CHUNK_SIZE {
        for u_index in 0..CHUNK_SIZE {
            if used[u_index][v_index] {
                continue;
            }

            let cell = mask[u_index][v_index];
            if !cell.visible {
                continue;
            }

            let block_id = cell.block_id;
            let texture_id = cell.texture_id;
            let bucket = cell.bucket;

            let mut width = 1usize;
            while u_index + width < CHUNK_SIZE {
                let c = mask[u_index + width][v_index];
                if used[u_index + width][v_index]
                    || !c.visible
                    || c.block_id != block_id
                    || c.texture_id != texture_id
                    || c.bucket != bucket
                {
                    break;
                }
                width += 1;
            }

            let mut height = 1usize;
            'outer: while v_index + height < CHUNK_SIZE {
                for x in 0..width {
                    let c = mask[u_index + x][v_index + height];
                    if used[u_index + x][v_index + height]
                        || !c.visible
                        || c.block_id != block_id
                        || c.texture_id != texture_id
                        || c.bucket != bucket
                    {
                        break 'outer;
                    }
                }
                height += 1;
            }

            for used_column in used.iter_mut().skip(u_index).take(width) {
                for used_cell in used_column.iter_mut().skip(v_index).take(height) {
                    *used_cell = true;
                }
            }

            let anchor = face_local_xyz(dir, depth, u_index, v_index);

            let packed = PackedFace::pack(
                anchor.x as u32,
                anchor.y as u32,
                anchor.z as u32,
                texture_id as u32,
                (width as u32) - 1,
                (height as u32) - 1,
            );

            out[bucket as usize].push(packed);
        }
    }

    out
}

fn face_visible_between(
    current_id: BlockId,
    current: ResolvedBlock,
    neighbor_id: BlockId,
    neighbor: ResolvedBlock,
) -> bool {
    if current.is_air() {
        return false;
    }

    if current.is_no_cull() {
        return true;
    }

    if neighbor.is_air() {
        return true;
    }

    if neighbor.is_opaque() && !neighbor.is_no_cull() {
        return false;
    }

    if current_id == neighbor_id && current.is_transparent() && neighbor.is_transparent() {
        return false;
    }

    true
}

fn face_local_xyz(dir: FaceDir, depth: usize, u: usize, v: usize) -> IVec3 {
    match dir {
        FaceDir::PosX => IVec3::new(depth as i32, u as i32, v as i32),
        FaceDir::NegX => IVec3::new(depth as i32, v as i32, u as i32),
        FaceDir::PosY => IVec3::new(v as i32, depth as i32, u as i32),
        FaceDir::NegY => IVec3::new(u as i32, depth as i32, v as i32),
        FaceDir::PosZ => IVec3::new(u as i32, v as i32, depth as i32),
        FaceDir::NegZ => IVec3::new(v as i32, u as i32, depth as i32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::world::block::{flags::BlockFlags, resolved::ResolvedFaceTextures};

    fn resolved(id: u16, flags: BlockFlags) -> ResolvedBlock {
        ResolvedBlock {
            id: BlockId(id),
            flags,
            textures: ResolvedFaceTextures {
                pos_x: 0,
                neg_x: 0,
                pos_y: 0,
                neg_y: 0,
                pos_z: 0,
                neg_z: 0,
            },
        }
    }

    #[test]
    fn opaque_neighbor_hides_face() {
        let stone = resolved(1, BlockFlags::SOLID | BlockFlags::OPAQUE);
        let dirt = resolved(2, BlockFlags::SOLID | BlockFlags::OPAQUE);

        assert!(!face_visible_between(stone.id, stone, dirt.id, dirt));
    }

    #[test]
    fn same_transparent_block_hides_internal_face() {
        let glass = resolved(3, BlockFlags::SOLID | BlockFlags::TRANSPARENT);

        assert!(!face_visible_between(glass.id, glass, glass.id, glass));
    }

    #[test]
    fn no_cull_block_keeps_face_visible() {
        let leaves = resolved(4, BlockFlags::SOLID | BlockFlags::TRANSPARENT | BlockFlags::NO_CULL);
        let leaves_neighbor =
            resolved(4, BlockFlags::SOLID | BlockFlags::TRANSPARENT | BlockFlags::NO_CULL);

        assert!(face_visible_between(leaves.id, leaves, leaves_neighbor.id, leaves_neighbor));
    }

    #[test]
    fn dirty_region_marks_adjacent_axis_slices() {
        let region = ChunkMeshDirtyRegion::from_local_voxel(UVec3::new(5, 7, 9));

        assert!(region.touches(FaceDir::NegX, 5));
        assert!(region.touches(FaceDir::PosX, 5));
        assert!(region.touches(FaceDir::PosX, 4));
        assert!(region.touches(FaceDir::NegX, 6));
        assert!(region.touches(FaceDir::NegY, 7));
        assert!(region.touches(FaceDir::PosY, 6));
        assert!(region.touches(FaceDir::NegZ, 10));
        assert!(!region.touches(FaceDir::PosX, 7));
    }

    #[test]
    fn dirty_region_merge_preserves_slice_bits() {
        let mut region = ChunkMeshDirtyRegion::from_local_voxel(UVec3::new(0, 0, 0));
        region.merge(ChunkMeshDirtyRegion::from_local_voxel(UVec3::new(31, 31, 31)));

        assert!(region.touches(FaceDir::NegX, 0));
        assert!(region.touches(FaceDir::PosX, 0));
        assert!(region.touches(FaceDir::NegX, 31));
        assert!(region.touches(FaceDir::PosX, 30));
        assert!(!region.is_full());
    }
}
