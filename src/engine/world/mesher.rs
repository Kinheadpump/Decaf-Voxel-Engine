use crate::engine::{
    core::types::{CHUNK_SIZE, FaceDir},
    render::gpu_types::{ChunkMeshCpu, PackedFace, RenderBucket},
    world::{
        accessor::WorldVoxelReader,
        block::{
            id::BlockId,
            resolved::{ResolvedBlock, ResolvedBlockRegistry},
            tint::BiomeTint,
        },
        chunk::Chunk,
        coord::{ChunkCoord, LocalVoxelPos, WorldVoxelPos},
    },
};

use crate::engine::render::gpu_types::{
    DEFAULT_FACE_TINT, FACE_TINT_MODE_GRASS, FACE_TINT_MODE_MULTIPLY, pack_face_tint,
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

    pub fn from_face_slice(dir: FaceDir, depth: usize) -> Self {
        let mut region = Self::default();
        region.mark_slice(dir, depth);
        region
    }

    pub fn from_local_voxel(local: impl Into<LocalVoxelPos>) -> Self {
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

    pub fn mark_local_voxel(&mut self, local: impl Into<LocalVoxelPos>) {
        if self.full {
            return;
        }

        let local = local.into().as_uvec3();
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
                let total_faces: usize =
                    self.faces[bucket as usize][dir].iter().map(Vec::len).sum();
                dst.reserve(total_faces);
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
pub struct MeshingBuildProfile {
    pub faces_emitted: u32,
    pub slice_buffer_growths: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MaskCell {
    visible: bool,
    block_id: BlockId,
    texture_id: u16,
    bucket: RenderBucket,
    tint: u32,
}

pub fn build_chunk_mesh_slices(
    coord: ChunkCoord,
    chunk: &Chunk,
    accessor: &(impl WorldVoxelReader + ?Sized),
    resolved_blocks: &ResolvedBlockRegistry,
) -> (ChunkMeshSlices, MeshingBuildProfile) {
    let mut out = ChunkMeshSlices::new();
    let profile = rebuild_chunk_mesh_slices(
        ChunkMeshDirtyRegion::full(),
        coord,
        chunk,
        accessor,
        resolved_blocks,
        &mut out,
    );
    (out, profile)
}

pub fn rebuild_chunk_mesh_slices(
    dirty_region: ChunkMeshDirtyRegion,
    coord: ChunkCoord,
    chunk: &Chunk,
    accessor: &(impl WorldVoxelReader + ?Sized),
    resolved_blocks: &ResolvedBlockRegistry,
    out: &mut ChunkMeshSlices,
) -> MeshingBuildProfile {
    let chunk_origin = coord.world_origin();
    let mut profile = MeshingBuildProfile::default();

    for dir in FaceDir::ALL {
        for depth in 0..CHUNK_SIZE {
            if !dirty_region.touches(dir, depth) {
                continue;
            }

            let (opaque_buckets, transparent_buckets) = out.faces.split_at_mut(1);
            let slice_faces = [
                &mut opaque_buckets[RenderBucket::Opaque as usize][dir as usize][depth],
                &mut transparent_buckets[0][dir as usize][depth],
            ];
            build_direction_slice_into(
                chunk_origin,
                chunk,
                accessor,
                resolved_blocks,
                dir,
                depth,
                slice_faces,
                &mut profile,
            );
        }
    }

    out.source_generation = chunk.generation;
    profile
}

fn build_direction_slice_into(
    chunk_origin: WorldVoxelPos,
    chunk: &Chunk,
    accessor: &(impl WorldVoxelReader + ?Sized),
    resolved_blocks: &ResolvedBlockRegistry,
    dir: FaceDir,
    depth: usize,
    mut out: [&mut Vec<PackedFace>; 2],
    profile: &mut MeshingBuildProfile,
) {
    let mut mask = [[MaskCell::default(); CHUNK_SIZE]; CHUNK_SIZE];
    let mut used = [[false; CHUNK_SIZE]; CHUNK_SIZE];

    for faces in &mut out {
        faces.clear();
    }

    for v_index in 0..CHUNK_SIZE {
        for u_index in 0..CHUNK_SIZE {
            used[u_index][v_index] = false;

            let local = face_local_xyz(dir, depth, u_index, v_index);
            let voxel = chunk.get_local(local);
            let block_id = voxel.block_id();
            let block = resolved_blocks.get_voxel(voxel);

            if block.is_air() {
                mask[u_index][v_index] = MaskCell::default();
                continue;
            }

            let world_voxel = chunk_origin + local.as_ivec3();
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
                    tint: resolve_face_tint(chunk, block, dir, local.x(), local.z()),
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
                    || c.tint != cell.tint
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
                        || c.tint != cell.tint
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
                anchor.as_uvec3().x,
                anchor.as_uvec3().y,
                anchor.as_uvec3().z,
                texture_id as u32,
                (width as u32) - 1,
                (height as u32) - 1,
                cell.tint,
            );

            let faces = &mut out[bucket as usize];
            let previous_capacity = faces.capacity();
            faces.push(packed);
            profile.faces_emitted += 1;
            if faces.capacity() > previous_capacity {
                profile.slice_buffer_growths += 1;
            }
        }
    }
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

fn resolve_face_tint(chunk: &Chunk, block: ResolvedBlock, dir: FaceDir, x: usize, z: usize) -> u32 {
    let tint = chunk.biome_tints(x, z);

    match block.tints.get(dir) {
        BiomeTint::None => DEFAULT_FACE_TINT,
        BiomeTint::Grass => pack_face_tint(FACE_TINT_MODE_GRASS, tint.grass),
        BiomeTint::Foliage => pack_face_tint(FACE_TINT_MODE_MULTIPLY, tint.foliage),
    }
}

fn face_local_xyz(dir: FaceDir, depth: usize, u: usize, v: usize) -> LocalVoxelPos {
    match dir {
        FaceDir::PosX => LocalVoxelPos::new(depth as u32, u as u32, v as u32),
        FaceDir::NegX => LocalVoxelPos::new(depth as u32, v as u32, u as u32),
        FaceDir::PosY => LocalVoxelPos::new(v as u32, depth as u32, u as u32),
        FaceDir::NegY => LocalVoxelPos::new(u as u32, depth as u32, v as u32),
        FaceDir::PosZ => LocalVoxelPos::new(u as u32, v as u32, depth as u32),
        FaceDir::NegZ => LocalVoxelPos::new(v as u32, u as u32, depth as u32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{
        render::materials::create_texture_registry,
        world::{
            accessor::VoxelAccessor,
            block::{
                create_default_block_registry,
                flags::BlockFlags,
                resolved::{ResolvedBlockRegistry, ResolvedFaceTextures},
                tint::{BiomeTint, ResolvedFaceTints},
            },
            storage::World,
        },
    };

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
            tints: ResolvedFaceTints::default(),
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
        let region = ChunkMeshDirtyRegion::from_local_voxel(LocalVoxelPos::new(5, 7, 9));

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
        let mut region = ChunkMeshDirtyRegion::from_local_voxel(LocalVoxelPos::new(0, 0, 0));
        region.merge(ChunkMeshDirtyRegion::from_local_voxel(LocalVoxelPos::new(31, 31, 31)));

        assert!(region.touches(FaceDir::NegX, 0));
        assert!(region.touches(FaceDir::PosX, 0));
        assert!(region.touches(FaceDir::NegX, 31));
        assert!(region.touches(FaceDir::PosX, 30));
        assert!(!region.is_full());
    }

    #[test]
    fn face_tint_uses_chunk_biome_colors() {
        let mut chunk = Chunk::new();
        chunk.set_biome_tints(
            3,
            5,
            crate::engine::world::chunk::ColumnBiomeTints {
                grass: [101, 152, 77],
                foliage: [72, 118, 54],
            },
        );

        let block = ResolvedBlock {
            id: BlockId(7),
            flags: BlockFlags::SOLID | BlockFlags::OPAQUE,
            textures: ResolvedFaceTextures {
                pos_x: 0,
                neg_x: 0,
                pos_y: 0,
                neg_y: 0,
                pos_z: 0,
                neg_z: 0,
            },
            tints: ResolvedFaceTints {
                pos_x: BiomeTint::Grass,
                neg_x: BiomeTint::Foliage,
                pos_y: BiomeTint::None,
                neg_y: BiomeTint::None,
                pos_z: BiomeTint::None,
                neg_z: BiomeTint::None,
            },
        };

        assert_eq!(
            resolve_face_tint(&chunk, block, FaceDir::PosX, 3, 5),
            pack_face_tint(FACE_TINT_MODE_GRASS, [101, 152, 77])
        );
        assert_eq!(
            resolve_face_tint(&chunk, block, FaceDir::NegX, 3, 5),
            pack_face_tint(FACE_TINT_MODE_MULTIPLY, [72, 118, 54])
        );
        assert_eq!(resolve_face_tint(&chunk, block, FaceDir::PosY, 3, 5), DEFAULT_FACE_TINT);
    }

    #[test]
    fn rebuilding_mesh_slices_reuses_existing_slice_buffers() {
        let resolved = test_resolved_registry();
        let coord = ChunkCoord(glam::IVec3::ZERO);
        let mut chunk = Chunk::new();
        let mut world = World::new();

        chunk.set(0, 0, 0, crate::engine::world::voxel::Voxel::from_block_id(BlockId(3)));
        world.insert_chunk(coord, chunk.clone());

        let accessor = VoxelAccessor { world: &world };
        let (mut mesh_slices, first_profile) =
            build_chunk_mesh_slices(coord, &chunk, &accessor, &resolved);
        let second_profile = rebuild_chunk_mesh_slices(
            ChunkMeshDirtyRegion::full(),
            coord,
            &chunk,
            &accessor,
            &resolved,
            &mut mesh_slices,
        );

        assert!(first_profile.faces_emitted > 0);
        assert!(first_profile.slice_buffer_growths > 0);
        assert_eq!(second_profile.faces_emitted, first_profile.faces_emitted);
        assert_eq!(second_profile.slice_buffer_growths, 0);
    }

    fn test_resolved_registry() -> ResolvedBlockRegistry {
        let registry = create_default_block_registry();
        let textures = create_texture_registry(&registry);
        ResolvedBlockRegistry::build(&registry, textures.layer_map())
    }
}
