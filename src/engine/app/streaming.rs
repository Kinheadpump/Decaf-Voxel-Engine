use std::sync::Arc;

use ahash::AHashSet;

use crate::engine::{
    core::math::IVec3,
    player::state::Player,
    render::{
        meshing::{MeshingFocus, sort_chunk_coords_by_priority},
        renderer::Renderer,
    },
    world::{
        coord::ChunkCoord,
        generation::{GenerationResult, ThreadedGenerator},
        generator::ChunkGenerator,
        storage::World,
    },
};

pub struct WorldStreamer {
    generator: ThreadedGenerator,
    desired_coords: AHashSet<ChunkCoord>,
    max_inflight_generations: usize,
}

impl WorldStreamer {
    pub fn new(
        generator: Arc<dyn ChunkGenerator>,
        generation_worker_count: usize,
        max_inflight_generations: usize,
    ) -> Self {
        Self {
            generator: ThreadedGenerator::new(generator, generation_worker_count),
            desired_coords: AHashSet::new(),
            max_inflight_generations: if max_inflight_generations == 0 {
                usize::MAX
            } else {
                max_inflight_generations
            },
        }
    }

    pub fn pump(
        &mut self,
        world: &mut World,
        renderer: Option<&mut Renderer>,
        focus: MeshingFocus,
        render_radius_xz: i32,
        render_radius_y: i32,
        generation_budget: usize,
        completed_chunk_budget: usize,
    ) -> anyhow::Result<()> {
        let desired_coords =
            chunk_coords_in_render_radius(focus, render_radius_xz, render_radius_y);
        let desired_set: AHashSet<_> = desired_coords.iter().copied().collect();

        let unloaded_chunk_count = self.prune_unwanted(world, renderer, &desired_set);
        let generated_chunk_count =
            self.drain_ready_results(world, &desired_set, completed_chunk_budget);
        let queued_chunk_count =
            self.enqueue_missing_chunks(world, &desired_coords, generation_budget)?;
        self.desired_coords = desired_set;

        if generated_chunk_count > 0 || unloaded_chunk_count > 0 || queued_chunk_count > 0 {
            crate::log_debug!(
                "Streaming world around {:?}: queued {}, generated {}, unloaded {}, loaded {}, pending {}",
                focus.center.0,
                queued_chunk_count,
                generated_chunk_count,
                unloaded_chunk_count,
                world.chunks.len(),
                self.generator.pending_count()
            );
        }

        Ok(())
    }

    pub fn finish_generation(
        &mut self,
        world: &mut World,
        focus: MeshingFocus,
        render_radius_xz: i32,
        render_radius_y: i32,
        generation_budget: usize,
    ) -> anyhow::Result<()> {
        let desired_coords =
            chunk_coords_in_render_radius(focus, render_radius_xz, render_radius_y);
        let desired_set: AHashSet<_> = desired_coords.iter().copied().collect();

        self.prune_unwanted(world, None, &desired_set);
        self.drain_ready_results(world, &desired_set, 0);
        self.enqueue_missing_chunks(world, &desired_coords, generation_budget)?;

        while desired_coords.iter().any(|coord| !world.contains_chunk(*coord)) {
            let result = self.generator.recv_ready()?;
            let _ = self.insert_generated_chunk(world, &desired_set, result);
            self.enqueue_missing_chunks(world, &desired_coords, generation_budget)?;
        }

        self.desired_coords = desired_set;

        Ok(())
    }

    fn prune_unwanted(
        &mut self,
        world: &mut World,
        mut renderer: Option<&mut Renderer>,
        desired_set: &AHashSet<ChunkCoord>,
    ) -> usize {
        let mut coords_to_unload: AHashSet<_> =
            self.desired_coords.difference(desired_set).copied().collect();
        coords_to_unload
            .extend(world.chunks.keys().copied().filter(|coord| !desired_set.contains(coord)));

        let unloaded_chunk_count = coords_to_unload.len();

        for coord in coords_to_unload {
            self.generator.cancel(coord);
            world.remove_chunk(coord);
            if let Some(renderer) = renderer.as_deref_mut() {
                renderer.remove_chunk_mesh(coord);
            }
        }

        unloaded_chunk_count
    }

