use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    config::RenderConfig,
    engine::{
        app::streaming::WorldStreamer,
        world::{
            block::{id::BlockId, registry::BlockRegistry},
            generator::StagedGenerator,
            persistence::{self, WorldSaveContext},
        },
    },
};

pub(super) struct BackgroundServices {
    pub generation: GenerationService,
    pub persistence: PersistenceService,
}

pub(super) struct GenerationService {
    pub staged_generator: Arc<StagedGenerator>,
    pub streamer: WorldStreamer,
}

pub(super) struct PersistenceService {
    save_file: PathBuf,
    world_saver: persistence::AsyncWorldSaver,
}

impl PersistenceService {
    pub fn new(
        save_file: PathBuf,
        save_context: WorldSaveContext,
        block_registry: &BlockRegistry,
        initial_edits: impl IntoIterator<Item = (crate::engine::world::coord::WorldVoxelPos, BlockId)>,
    ) -> anyhow::Result<Self> {
        let world_saver = persistence::AsyncWorldSaver::new(
            save_file.clone(),
            save_context,
            block_registry,
            initial_edits,
        )?;
        Ok(Self { save_file, world_saver })
    }

    #[inline]
    pub fn save_file(&self) -> &Path {
        &self.save_file
    }

    pub fn queue_world_edit(
        &self,
        position: impl Into<crate::engine::world::coord::WorldVoxelPos>,
        block_id: BlockId,
    ) {
        if let Err(err) = self.world_saver.record_edit(position, block_id) {
            crate::log_warn!(
                "failed to queue world save update for {}: {err:#}",
                self.save_file.display()
            );
        }
    }

    pub fn flush(&self) -> anyhow::Result<()> {
        self.world_saver.flush()
    }
}

pub(super) fn resolve_background_worker_counts(render_config: &RenderConfig) -> (usize, usize) {
    let available_workers = std::thread::available_parallelism()
        .map(|count| count.get().saturating_sub(1).max(1))
        .unwrap_or(1);
    let generation_worker_count = if render_config.generation_worker_count == 0 {
        (available_workers / 3).max(1)
    } else {
        render_config.generation_worker_count.max(1)
    };
    let meshing_worker_count = if render_config.meshing_worker_count == 0 {
        available_workers.saturating_sub(generation_worker_count).max(1)
    } else {
        render_config.meshing_worker_count.max(1)
    };

    (generation_worker_count, meshing_worker_count)
}
