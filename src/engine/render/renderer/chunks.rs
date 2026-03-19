use crate::engine::{
    render::{gpu_types::ChunkMeshCpu, meshing::MeshingFocus},
    world::{coord::ChunkCoord, storage::World},
};

use super::Renderer;

impl Renderer {
    pub fn pump_meshing(&mut self, world: &mut World, focus: MeshingFocus) -> anyhow::Result<()> {
        let _span = crate::profile_span!("renderer::pump_meshing");
        self.meshing_last_faces_uploaded = 0;
        self.meshing_last_slice_buffer_growths = 0;

        for result in self.mesher.try_take_ready_limit(self.mesh_upload_budget) {
            self.finish_chunk_mesh_result(world, result.coord, result.mesh, result.profile)?;
        }
        self.mesher.enqueue_dirty(world, focus, self.meshing_enqueue_budget)?;

        Ok(())
    }

    pub fn finish_meshing(&mut self, world: &mut World, focus: MeshingFocus) -> anyhow::Result<()> {
        let _span = crate::profile_span!("renderer::finish_meshing");
        self.meshing_last_faces_uploaded = 0;
        self.meshing_last_slice_buffer_growths = 0;

        self.mesher.enqueue_dirty(world, focus, 0)?;

        while self.mesher.has_pending_work() {
            if self.mesher.has_inflight_jobs() {
                let result = self.mesher.recv_ready()?;
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
        world.mark_chunk_meshed(coord, generation);
        self.meshing_last_faces_uploaded += mesh.face_count();
        self.meshing_last_slice_buffer_growths += profile.slice_buffer_growths;
        self.upload_chunk_mesh(coord, mesh)?;
        world.mark_chunk_uploaded(coord, generation);
        Ok(())
    }
}
