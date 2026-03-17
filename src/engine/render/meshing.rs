use std::{
    sync::Arc,
    thread::{self, JoinHandle},
};

use ahash::AHashMap;
use anyhow::Context;
use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::engine::{
    core::{math::IVec3, types::FaceDir},
    render::gpu_types::ChunkMeshCpu,
    world::{
        accessor::WorldVoxelReader, block::resolved::ResolvedBlockRegistry, chunk::Chunk,
        coord::ChunkCoord, mesher::build_chunk_mesh, storage::World, voxel::Voxel,
    },
};

struct MeshJob {
    snapshot: ChunkMeshingSnapshot,
}

pub struct MeshResult {
    pub coord: ChunkCoord,
    pub generation: u32,
    pub mesh: ChunkMeshCpu,
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
    pending_generations: AHashMap<ChunkCoord, u32>,
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

        Self { job_tx: Some(job_tx), result_rx, pending_generations: AHashMap::new(), workers }
    }

    pub fn enqueue_dirty(&mut self, world: &mut World) -> anyhow::Result<()> {
        let Some(job_tx) = &self.job_tx else {
            anyhow::bail!("meshing workers are unavailable");
        };

        for coord in world.take_dirty() {
            let Some(snapshot) = ChunkMeshingSnapshot::capture(world, coord) else {
                self.pending_generations.remove(&coord);
                continue;
            };

            let generation = snapshot.generation;
            if self.pending_generations.get(&coord).is_some_and(|&queued| queued >= generation) {
                continue;
            }

            self.pending_generations.insert(coord, generation);
            job_tx
                .send(MeshJob { snapshot })
                .context("failed to send chunk meshing job to worker thread")?;
        }

        Ok(())
    }

    pub fn has_pending(&self) -> bool {
        !self.pending_generations.is_empty()
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
        match self.pending_generations.get(&result.coord).copied() {
            Some(expected_generation) if expected_generation == result.generation => {
                self.pending_generations.remove(&result.coord);
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

        if result_tx.send(MeshResult { coord, generation, mesh }).is_err() {
            break;
        }
    }
}
