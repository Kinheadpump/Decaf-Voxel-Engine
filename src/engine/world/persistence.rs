use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::engine::world::{
    block::{id::BlockId, registry::BlockRegistry},
    coord::WorldVoxelPos,
    storage::World,
};

const WORLD_SAVE_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct WorldSaveFile {
    version: u32,
    #[serde(default)]
    edits: Vec<SavedBlockEdit>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SavedBlockEdit {
    x: i32,
    y: i32,
    z: i32,
    block: String,
}

pub fn load_block_edits(
    path: &Path,
    block_registry: &BlockRegistry,
) -> anyhow::Result<Vec<(WorldVoxelPos, BlockId)>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let save: WorldSaveFile = toml::from_str(&contents)
        .with_context(|| format!("failed to parse world save {}", path.display()))?;

    if save.version != WORLD_SAVE_VERSION {
        anyhow::bail!("unsupported world save version {} in {}", save.version, path.display());
    }

    let mut edits = Vec::with_capacity(save.edits.len());
    for edit in save.edits {
        let Some(block_id) = block_registry.get_id(&edit.block) else {
            crate::log_warn!(
                "Skipping persisted block edit at ({}, {}, {}) with unknown block '{}'",
                edit.x,
                edit.y,
                edit.z,
                edit.block
            );
            continue;
        };
        edits.push((WorldVoxelPos::new(edit.x, edit.y, edit.z), block_id));
    }

    Ok(edits)
}

pub fn save_block_edits(
    path: &Path,
    world: &World,
    block_registry: &BlockRegistry,
) -> anyhow::Result<()> {
    let mut edits = Vec::new();
    for (position, block_id) in world.iter_persistent_edits() {
        let block = block_registry
            .get(block_id)
            .with_context(|| format!("block id {} is not registered", block_id.0))?;
        edits.push(SavedBlockEdit {
            x: position.as_ivec3().x,
            y: position.as_ivec3().y,
            z: position.as_ivec3().z,
            block: block.name.clone(),
        });
    }

    edits.sort_by(|a, b| (a.x, a.y, a.z, &a.block).cmp(&(b.x, b.y, b.z, &b.block)));

    if edits.is_empty() {
        if path.exists() {
            fs::remove_file(path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
        return Ok(());
    }

    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let contents = toml::to_string_pretty(&WorldSaveFile { version: WORLD_SAVE_VERSION, edits })
        .context("failed to encode world save")?;
    fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub fn save_path(path: &str) -> PathBuf {
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use std::{
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::engine::core::math::IVec3;
    use crate::engine::world::{
        block::create_default_block_registry, chunk::Chunk, coord::ChunkCoord,
    };

    use super::*;

    #[test]
    fn block_edits_round_trip_through_disk() -> anyhow::Result<()> {
        let registry = create_default_block_registry();
        let stone = registry.must_get_id("stone");
        let mut world = World::new();
        let coord = ChunkCoord(IVec3::ZERO);
        let save_path = unique_test_save_path();

        world.insert_chunk(coord, Chunk::new());
        let _ = world.take_dirty();
        assert!(world.set_block_world(IVec3::new(2, 3, 4), stone));

        save_block_edits(&save_path, &world, &registry)?;
        let edits = load_block_edits(&save_path, &registry)?;

        assert_eq!(edits, vec![(WorldVoxelPos::new(2, 3, 4), stone)]);

        if save_path.exists() {
            fs::remove_file(&save_path)?;
        }
        Ok(())
    }

    fn unique_test_save_path() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("decaf-world-save-{}-{timestamp}.toml", process::id()))
    }
}
