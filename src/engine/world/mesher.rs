use std::{cell::RefCell, thread_local, time::Instant};

use crate::engine::{
    core::types::{CHUNK_SIZE, FaceDir},
    render::gpu_types::{ChunkMeshCpu, PackedFace, RenderBucket},
    world::{
        accessor::ChunkNeighborReader,
        block::{
            id::BlockId,
            resolved::{ResolvedBlock, ResolvedBlockRegistry},
            tint::BiomeTint,
        },
        chunk::Chunk,
        coord::{ChunkCoord, LocalVoxelPos},
        voxel::Voxel,
    },
};

use crate::engine::render::gpu_types::{
    DEFAULT_FACE_TINT, FACE_TINT_MODE_GRASS, FACE_TINT_MODE_MULTIPLY, pack_face_tint,
};

const SLICE_AREA: usize = CHUNK_SIZE * CHUNK_SIZE;

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

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn touches(self, dir: FaceDir, depth: usize) -> bool {
        self.full || (self.slice_masks[dir as usize] & (1u32 << depth)) != 0
    }

    fn collect_dirty_slices(self, out: &mut Vec<DirtySlice>) {
        out.clear();

        if self.full {
            for dir in FaceDir::ALL {
                for depth in 0..CHUNK_SIZE {
                    out.push(DirtySlice { dir, depth });
                }
            }
            return;
        }

        for dir in FaceDir::ALL {
            let mut slices = self.slice_masks[dir as usize];
            while slices != 0 {
                let depth = slices.trailing_zeros() as usize;
                out.push(DirtySlice { dir, depth });
                slices &= slices - 1;
            }
        }
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
    pub dirty_slice_count: u32,
    pub build_cpu_time_ns: u64,
    pub snapshot_capture_cpu_time_ns: u64,
    pub slice_construction_cpu_time_ns: u64,
    pub greedy_merge_cpu_time_ns: u64,
    pub flatten_cpu_time_ns: u64,
}

