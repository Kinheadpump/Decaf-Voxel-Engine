use std::{
    sync::Arc,
    thread::{self, JoinHandle},
};

use ahash::AHashMap;
use anyhow::Context;
use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::engine::{
    core::{
        math::{IVec3, Vec3},
        types::FaceDir,
    },
    render::gpu_types::ChunkMeshCpu,
    world::{
        accessor::WorldVoxelReader, block::resolved::ResolvedBlockRegistry, chunk::Chunk,
        coord::ChunkCoord, mesher::build_chunk_mesh, storage::World, voxel::Voxel,
    },
};

struct MeshJob {
    request_id: u64,
    snapshot: ChunkMeshingSnapshot,
}

pub struct MeshResult {
    pub coord: ChunkCoord,
    pub request_id: u64,
    pub generation: u32,
    pub mesh: ChunkMeshCpu,
}

#[derive(Clone, Copy, Debug)]
pub struct MeshingFocus {
    pub center: ChunkCoord,
    pub forward: Vec3,
}

impl MeshingFocus {
    pub fn new(center: ChunkCoord, forward: Vec3) -> Self {
        Self { center, forward: forward.normalize_or_zero() }
    }
}

#[derive(Clone, Copy, Debug)]
struct PendingJob {
    request_id: u64,
    generation: u32,
}

struct ChunkMeshingSnapshot {
    coord: ChunkCoord,
    generation: u32,
    center: Chunk,
    neighbors: [Option<Chunk>; 6],
}

impl ChunkMeshingSnapshot {
    fn capture(world: &World, coord: ChunkCoord) -> Option<Self> {
        let center = world.chunks.get(&coord)?.clone();
        let neighbors = std::array::from_fn(|i| {
            let dir = FaceDir::ALL[i];
            world.chunks.get(&coord.offset(dir.normal())).cloned()
        });

        Some(Self { coord, generation: center.generation, center, neighbors })
    }

    fn chunk_for_coord(&self, coord: ChunkCoord) -> Option<&Chunk> {
        let delta = coord.0 - self.coord.0;

        match (delta.x, delta.y, delta.z) {
            (0, 0, 0) => Some(&self.center),
            (1, 0, 0) => self.neighbors[FaceDir::PosX as usize].as_ref(),
            (-1, 0, 0) => self.neighbors[FaceDir::NegX as usize].as_ref(),
            (0, 1, 0) => self.neighbors[FaceDir::PosY as usize].as_ref(),
            (0, -1, 0) => self.neighbors[FaceDir::NegY as usize].as_ref(),
            (0, 0, 1) => self.neighbors[FaceDir::PosZ as usize].as_ref(),
            (0, 0, -1) => self.neighbors[FaceDir::NegZ as usize].as_ref(),
            _ => None,
        }
    }
}

impl WorldVoxelReader for ChunkMeshingSnapshot {
    fn get_world_voxel(&self, p: IVec3) -> Voxel {
        let coord = ChunkCoord::from_world_voxel(p);
        let local = coord.local_voxel(p);

        self.chunk_for_coord(coord)
            .map(|chunk| chunk.get(local.x as usize, local.y as usize, local.z as usize))
            .unwrap_or(Voxel::AIR)
    }
}

pub struct ThreadedMesher {
    job_tx: Option<Sender<MeshJob>>,
    result_rx: Receiver<MeshResult>,
    pending_jobs: AHashMap<ChunkCoord, PendingJob>,
    next_request_id: u64,
    workers: Vec<JoinHandle<()>>,
}

impl ThreadedMesher {
    pub fn new(resolved_blocks: ResolvedBlockRegistry) -> Self {
        let (job_tx, job_rx) = unbounded::<MeshJob>();
        let (result_tx, result_rx) = unbounded::<MeshResult>();
        let resolved_blocks = Arc::new(resolved_blocks);
        let worker_count = thread::available_parallelism()
            .map(|count| count.get().saturating_sub(1).max(1))
            .unwrap_or(1);
        crate::log_debug!("Starting {worker_count} chunk meshing worker threads");

        let mut workers = Vec::with_capacity(worker_count);

        for worker_index in 0..worker_count {
            let job_rx = job_rx.clone();
            let result_tx = result_tx.clone();
            let resolved_blocks = Arc::clone(&resolved_blocks);
            let thread_name = format!("chunk-mesher-{worker_index}");

            let handle = thread::Builder::new()
                .name(thread_name)
                .spawn(move || worker_loop(job_rx, result_tx, resolved_blocks))
                .expect("failed to spawn chunk meshing worker thread");

            workers.push(handle);
        }

        Self {
            job_tx: Some(job_tx),
            result_rx,
            pending_jobs: AHashMap::new(),
            next_request_id: 1,
            workers,
        }
    }

