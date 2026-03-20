use std::{
    sync::Arc,
    thread::{self, JoinHandle},
    time::Instant,
};

use ahash::AHashMap;
use anyhow::Context;
use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::engine::{
    core::{math::Vec3, types::FaceDir},
    world::{
        accessor::ChunkNeighborReader,
        block::resolved::ResolvedBlockRegistry,
        chunk::Chunk,
        coord::ChunkCoord,
        mesher::{
            ChunkMeshDirtyRegion, ChunkMeshSlices, MeshingBuildProfile, build_chunk_mesh_slices,
            rebuild_chunk_mesh_slices,
        },
        storage::{DirtyChunkEntry, World},
    },
};

struct MeshJob {
    request_id: u64,
    dirty_region: ChunkMeshDirtyRegion,
    previous_mesh: Option<ChunkMeshSlices>,
    snapshot_capture_cpu_time_ns: u64,
    snapshot: ChunkMeshingSnapshot,
}

struct WorkerMeshResult {
    pub coord: ChunkCoord,
    pub request_id: u64,
    pub generation: u32,
    pub mesh: crate::engine::render::gpu_types::ChunkMeshCpu,
    pub mesh_slices: ChunkMeshSlices,
    pub profile: MeshingBuildProfile,
}

pub struct MeshResult {
    pub coord: ChunkCoord,
    pub mesh: crate::engine::render::gpu_types::ChunkMeshCpu,
    pub profile: MeshingBuildProfile,
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
}

impl ChunkNeighborReader for ChunkMeshingSnapshot {
    fn get_chunk_neighbor(&self, center: ChunkCoord, dir: FaceDir) -> Option<&Chunk> {
        if center != self.coord {
            return None;
        }

        self.neighbors[dir as usize].as_ref()
    }
}

pub struct ThreadedMesher {
    job_tx: Option<Sender<MeshJob>>,
    result_rx: Receiver<WorkerMeshResult>,
    pending_jobs: AHashMap<ChunkCoord, PendingJob>,
    deferred_dirty: AHashMap<ChunkCoord, ChunkMeshDirtyRegion>,
    mesh_caches: AHashMap<ChunkCoord, ChunkMeshSlices>,
    next_request_id: u64,
    workers: Vec<JoinHandle<()>>,
}

