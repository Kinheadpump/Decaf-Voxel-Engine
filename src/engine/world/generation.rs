use std::{
    sync::Arc,
    thread::{self, JoinHandle},
};

use ahash::AHashMap;
use anyhow::Context;
use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::engine::world::{chunk::Chunk, coord::ChunkCoord, generator::ChunkGenerator};

struct GenerationJob {
    coord: ChunkCoord,
    request_id: u64,
}

struct WorkerGenerationResult {
    coord: ChunkCoord,
    request_id: u64,
    chunk: Chunk,
}

pub struct GenerationResult {
    pub coord: ChunkCoord,
    pub chunk: Chunk,
}

#[derive(Clone, Copy, Debug)]
struct PendingGeneration {
    request_id: u64,
}

pub struct ThreadedGenerator {
    job_tx: Option<Sender<GenerationJob>>,
    result_rx: Receiver<WorkerGenerationResult>,
    pending_jobs: AHashMap<ChunkCoord, PendingGeneration>,
    next_request_id: u64,
    workers: Vec<JoinHandle<()>>,
}

impl ThreadedGenerator {
    pub fn new(generator: Arc<dyn ChunkGenerator>, worker_count: usize) -> Self {
        let worker_count = worker_count.max(1);
        let (job_tx, job_rx) = unbounded::<GenerationJob>();
        let (result_tx, result_rx) = unbounded::<WorkerGenerationResult>();
        crate::log_debug!("Starting {worker_count} chunk generation worker threads");

        let mut workers = Vec::with_capacity(worker_count);

        for worker_index in 0..worker_count {
            let job_rx = job_rx.clone();
            let result_tx = result_tx.clone();
            let generator = Arc::clone(&generator);
            let thread_name = format!("chunk-generator-{worker_index}");

            let handle = thread::Builder::new()
                .name(thread_name)
                .spawn(move || worker_loop(job_rx, result_tx, generator))
                .expect("failed to spawn chunk generation worker thread");

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

    pub fn enqueue(&mut self, coord: ChunkCoord) -> anyhow::Result<bool> {
        if self.pending_jobs.contains_key(&coord) {
            return Ok(false);
        }

        let Some(job_tx) = &self.job_tx else {
            anyhow::bail!("generation workers are unavailable");
        };

        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        self.pending_jobs.insert(coord, PendingGeneration { request_id });
        job_tx
            .send(GenerationJob { coord, request_id })
            .context("failed to send chunk generation job to worker thread")?;
        Ok(true)
    }

    pub fn cancel(&mut self, coord: ChunkCoord) {
        self.pending_jobs.remove(&coord);
    }

    pub fn is_pending(&self, coord: ChunkCoord) -> bool {
        self.pending_jobs.contains_key(&coord)
    }

    pub fn pending_count(&self) -> usize {
        self.pending_jobs.len()
    }

    pub fn try_take_ready(&mut self) -> Vec<GenerationResult> {
        let mut ready = Vec::new();

        while let Ok(result) = self.result_rx.try_recv() {
            if let Some(result) = self.accept_result(result) {
                ready.push(result);
            }
        }

        ready
    }

    pub fn recv_ready(&mut self) -> anyhow::Result<GenerationResult> {
        loop {
            let result = self
                .result_rx
                .recv()
                .context("chunk generation worker result channel disconnected")?;

            if let Some(result) = self.accept_result(result) {
                return Ok(result);
            }
        }
    }

    fn accept_result(&mut self, result: WorkerGenerationResult) -> Option<GenerationResult> {
        match self.pending_jobs.get(&result.coord).copied() {
            Some(pending) if pending.request_id == result.request_id => {
                self.pending_jobs.remove(&result.coord);
                Some(GenerationResult { coord: result.coord, chunk: result.chunk })
            }
            _ => None,
        }
    }
}

impl Drop for ThreadedGenerator {
    fn drop(&mut self) {
        let _ = self.job_tx.take();

        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker_loop(
    job_rx: Receiver<GenerationJob>,
    result_tx: Sender<WorkerGenerationResult>,
    generator: Arc<dyn ChunkGenerator>,
) {
    if let Some(client) = tracy_client::Client::running()
        && let Some(thread_name) = thread::current().name()
    {
        client.set_thread_name(thread_name);
    }

    while let Ok(job) = job_rx.recv() {
        let _span = crate::profile_span!("generator::generate_chunk");
        let mut chunk = Chunk::new();
        generator.generate(job.coord, &mut chunk);

        if result_tx
            .send(WorkerGenerationResult { coord: job.coord, request_id: job.request_id, chunk })
            .is_err()
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::engine::world::{block::id::BlockId, voxel::Voxel};

    use super::*;

    struct TestGenerator {
        fill_block: BlockId,
    }

    impl ChunkGenerator for TestGenerator {
        fn generate(&self, _coord: ChunkCoord, chunk: &mut Chunk) {
            let voxel = Voxel::from_block_id(self.fill_block);
            chunk.voxels.fill(voxel);
            chunk.dirty = true;
            chunk.generation = chunk.generation.wrapping_add(1);
        }
    }

    #[test]
    fn generated_chunks_arrive_from_worker_threads() -> anyhow::Result<()> {
        let mut generator =
            ThreadedGenerator::new(Arc::new(TestGenerator { fill_block: BlockId(9) }), 1);
        let coord = ChunkCoord(glam::IVec3::new(4, 5, 6));

        assert!(generator.enqueue(coord)?);

        let result = generator.recv_ready()?;
        assert_eq!(result.coord, coord);
        assert_eq!(result.chunk.get(0, 0, 0), Voxel::from_block_id(BlockId(9)));
        assert_eq!(generator.pending_count(), 0);
        Ok(())
    }
}