impl MeshingBuildProfile {
    pub(crate) fn recompute_total(&mut self) {
        self.build_cpu_time_ns = self
            .snapshot_capture_cpu_time_ns
            .saturating_add(self.slice_construction_cpu_time_ns)
            .saturating_add(self.greedy_merge_cpu_time_ns)
            .saturating_add(self.flatten_cpu_time_ns);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MaskCell {
    visible: bool,
    block_id: BlockId,
    texture_id: u16,
    bucket: RenderBucket,
    tint: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DirtySlice {
    dir: FaceDir,
    depth: usize,
}

struct MeshingScratch {
    mask: Box<[MaskCell; SLICE_AREA]>,
    used: Box<[bool; SLICE_AREA]>,
    neighbor_voxels: Box<[Voxel; SLICE_AREA]>,
    dirty_slices: Vec<DirtySlice>,
}

impl Default for MeshingScratch {
    fn default() -> Self {
        Self {
            mask: Box::new([MaskCell::default(); SLICE_AREA]),
            used: Box::new([false; SLICE_AREA]),
            neighbor_voxels: Box::new([Voxel::AIR; SLICE_AREA]),
            dirty_slices: Vec::with_capacity(FaceDir::ALL.len() * CHUNK_SIZE),
        }
    }
}

thread_local! {
    static MESHING_SCRATCH: RefCell<MeshingScratch> =
        RefCell::new(MeshingScratch::default());
}

pub fn build_chunk_mesh_slices(
    coord: ChunkCoord,
    chunk: &Chunk,
    neighbors: &(impl ChunkNeighborReader + ?Sized),
    resolved_blocks: &ResolvedBlockRegistry,
) -> (ChunkMeshSlices, MeshingBuildProfile) {
    let mut out = ChunkMeshSlices::new();
    let profile = rebuild_chunk_mesh_slices(
        ChunkMeshDirtyRegion::full(),
        coord,
        chunk,
        neighbors,
        resolved_blocks,
        &mut out,
    );
    (out, profile)
}

pub fn rebuild_chunk_mesh_slices(
    dirty_region: ChunkMeshDirtyRegion,
    coord: ChunkCoord,
    chunk: &Chunk,
    neighbors: &(impl ChunkNeighborReader + ?Sized),
    resolved_blocks: &ResolvedBlockRegistry,
    out: &mut ChunkMeshSlices,
) -> MeshingBuildProfile {
    let profile = MESHING_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        dirty_region.collect_dirty_slices(&mut scratch.dirty_slices);

        let mut profile = MeshingBuildProfile {
            dirty_slice_count: scratch.dirty_slices.len() as u32,
            ..Default::default()
        };

        for dirty_slice_index in 0..scratch.dirty_slices.len() {
            let dirty_slice = scratch.dirty_slices[dirty_slice_index];
            let (opaque_buckets, transparent_buckets) = out.faces.split_at_mut(1);
            let slice_faces = [
                &mut opaque_buckets[RenderBucket::Opaque as usize][dirty_slice.dir as usize]
                    [dirty_slice.depth],
                &mut transparent_buckets[0][dirty_slice.dir as usize][dirty_slice.depth],
            ];
            build_direction_slice_into(
                coord,
                chunk,
                neighbors,
                resolved_blocks,
                dirty_slice,
                slice_faces,
                &mut scratch,
                &mut profile,
            );
        }

        profile
    });

    out.source_generation = chunk.generation;
    let mut profile = profile;
    profile.recompute_total();
    profile
}

fn build_direction_slice_into(
    coord: ChunkCoord,
    chunk: &Chunk,
    neighbors: &(impl ChunkNeighborReader + ?Sized),
    resolved_blocks: &ResolvedBlockRegistry,
    dirty_slice: DirtySlice,
    mut out: [&mut Vec<PackedFace>; 2],
    scratch: &mut MeshingScratch,
    profile: &mut MeshingBuildProfile,
) {
    let dir = dirty_slice.dir;
    let depth = dirty_slice.depth;
    let axis_map = SliceAxisMap::for_face(dir);
    let mask = &mut scratch.mask[..];
    let used = &mut scratch.used[..];
    let neighbor_voxels = &mut scratch.neighbor_voxels[..];
    let slice_construction_started_at = Instant::now();

    prefetch_neighbor_slice(chunk, coord, neighbors, axis_map, dirty_slice, neighbor_voxels);
    used.fill(false);

    for faces in &mut out {
        faces.clear();
    }

    for v_index in 0..CHUNK_SIZE {
        for u_index in 0..CHUNK_SIZE {
            let slice_index = slice_index(u_index, v_index);
            let local = axis_map.local(depth, u_index, v_index);
            let voxel = chunk.get_local(local);
            let block_id = voxel.block_id();
            let block = resolved_blocks.get_voxel(voxel);

            if block.is_air() {
                mask[slice_index] = MaskCell::default();
                continue;
            }

            let neighbor_voxel = neighbor_voxels[slice_index];
            let neighbor_id = neighbor_voxel.block_id();
            let neighbor = resolved_blocks.get_voxel(neighbor_voxel);

            let visible = face_visible_between(block_id, block, neighbor_id, neighbor);

            mask[slice_index] = if visible {
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
    profile.slice_construction_cpu_time_ns = profile
        .slice_construction_cpu_time_ns
        .saturating_add(duration_to_nanos(slice_construction_started_at.elapsed()));

    let greedy_merge_started_at = Instant::now();

    // Greedy meshing on the mask
    for v_index in 0..CHUNK_SIZE {
        for u_index in 0..CHUNK_SIZE {
            let cell_index = slice_index(u_index, v_index);
            if used[cell_index] {
                continue;
            }

            let cell = mask[cell_index];
            if !cell.visible {
                continue;
            }

            let block_id = cell.block_id;
            let texture_id = cell.texture_id;
            let bucket = cell.bucket;

            let mut width = 1usize;
            while u_index + width < CHUNK_SIZE {
                let index = slice_index(u_index + width, v_index);
                let c = mask[index];
                if used[index]
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
                    let index = slice_index(u_index + x, v_index + height);
                    let c = mask[index];
                    if used[index]
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

            for dy in 0..height {
                let row_start = slice_index(u_index, v_index + dy);
                for used_cell in used[row_start..row_start + width].iter_mut() {
                    *used_cell = true;
                }
            }

            let anchor = axis_map.local(depth, u_index, v_index);

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
    profile.greedy_merge_cpu_time_ns = profile
        .greedy_merge_cpu_time_ns
        .saturating_add(duration_to_nanos(greedy_merge_started_at.elapsed()));
}

fn prefetch_neighbor_slice(
    center_chunk: &Chunk,
    coord: ChunkCoord,
    neighbors: &(impl ChunkNeighborReader + ?Sized),
    axis_map: SliceAxisMap,
    dirty_slice: DirtySlice,
    out: &mut [Voxel],
) {
    debug_assert_eq!(out.len(), SLICE_AREA);

    let boundary_face = is_boundary_slice(dirty_slice.dir, dirty_slice.depth);
    if boundary_face {
        let Some(neighbor_chunk) = neighbors.get_chunk_neighbor(coord, dirty_slice.dir) else {
            out.fill(Voxel::AIR);
            return;
        };
        let neighbor_depth = boundary_neighbor_depth(dirty_slice.dir);
        fill_slice_voxels(neighbor_chunk, axis_map, neighbor_depth, out);
    } else {
        let neighbor_depth = adjacent_depth(dirty_slice.dir, dirty_slice.depth);
        fill_slice_voxels(center_chunk, axis_map, neighbor_depth, out);
    }
}

fn fill_slice_voxels(chunk: &Chunk, axis_map: SliceAxisMap, depth: usize, out: &mut [Voxel]) {
    for v_index in 0..CHUNK_SIZE {
        let row_start = v_index * CHUNK_SIZE;
        for u_index in 0..CHUNK_SIZE {
            let local = axis_map.local(depth, u_index, v_index);
            out[row_start + u_index] = chunk.get_local(local);
        }
    }
}

#[inline]
fn slice_index(u: usize, v: usize) -> usize {
    debug_assert!(u < CHUNK_SIZE);
    debug_assert!(v < CHUNK_SIZE);
    u + v * CHUNK_SIZE
}

#[inline]
fn is_boundary_slice(dir: FaceDir, depth: usize) -> bool {
    match dir {
        FaceDir::PosX | FaceDir::PosY | FaceDir::PosZ => depth + 1 == CHUNK_SIZE,
        FaceDir::NegX | FaceDir::NegY | FaceDir::NegZ => depth == 0,
    }
}

#[inline]
fn adjacent_depth(dir: FaceDir, depth: usize) -> usize {
    match dir {
        FaceDir::PosX | FaceDir::PosY | FaceDir::PosZ => depth + 1,
        FaceDir::NegX | FaceDir::NegY | FaceDir::NegZ => depth - 1,
    }
}

#[inline]
fn boundary_neighbor_depth(dir: FaceDir) -> usize {
    match dir {
        FaceDir::PosX | FaceDir::PosY | FaceDir::PosZ => 0,
        FaceDir::NegX | FaceDir::NegY | FaceDir::NegZ => CHUNK_SIZE - 1,
    }
}

#[derive(Clone, Copy)]
struct SliceAxisMap {
    x_from: usize,
    y_from: usize,
    z_from: usize,
}

impl SliceAxisMap {
    #[inline]
    fn for_face(dir: FaceDir) -> Self {
        match dir {
            FaceDir::PosX => Self { x_from: 0, y_from: 1, z_from: 2 },
            FaceDir::NegX => Self { x_from: 0, y_from: 2, z_from: 1 },
            FaceDir::PosY => Self { x_from: 2, y_from: 0, z_from: 1 },
            FaceDir::NegY => Self { x_from: 1, y_from: 0, z_from: 2 },
            FaceDir::PosZ => Self { x_from: 1, y_from: 2, z_from: 0 },
            FaceDir::NegZ => Self { x_from: 2, y_from: 1, z_from: 0 },
        }
    }

    #[inline]
    fn local(self, depth: usize, u: usize, v: usize) -> LocalVoxelPos {
        let components = [depth as u32, u as u32, v as u32];
        LocalVoxelPos::new(
            components[self.x_from],
            components[self.y_from],
            components[self.z_from],
        )
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

#[inline]
fn duration_to_nanos(duration: std::time::Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

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
    fn full_dirty_region_collects_every_slice() {
        let mut slices = Vec::new();
        ChunkMeshDirtyRegion::full().collect_dirty_slices(&mut slices);

        assert_eq!(slices.len(), FaceDir::ALL.len() * CHUNK_SIZE);
        assert_eq!(slices[0], DirtySlice { dir: FaceDir::PosX, depth: 0 });
        assert_eq!(
            slices.last().copied(),
            Some(DirtySlice { dir: FaceDir::NegZ, depth: CHUNK_SIZE - 1 })
        );
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

    #[test]
    fn chunk_clone_uses_shared_arc_storage() {
        let mut chunk = Chunk::new();
        chunk.set(0, 0, 0, crate::engine::world::voxel::Voxel::from_block_id(BlockId(3)));

        let clone = chunk.clone();

        assert!(Arc::ptr_eq(&chunk.voxels, &clone.voxels));
        assert!(Arc::ptr_eq(&chunk.column_biome_tints, &clone.column_biome_tints));
    }

    fn test_resolved_registry() -> ResolvedBlockRegistry {
        let registry = create_default_block_registry();
        let textures = create_texture_registry(&registry);
        ResolvedBlockRegistry::build(&registry, textures.layer_map())
    }
}
