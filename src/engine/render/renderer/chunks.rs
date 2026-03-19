use std::time::Instant;

use crate::engine::{
    render::{gpu_types::ChunkMeshCpu, meshing::MeshingFocus},
    world::{coord::ChunkCoord, storage::World},
};

use super::{Renderer, duration_to_nanos};

impl Renderer {
    pub fn pump_meshing(&mut self, world: &mut World, focus: MeshingFocus) -> anyhow::Result<()> {
        let _span = crate::profile_span!("renderer::pump_meshing");
        self.last_meshing_pass_stats = Default::default();

        for result in self.mesher.try_take_ready_limit(self.mesh_upload_budget) {
            self.finish_chunk_mesh_result(world, result.coord, result.mesh, result.profile)?;
        }
        self.mesher.enqueue_dirty(world, focus, self.meshing_enqueue_budget)?;

        Ok(())
    }

    pub fn finish_meshing(&mut self, world: &mut World, focus: MeshingFocus) -> anyhow::Result<()> {
        let _span = crate::profile_span!("renderer::finish_meshing");
        self.last_meshing_pass_stats = Default::default();

        self.mesher.enqueue_dirty(world, focus, 0)?;

        while self.mesher.has_pending_work() {
            if self.mesher.has_inflight_jobs() {
                let wait_started_at = Instant::now();
                let result = self.mesher.recv_ready()?;
                self.last_meshing_pass_stats.wait_cpu_time_ns = self
                    .last_meshing_pass_stats
                    .wait_cpu_time_ns
                    .saturating_add(duration_to_nanos(wait_started_at.elapsed()));
                self.finish_chunk_mesh_result(world, result.coord, result.mesh, result.profile)?;
            }
            self.mesher.enqueue_dirty(world, focus, 0)?;
        }

        Ok(())
    }

    pub fn remove_chunk_mesh(&mut self, coord: ChunkCoord) {
        self.mesher.cancel(coord);

        if let Some(entry) = self.gpu_entries.remove(&coord) {
            self.free_gpu_entry(&entry);
        }
    }

    fn finish_chunk_mesh_result(
        &mut self,
        world: &mut World,
        coord: ChunkCoord,
        mesh: ChunkMeshCpu,
        profile: crate::engine::world::mesher::MeshingBuildProfile,
    ) -> anyhow::Result<()> {
        let generation = mesh.source_generation;
        let face_count = mesh.face_count();
        world.mark_chunk_meshed(coord, generation);
        self.last_meshing_pass_stats.chunk_results =
            self.last_meshing_pass_stats.chunk_results.saturating_add(1);
        self.last_meshing_pass_stats.faces_uploaded = self
            .last_meshing_pass_stats
            .faces_uploaded
            .saturating_add(face_count);
        self.last_meshing_pass_stats.dirty_slices = self
            .last_meshing_pass_stats
            .dirty_slices
            .saturating_add(profile.dirty_slice_count);
        self.last_meshing_pass_stats.slice_buffer_growths = self
            .last_meshing_pass_stats
            .slice_buffer_growths
            .saturating_add(profile.slice_buffer_growths);
        self.last_meshing_pass_stats.build_cpu_time_ns = self
            .last_meshing_pass_stats
            .build_cpu_time_ns
            .saturating_add(profile.build_cpu_time_ns);
        self.last_meshing_pass_stats.snapshot_capture_cpu_time_ns = self
            .last_meshing_pass_stats
            .snapshot_capture_cpu_time_ns
            .saturating_add(profile.snapshot_capture_cpu_time_ns);
        self.last_meshing_pass_stats.slice_construction_cpu_time_ns = self
            .last_meshing_pass_stats
            .slice_construction_cpu_time_ns
            .saturating_add(profile.slice_construction_cpu_time_ns);
        self.last_meshing_pass_stats.greedy_merge_cpu_time_ns = self
            .last_meshing_pass_stats
            .greedy_merge_cpu_time_ns
            .saturating_add(profile.greedy_merge_cpu_time_ns);
        self.last_meshing_pass_stats.flatten_cpu_time_ns = self
            .last_meshing_pass_stats
            .flatten_cpu_time_ns
            .saturating_add(profile.flatten_cpu_time_ns);
        let upload_started_at = Instant::now();
        self.upload_chunk_mesh(coord, mesh)?;
        self.last_meshing_pass_stats.upload_cpu_time_ns = self
            .last_meshing_pass_stats
            .upload_cpu_time_ns
            .saturating_add(duration_to_nanos(upload_started_at.elapsed()));
        world.mark_chunk_uploaded(coord, generation);
        Ok(())
    }
}