    fn drain_ready_results(
        &mut self,
        world: &mut World,
        desired_set: &AHashSet<ChunkCoord>,
        completed_chunk_budget: usize,
    ) -> usize {
        let mut generated_chunk_count = 0usize;

        let completed_chunk_budget =
            if completed_chunk_budget == 0 { usize::MAX } else { completed_chunk_budget };

        for result in self.generator.try_take_ready_limit(completed_chunk_budget) {
            generated_chunk_count +=
                usize::from(self.insert_generated_chunk(world, desired_set, result));
        }

        generated_chunk_count
    }

    fn insert_generated_chunk(
        &mut self,
        world: &mut World,
        desired_set: &AHashSet<ChunkCoord>,
        result: GenerationResult,
    ) -> bool {
        if !desired_set.contains(&result.coord) || world.contains_chunk(result.coord) {
            return false;
        }

        world.insert_chunk(result.coord, result.chunk);
        true
    }

    fn enqueue_missing_chunks(
        &mut self,
        world: &World,
        desired_coords: &[ChunkCoord],
        generation_budget: usize,
    ) -> anyhow::Result<usize> {
        let generation_budget = if generation_budget == 0 { usize::MAX } else { generation_budget };
        let mut available_slots =
            self.max_inflight_generations.saturating_sub(self.generator.pending_count());
        let mut queued_chunk_count = 0usize;

        for &coord in desired_coords {
            if queued_chunk_count >= generation_budget || available_slots == 0 {
                break;
            }
            if world.contains_chunk(coord) || self.generator.is_pending(coord) {
                continue;
            }
            if self.generator.enqueue(coord)? {
                queued_chunk_count += 1;
                available_slots = available_slots.saturating_sub(1);
            }
        }

        Ok(queued_chunk_count)
    }
}

pub fn meshing_focus_from_player(player: &Player) -> MeshingFocus {
    MeshingFocus::new(
        ChunkCoord::from_world_voxel(player.position.floor().as_ivec3()),
        player.forward_3d(),
    )
}

fn chunk_coords_in_render_radius(
    focus: MeshingFocus,
    render_radius_xz: i32,
    render_radius_y: i32,
) -> Vec<ChunkCoord> {
    let xz_diameter = (render_radius_xz * 2 + 1).max(0) as usize;
    let y_diameter = (render_radius_y * 2 + 1).max(0) as usize;
    let mut coords = Vec::with_capacity(xz_diameter * xz_diameter * y_diameter);

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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::engine::{
        core::math::Vec3,
        world::{block::id::BlockId, chunk::Chunk, voxel::Voxel},
    };

    use super::*;

    struct TestGenerator {
        fill_block: BlockId,
    }

    impl ChunkGenerator for TestGenerator {
        fn generate(&self, _coord: ChunkCoord, chunk: &mut Chunk) {
            let voxel = Voxel::from_block_id(self.fill_block);
            chunk.voxels.fill(voxel);
            chunk.bump_generation();
        }
    }

    #[test]
    fn finish_generation_loads_missing_chunks_async() -> anyhow::Result<()> {
        let mut world = World::new();
        let center = ChunkCoord(IVec3::new(2, 3, 4));
        let mut streamer =
            WorldStreamer::new(Arc::new(TestGenerator { fill_block: BlockId(7) }), 1, 1);

        streamer.finish_generation(&mut world, MeshingFocus::new(center, Vec3::Z), 0, 0, 1)?;

        let chunk = world.chunks.get(&center).expect("expected center chunk to be generated");
        assert_eq!(chunk.get(0, 0, 0), Voxel::from_block_id(BlockId(7)));
        Ok(())
    }
}