impl ThreadedMesher {
    pub fn new(resolved_blocks: ResolvedBlockRegistry, worker_count: usize) -> Self {
        let (job_tx, job_rx) = unbounded::<MeshJob>();
        let (result_tx, result_rx) = unbounded::<WorkerMeshResult>();
        let resolved_blocks = Arc::new(resolved_blocks);
        let worker_count = worker_count.max(1);
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
            deferred_dirty: AHashMap::new(),
            mesh_caches: AHashMap::new(),
            next_request_id: 1,
            workers,
        }
    }

    pub fn enqueue_dirty(
        &mut self,
        world: &mut World,
        focus: MeshingFocus,
        enqueue_budget: usize,
    ) -> anyhow::Result<usize> {
        let Some(job_tx) = &self.job_tx else {
            anyhow::bail!("meshing workers are unavailable");
        };

        for entry in world.take_dirty() {
            world.mark_chunk_meshing_queued(entry.coord);
            if let Some(existing) = self.deferred_dirty.get_mut(&entry.coord) {
                existing.merge(entry.region);
            } else {
                self.deferred_dirty.insert(entry.coord, entry.region);
            }
        }

        if self.deferred_dirty.is_empty() {
            return Ok(0);
        }

        let enqueue_budget = if enqueue_budget == 0 { usize::MAX } else { enqueue_budget };
        let mut dirty: Vec<_> = self
            .deferred_dirty
            .iter()
            .map(|(&coord, &region)| DirtyChunkEntry { coord, region })
            .collect();
        sort_dirty_chunk_entries_by_priority(&mut dirty, focus);
        let mut queued_chunk_count = 0usize;

        for entry in dirty {
            if queued_chunk_count >= enqueue_budget {
                break;
            }

            let coord = entry.coord;
            if self.pending_jobs.contains_key(&coord) {
                continue;
            }

            let Some(region) = self.deferred_dirty.remove(&coord) else {
                continue;
            };
            let snapshot_started_at = Instant::now();
            let Some(snapshot) = ChunkMeshingSnapshot::capture(world, coord) else {
                self.pending_jobs.remove(&coord);
                self.deferred_dirty.remove(&coord);
                self.mesh_caches.remove(&coord);
                continue;
            };
            let snapshot_capture_cpu_time_ns = duration_to_nanos(snapshot_started_at.elapsed());

            let generation = snapshot.generation;
            let full_rebuild = region.is_full() || !self.mesh_caches.contains_key(&coord);
            let dirty_region = if full_rebuild { ChunkMeshDirtyRegion::full() } else { region };
            let previous_mesh =
                if full_rebuild { None } else { self.mesh_caches.get(&coord).cloned() };

            let request_id = self.next_request_id;
            self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
            self.pending_jobs.insert(coord, PendingJob { request_id, generation });
            world.mark_chunk_meshing(coord, generation);
            job_tx
                .send(MeshJob {
                    request_id,
                    dirty_region,
                    previous_mesh,
                    snapshot_capture_cpu_time_ns,
                    snapshot,
                })
                .context("failed to send chunk meshing job to worker thread")?;
            queued_chunk_count += 1;
        }

        Ok(queued_chunk_count)
    }

    pub fn cancel(&mut self, coord: ChunkCoord) {
        self.pending_jobs.remove(&coord);
        self.deferred_dirty.remove(&coord);
        self.mesh_caches.remove(&coord);
    }

    pub fn has_pending_work(&self) -> bool {
        !self.pending_jobs.is_empty() || !self.deferred_dirty.is_empty()
    }

    pub fn has_inflight_jobs(&self) -> bool {
        !self.pending_jobs.is_empty()
    }

    pub fn pending_count(&self) -> usize {
        self.pending_jobs.len()
            + self
                .deferred_dirty
                .keys()
                .filter(|coord| !self.pending_jobs.contains_key(coord))
                .count()
    }

    pub fn try_take_ready_limit(&mut self, max_results: usize) -> Vec<MeshResult> {
        let mut ready = Vec::new();
        let max_results = if max_results == 0 { usize::MAX } else { max_results };

        while ready.len() < max_results {
            let Ok(result) = self.result_rx.try_recv() else {
                break;
            };
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

    fn accept_result(&mut self, result: WorkerMeshResult) -> Option<MeshResult> {
        match self.pending_jobs.get(&result.coord).copied() {
            Some(pending)
                if pending.request_id == result.request_id
                    && pending.generation == result.generation =>
            {
                self.pending_jobs.remove(&result.coord);
                self.mesh_caches.insert(result.coord, result.mesh_slices);
                Some(MeshResult { coord: result.coord, mesh: result.mesh, profile: result.profile })
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
    result_tx: Sender<WorkerMeshResult>,
    resolved_blocks: Arc<ResolvedBlockRegistry>,
) {
    if let Some(client) = tracy_client::Client::running()
        && let Some(thread_name) = thread::current().name()
    {
        client.set_thread_name(thread_name);
    }

    while let Ok(job) = job_rx.recv() {
        let _span = crate::profile_span!("mesher::build_chunk_mesh");
        let coord = job.snapshot.coord;
        let generation = job.snapshot.generation;
        let (mesh_slices, mut profile) = if let Some(mut mesh_slices) = job.previous_mesh {
            let profile = rebuild_chunk_mesh_slices(
                job.dirty_region,
                coord,
                &job.snapshot.center,
                &job.snapshot,
                resolved_blocks.as_ref(),
                &mut mesh_slices,
            );
            (mesh_slices, profile)
        } else {
            build_chunk_mesh_slices(
                coord,
                &job.snapshot.center,
                &job.snapshot,
                resolved_blocks.as_ref(),
            )
        };
        let flatten_started_at = Instant::now();
        let mesh = mesh_slices.flatten();
        profile.snapshot_capture_cpu_time_ns = job.snapshot_capture_cpu_time_ns;
        profile.flatten_cpu_time_ns = duration_to_nanos(flatten_started_at.elapsed());
        profile.recompute_total();

        if result_tx
            .send(WorkerMeshResult {
                coord,
                request_id: job.request_id,
                generation,
                mesh,
                mesh_slices,
                profile,
            })
            .is_err()
        {
            break;
        }
    }
}

#[inline]
fn duration_to_nanos(duration: std::time::Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

pub fn sort_chunk_coords_by_priority(coords: &mut [ChunkCoord], focus: MeshingFocus) {
    coords.sort_by_key(|coord| chunk_priority_key(*coord, focus));
}

fn sort_dirty_chunk_entries_by_priority(entries: &mut [DirtyChunkEntry], focus: MeshingFocus) {
    entries.sort_by(|a, b| {
        chunk_priority_key(a.coord, focus).cmp(&chunk_priority_key(b.coord, focus))
    });
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
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::engine::core::math::IVec3;
    use crate::engine::world::{
        block::{create_default_block_registry, id::BlockId},
        chunk::Chunk,
        storage::World,
        voxel::Voxel,
    };

    use super::*;

    fn test_resolved_blocks() -> ResolvedBlockRegistry {
        let registry = create_default_block_registry();
        let mut texture_layers = HashMap::new();
        let mut next_layer = 0u16;

        for definition in registry.iter() {
            definition.textures.visit_refs(|texture_ref| {
                texture_layers.entry(texture_ref.0.clone()).or_insert_with(|| {
                    let layer = next_layer;
                    next_layer += 1;
                    layer
                });
            });
        }

        ResolvedBlockRegistry::build(&registry, &texture_layers)
    }

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

    #[test]
    fn enqueue_dirty_defers_chunks_that_already_have_inflight_jobs() -> anyhow::Result<()> {
        let mut mesher = ThreadedMesher::new(test_resolved_blocks(), 1);
        let coord = ChunkCoord(IVec3::ZERO);
        let mut world = World::new();
        world.insert_chunk(coord, Chunk::new());

        mesher.pending_jobs.insert(coord, PendingJob { request_id: 7, generation: 0 });
        let _ = mesher.enqueue_dirty(&mut world, MeshingFocus::new(coord, Vec3::Z), 8)?;

        assert_eq!(mesher.pending_jobs[&coord].request_id, 7);
        assert!(mesher.deferred_dirty.contains_key(&coord));
        Ok(())
    }

    #[test]
    fn snapshot_capture_shares_chunk_storage() {
        let center = ChunkCoord(IVec3::ZERO);
        let east = ChunkCoord(IVec3::X);
        let mut world = World::new();
        let mut center_chunk = Chunk::new();
        let mut east_chunk = Chunk::new();

        center_chunk.set(1, 2, 3, Voxel::from_block_id(BlockId(5)));
        east_chunk.set(4, 5, 6, Voxel::from_block_id(BlockId(6)));
        world.insert_chunk(center, center_chunk);
        world.insert_chunk(east, east_chunk);

        let snapshot = ChunkMeshingSnapshot::capture(&world, center)
            .expect("snapshot should capture loaded center chunk");
        let world_center = world.chunks.get(&center).expect("center chunk should exist");
        let world_east = world.chunks.get(&east).expect("east chunk should exist");
        let east_snapshot = snapshot.neighbors[FaceDir::PosX as usize]
            .as_ref()
            .expect("east neighbor should be captured");

        assert!(Arc::ptr_eq(&snapshot.center.voxels, &world_center.voxels));
        assert!(Arc::ptr_eq(&snapshot.center.column_biome_tints, &world_center.column_biome_tints));
        assert!(Arc::ptr_eq(&east_snapshot.voxels, &world_east.voxels));
    }
}
