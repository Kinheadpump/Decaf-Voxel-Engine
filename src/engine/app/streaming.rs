use ahash::AHashSet;

use crate::engine::{
    core::math::IVec3,
    player::controller::Player,
    render::{
        meshing::{MeshingFocus, sort_chunk_coords_by_priority},
        renderer::Renderer,
    },
    world::{chunk::Chunk, coord::ChunkCoord, generator::FlatGenerator, storage::World},
};

pub fn meshing_focus_from_player(player: &Player) -> MeshingFocus {
    MeshingFocus::new(
        ChunkCoord::from_world_voxel(player.position.floor().as_ivec3()),
        player.forward_3d(),
    )
}

pub fn stream_chunks_around_focus(
    world: &mut World,
    mut renderer: Option<&mut Renderer>,
    generator: &FlatGenerator,
    focus: MeshingFocus,
    render_radius_xz: i32,
    render_radius_y: i32,
    generation_budget: usize,
) {
    let mut desired_coords =
        chunk_coords_in_render_radius(focus, render_radius_xz, render_radius_y);
    let desired_set: AHashSet<_> = desired_coords.iter().copied().collect();

    let coords_to_unload: Vec<_> =
        world.chunks.keys().copied().filter(|coord| !desired_set.contains(coord)).collect();
    let unloaded_chunk_count = coords_to_unload.len();

    for coord in coords_to_unload {
        world.remove_chunk(coord);
        if let Some(renderer) = renderer.as_deref_mut() {
            renderer.remove_chunk_mesh(coord);
        }
    }

    let mut generated_chunk_count = 0usize;
    let generation_budget = if generation_budget == 0 { usize::MAX } else { generation_budget };

    desired_coords.retain(|coord| !world.contains_chunk(*coord));
    for coord in desired_coords.into_iter().take(generation_budget) {
        let mut chunk = Chunk::new();
        crate::engine::world::generator::ChunkGenerator::generate(generator, coord, &mut chunk);
        world.insert_chunk(coord, chunk);
        generated_chunk_count += 1;
    }

    if generated_chunk_count > 0 || unloaded_chunk_count > 0 {
        crate::log_debug!(
            "Streaming world around {:?}: generated {}, unloaded {}, loaded {}",
            focus.center.0,
            generated_chunk_count,
            unloaded_chunk_count,
            world.chunks.len()
        );
    }
}

fn chunk_coords_in_render_radius(
    focus: MeshingFocus,
    render_radius_xz: i32,
    render_radius_y: i32,
) -> Vec<ChunkCoord> {
    let mut coords = Vec::new();

    for chunk_z in -render_radius_xz..=render_radius_xz {
        for chunk_y in -render_radius_y..=render_radius_y {
            for chunk_x in -render_radius_xz..=render_radius_xz {
                coords.push(ChunkCoord(focus.center.0 + IVec3::new(chunk_x, chunk_y, chunk_z)));
            }
        }
    }

    sort_chunk_coords_by_priority(&mut coords, focus);
    coords
}
