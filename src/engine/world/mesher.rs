use crate::engine::{
    core::{
        math::IVec3,
        types::{CHUNK_SIZE, FaceDir},
    },
    render::gpu_types::{ChunkMeshCpu, PackedFace},
    world::{accessor::VoxelAccessor, chunk::Chunk, coord::ChunkCoord},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MaskCell {
    visible: bool,
    block_id: u32,
}

pub fn build_chunk_mesh(
    coord: ChunkCoord,
    chunk: &Chunk,
    accessor: &VoxelAccessor,
) -> ChunkMeshCpu {
    let mut out = ChunkMeshCpu::new();
    let chunk_origin = coord.world_origin();

    for dir in FaceDir::ALL {
        build_direction_mesh(chunk_origin, chunk, accessor, dir, &mut out.faces[dir as usize]);
    }
    out.source_generation = chunk.generation;
    out
}

fn build_direction_mesh(
    chunk_origin: IVec3,
    chunk: &Chunk,
    accessor: &VoxelAccessor,
    dir: FaceDir,
    out: &mut Vec<PackedFace>,
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

                if !voxel.is_solid() {
                    mask[u][v] = MaskCell::default();
                    continue;
                }

                let world_voxel = chunk_origin + local;
                let neighbor_world = world_voxel + dir.normal();
                let neighbor = accessor.get_world_voxel(neighbor_world);

                let visible = neighbor.is_air();

                mask[u][v] = if visible {
                    MaskCell { visible: true, block_id: voxel.block_id() }
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

                let mut width = 1usize;
                while u + width < CHUNK_SIZE {
                    let c = mask[u + width][v];
                    if used[u + width][v] || c.block_id != block_id || !c.visible {
                        break;
                    }
                    width += 1;
                }

                let mut height = 1usize;
                'outer: while v + height < CHUNK_SIZE {
                    for x in 0..width {
                        let c = mask[u + x][v + height];
                        if used[u + x][v + height] || c.block_id != block_id || !c.visible {
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
                    block_id as u32,
                    (width as u32) - 1,
                    (height as u32) - 1,
                );

                out.push(packed);
            }
        }
    }
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