    pub fn enqueue_dirty(&mut self, world: &mut World, focus: MeshingFocus) -> anyhow::Result<()> {
        let Some(job_tx) = &self.job_tx else {
            anyhow::bail!("meshing workers are unavailable");
        };

        let mut dirty = world.take_dirty();
        sort_chunk_coords_by_priority(&mut dirty, focus);

        for coord in dirty {
            let Some(snapshot) = ChunkMeshingSnapshot::capture(world, coord) else {
                self.pending_jobs.remove(&coord);
                continue;
            };

            let generation = snapshot.generation;
            if self.pending_jobs.get(&coord).is_some_and(|pending| pending.generation >= generation)
            {
                continue;
            }

            let request_id = self.next_request_id;
            self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
            self.pending_jobs.insert(coord, PendingJob { request_id, generation });
            job_tx
                .send(MeshJob { request_id, snapshot })
                .context("failed to send chunk meshing job to worker thread")?;
        }

        Ok(())
    }

    pub fn cancel(&mut self, coord: ChunkCoord) {
        self.pending_jobs.remove(&coord);
    }

    pub fn has_pending(&self) -> bool {
        !self.pending_jobs.is_empty()
    }

    pub fn pending_count(&self) -> usize {
        self.pending_jobs.len()
    }

    pub fn try_take_ready(&mut self) -> Vec<MeshResult> {
        let mut ready = Vec::new();

        while let Ok(result) = self.result_rx.try_recv() {
            if let Some(result) = self.accept_result(result) {
                ready.push(result);
            }
        }

        ready
    }

    pub fn recv_ready(&mut self) -> anyhow::Result<MeshResult> {
        loop {
            let result = self
                .result_rx
                .recv()
                .context("chunk meshing worker result channel disconnected")?;

            if let Some(result) = self.accept_result(result) {
                return Ok(result);
            }
        }
    }

    fn accept_result(&mut self, result: MeshResult) -> Option<MeshResult> {
        match self.pending_jobs.get(&result.coord).copied() {
            Some(pending)
                if pending.request_id == result.request_id
                    && pending.generation == result.generation =>
            {
                self.pending_jobs.remove(&result.coord);
                Some(result)
            }
            _ => None,
        }
    }
}

impl Drop for ThreadedMesher {
    fn drop(&mut self) {
        let _ = self.job_tx.take();

        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker_loop(
    job_rx: Receiver<MeshJob>,
    result_tx: Sender<MeshResult>,
    resolved_blocks: Arc<ResolvedBlockRegistry>,
) {
    if let Some(client) = tracy_client::Client::running() {
        if let Some(thread_name) = thread::current().name() {
            client.set_thread_name(thread_name);
        }
    }

    while let Ok(job) = job_rx.recv() {
        let _span = crate::profile_span!("mesher::build_chunk_mesh");
        let coord = job.snapshot.coord;
        let generation = job.snapshot.generation;
        let mesh =
            build_chunk_mesh(coord, &job.snapshot.center, &job.snapshot, resolved_blocks.as_ref());

        if result_tx
            .send(MeshResult { coord, request_id: job.request_id, generation, mesh })
            .is_err()
        {
            break;
        }
    }
}

pub fn sort_chunk_coords_by_priority(coords: &mut [ChunkCoord], focus: MeshingFocus) {
    coords.sort_by(|a, b| chunk_priority_key(*a, focus).cmp(&chunk_priority_key(*b, focus)));
}

fn chunk_priority_key(coord: ChunkCoord, focus: MeshingFocus) -> (i32, i32, i32, i32, i32) {
    let delta = coord.0 - focus.center.0;
    let dist_sq = delta.x * delta.x + delta.z * delta.z + delta.y * delta.y * 4;
    let delta_vec = Vec3::new(delta.x as f32, delta.y as f32 * 0.5, delta.z as f32);
    let forward_bias = if delta_vec.length_squared() > 0.0 {
        -(focus.forward.dot(delta_vec.normalize()) * 1024.0) as i32
    } else {
        i32::MIN
    };

    (dist_sq, forward_bias, delta.y.abs(), delta.x.abs(), delta.z.abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_prefers_nearby_chunks() {
        let focus = MeshingFocus::new(ChunkCoord(IVec3::ZERO), Vec3::Z);
        let mut coords = [ChunkCoord(IVec3::new(3, 0, 0)), ChunkCoord(IVec3::new(1, 0, 0))];

        sort_chunk_coords_by_priority(&mut coords, focus);

        assert_eq!(coords[0], ChunkCoord(IVec3::new(1, 0, 0)));
    }

    #[test]
    fn priority_prefers_forward_chunks_at_equal_distance() {
        let focus = MeshingFocus::new(ChunkCoord(IVec3::ZERO), Vec3::new(0.0, 0.0, 1.0));
        let mut coords = [ChunkCoord(IVec3::new(-1, 0, 0)), ChunkCoord(IVec3::new(0, 0, 1))];

        sort_chunk_coords_by_priority(&mut coords, focus);

        assert_eq!(coords[0], ChunkCoord(IVec3::new(0, 0, 1)));
    }
}
