use crate::engine::{
    core::{
        math::IVec3,
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
struct MaskCell {
    visible: bool,
    block_id: BlockId,
    texture_id: u16,
    bucket: RenderBucket,
}

pub fn build_chunk_mesh(
    coord: ChunkCoord,
    chunk: &Chunk,
    accessor: &(impl WorldVoxelReader + ?Sized),
    resolved_blocks: &ResolvedBlockRegistry,
) -> ChunkMeshCpu {
    let mut out = ChunkMeshCpu::new();
    let chunk_origin = coord.world_origin();

    for dir in FaceDir::ALL {
        build_direction_mesh(chunk_origin, chunk, accessor, resolved_blocks, dir, &mut out);
    }
    out.source_generation = chunk.generation;
    out
}

fn build_direction_mesh(
    chunk_origin: IVec3,
    chunk: &Chunk,
    accessor: &(impl WorldVoxelReader + ?Sized),
    resolved_blocks: &ResolvedBlockRegistry,
    dir: FaceDir,
    out: &mut ChunkMeshCpu,
) {
    let mut mask = [[MaskCell::default(); CHUNK_SIZE]; CHUNK_SIZE];
    let mut used = [[false; CHUNK_SIZE]; CHUNK_SIZE];

    for depth in 0..CHUNK_SIZE {
        // Build the mask for this slice
        for v in 0..CHUNK_SIZE {
            for u in 0..CHUNK_SIZE {
                used[u][v] = false;

                let local = face_local_xyz(dir, depth, u, v);
                let voxel = chunk.get(local.x as usize, local.y as usize, local.z as usize);
                let block_id = voxel.block_id();
                let block = resolved_blocks.get_voxel(voxel);

                if block.is_air() {
                    mask[u][v] = MaskCell::default();
                    continue;
                }

                let world_voxel = chunk_origin + local;
                let neighbor_world = world_voxel + dir.normal();
                let neighbor_voxel = accessor.get_world_voxel(neighbor_world);
                let neighbor_id = neighbor_voxel.block_id();
                let neighbor = resolved_blocks.get_voxel(neighbor_voxel);

                let visible = face_visible_between(block_id, block, neighbor_id, neighbor);

                mask[u][v] = if visible {
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
        for v in 0..CHUNK_SIZE {
            for u in 0..CHUNK_SIZE {
                if used[u][v] {
                    continue;
                }

                let cell = mask[u][v];
                if !cell.visible {
                    continue;
                }

                let block_id = cell.block_id;
                let texture_id = cell.texture_id;
                let bucket = cell.bucket;

                let mut width = 1usize;
                while u + width < CHUNK_SIZE {
                    let c = mask[u + width][v];
                    if used[u + width][v]
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
                'outer: while v + height < CHUNK_SIZE {
                    for x in 0..width {
                        let c = mask[u + x][v + height];
                        if used[u + x][v + height]
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

                for vv in v..v + height {
                    for uu in u..u + width {
                        used[uu][vv] = true;
                    }
                }

                let anchor = face_local_xyz(dir, depth, u, v);

                let packed = PackedFace::pack(
                    anchor.x as u32,
                    anchor.y as u32,
                    anchor.z as u32,
                    texture_id as u32,
                    (width as u32) - 1,
                    (height as u32) - 1,
                );

                out.faces[bucket as usize][dir as usize].push(packed);
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
}
