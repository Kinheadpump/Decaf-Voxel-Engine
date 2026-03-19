use std::{
    ffi::OsString,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{
    config::WorldConfig,
    engine::world::{
        block::{id::BlockId, registry::BlockRegistry},
        coord::WorldVoxelPos,
        storage::World,
    },
};

const WORLD_SAVE_VERSION: u32 = 2;
const WORLD_SAVE_KIND: &str = "decaf_world";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldSaveContext {
    pub seed: u64,
    pub biomes_file: String,
}

impl WorldSaveContext {
    pub fn from_world_config(world: &WorldConfig) -> Self {
        Self { seed: world.seed, biomes_file: world.biomes_file.clone() }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct WorldSaveFile {
    version: u32,
    metadata: WorldSaveMetadata,
    #[serde(default)]
    edits: Vec<SavedBlockEdit>,
}

#[derive(Debug, Deserialize)]
struct LegacyWorldSaveFile {
    version: u32,
    #[serde(default)]
    edits: Vec<SavedBlockEdit>,
}

#[derive(Debug, Deserialize)]
struct SaveFileHeader {
    version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorldSaveMetadata {
    kind: String,
    seed: u64,
    biomes_file: String,
    created_at_unix_ms: u64,
    saved_at_unix_ms: u64,
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
    expected: &WorldSaveContext,
    block_registry: &BlockRegistry,
) -> anyhow::Result<Vec<(WorldVoxelPos, BlockId)>> {
    let backup_path = backup_path(path);
    if !path.exists() && !backup_path.exists() {
        return Ok(Vec::new());
    }

    if path.exists() {
        match load_block_edits_from_path(path, expected, block_registry) {
            Ok(edits) => return Ok(edits),
            Err(primary_err) => {
                if backup_path.exists() {
                    crate::log_warn!(
                        "Failed to load world save {}; trying backup {}: {primary_err:#}",
                        path.display(),
                        backup_path.display()
                    );
                    let edits = load_block_edits_from_path(&backup_path, expected, block_registry)
                        .with_context(|| {
                            format!(
                                "failed to load world save {} and backup {}",
                                path.display(),
                                backup_path.display()
                            )
                        })?;
                    crate::log_warn!(
                        "Recovered persisted world edits from backup {}",
                        backup_path.display()
                    );
                    return Ok(edits);
                }

                return Err(primary_err);
            }
        }
    }

    crate::log_warn!(
        "Primary world save {} is missing; loading backup {}",
        path.display(),
        backup_path.display()
    );
    load_block_edits_from_path(&backup_path, expected, block_registry)
        .with_context(|| format!("failed to load backup world save {}", backup_path.display()))
}

pub fn save_block_edits(
    path: &Path,
    context: &WorldSaveContext,
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

    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let metadata = build_save_metadata(path, context, block_registry);
    let contents =
        toml::to_string_pretty(&WorldSaveFile { version: WORLD_SAVE_VERSION, metadata, edits })
            .context("failed to encode world save")?;
    write_world_save_atomically(path, &contents, block_registry)?;
    Ok(())
}

pub fn save_path(path: &str) -> PathBuf {
    PathBuf::from(path)
}

fn load_block_edits_from_path(
    path: &Path,
    expected: &WorldSaveContext,
    block_registry: &BlockRegistry,
) -> anyhow::Result<Vec<(WorldVoxelPos, BlockId)>> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let save = match parse_world_save(path, &contents, block_registry) {
        Ok(save) => save,
        Err(err) => {
            if path.exists() {
                if let Ok(corrupt_path) = archive_corrupt_save(path) {
                    return Err(anyhow::anyhow!(
                        "{}; archived unreadable world save to {}",
                        err,
                        corrupt_path.display()
                    ));
                }
            }
            return Err(err);
        }
    };
    if let Some(metadata) = save.metadata.as_ref() {
        warn_on_metadata_mismatch(path, metadata, expected);
    } else {
        crate::log_info!(
            "Loading legacy world save {} without metadata; it will be upgraded on the next save",
            path.display()
        );
    }
    Ok(save.edits)
}

#[derive(Debug)]
struct LoadedWorldSave {
    metadata: Option<WorldSaveMetadata>,
    edits: Vec<(WorldVoxelPos, BlockId)>,
}

fn parse_world_save(
    path: &Path,
    contents: &str,
    block_registry: &BlockRegistry,
) -> anyhow::Result<LoadedWorldSave> {
    let header: SaveFileHeader = toml::from_str(contents)
        .with_context(|| format!("failed to read save header from {}", path.display()))?;

    match header.version {
        1 => {
            let save: LegacyWorldSaveFile = toml::from_str(contents)
                .with_context(|| format!("failed to parse legacy world save {}", path.display()))?;
            debug_assert_eq!(save.version, 1);
            Ok(LoadedWorldSave {
                metadata: None,
                edits: resolve_saved_block_edits(save.edits, block_registry),
            })
        }
        WORLD_SAVE_VERSION => {
            let save: WorldSaveFile = toml::from_str(contents)
                .with_context(|| format!("failed to parse world save {}", path.display()))?;

            if save.metadata.kind != WORLD_SAVE_KIND {
                anyhow::bail!(
                    "unsupported world save kind '{}' in {}",
                    save.metadata.kind,
                    path.display()
                );
            }

            Ok(LoadedWorldSave {
                metadata: Some(save.metadata),
                edits: resolve_saved_block_edits(save.edits, block_registry),
            })
        }
        other => anyhow::bail!("unsupported world save version {} in {}", other, path.display()),
    }
}

fn resolve_saved_block_edits(
    saved_edits: Vec<SavedBlockEdit>,
    block_registry: &BlockRegistry,
) -> Vec<(WorldVoxelPos, BlockId)> {
    let mut edits = Vec::with_capacity(saved_edits.len());
    for edit in saved_edits {
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
    edits
}

fn warn_on_metadata_mismatch(
    path: &Path,
    metadata: &WorldSaveMetadata,
    expected: &WorldSaveContext,
) {
    if metadata.seed != expected.seed {
        crate::log_warn!(
            "World save {} was created with seed {} but the current config uses {}; persisted edits will still be applied",
            path.display(),
            metadata.seed,
            expected.seed
        );
    }

    if metadata.biomes_file != expected.biomes_file {
        crate::log_warn!(
            "World save {} references biomes file '{}' but the current config uses '{}'; persisted edits will still be applied",
            path.display(),
            metadata.biomes_file,
            expected.biomes_file
        );
    }
}

fn build_save_metadata(
    path: &Path,
    context: &WorldSaveContext,
    block_registry: &BlockRegistry,
) -> WorldSaveMetadata {
    let now = unix_timestamp_millis();
    let existing = load_existing_metadata(path, block_registry);
    let created_at_unix_ms =
        existing.as_ref().map(|metadata| metadata.created_at_unix_ms).unwrap_or(now);

    WorldSaveMetadata {
        kind: WORLD_SAVE_KIND.to_string(),
        seed: context.seed,
        biomes_file: context.biomes_file.clone(),
        created_at_unix_ms,
        saved_at_unix_ms: now,
    }
}

fn load_existing_metadata(
    path: &Path,
    block_registry: &BlockRegistry,
) -> Option<WorldSaveMetadata> {
    for candidate in [path.to_path_buf(), backup_path(path)] {
        let Ok(contents) = fs::read_to_string(&candidate) else {
            continue;
        };
        let Ok(save) = parse_world_save(&candidate, &contents, block_registry) else {
            continue;
        };
        if let Some(metadata) = save.metadata {
            return Some(metadata);
        }
    }

    None
}

fn write_world_save_atomically(
    path: &Path,
    contents: &str,
    block_registry: &BlockRegistry,
) -> anyhow::Result<()> {
    let temp_path = temporary_save_path(path);
    let backup_path = backup_path(path);
    let write_result = (|| -> anyhow::Result<()> {
        let mut file = File::create(&temp_path)
            .with_context(|| format!("failed to create {}", temp_path.display()))?;
        file.write_all(contents.as_bytes())
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
        file.sync_all().with_context(|| format!("failed to flush {}", temp_path.display()))?;

        if path.exists() {
            preserve_existing_save(path, &backup_path, block_registry)?;
        }

        fs::rename(&temp_path, path).with_context(|| {
            format!("failed to replace world save {} with {}", path.display(), temp_path.display())
        })?;

        Ok(())
    })();

    if write_result.is_err() && temp_path.exists() {
        let _ = fs::remove_file(&temp_path);
    }

    write_result
}

fn preserve_existing_save(
    path: &Path,
    backup_path: &Path,
    block_registry: &BlockRegistry,
) -> anyhow::Result<()> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    match parse_world_save(path, &contents, block_registry) {
        Ok(_) => {
            if backup_path.exists() {
                fs::remove_file(backup_path).with_context(|| {
                    format!("failed to remove old backup {}", backup_path.display())
                })?;
            }
            fs::rename(path, backup_path).with_context(|| {
                format!("failed to rotate {} to backup {}", path.display(), backup_path.display())
            })?;
        }
        Err(err) => {
            let corrupt_path = archive_corrupt_save(path)?;
            crate::log_warn!(
                "Existing world save {} was corrupt during save and was archived to {}: {err:#}",
                path.display(),
                corrupt_path.display()
            );
        }
    }

    Ok(())
}

fn archive_corrupt_save(path: &Path) -> anyhow::Result<PathBuf> {
    let corrupt_path = corrupt_save_path(path);
    fs::rename(path, &corrupt_path).with_context(|| {
        format!(
            "failed to archive corrupt world save {} to {}",
            path.display(),
            corrupt_path.display()
        )
    })?;
    Ok(corrupt_path)
}

fn backup_path(path: &Path) -> PathBuf {
    append_path_suffix(path, ".bak")
}

fn temporary_save_path(path: &Path) -> PathBuf {
    append_path_suffix(path, &format!(".tmp-{}-{}", process::id(), unix_timestamp_millis()))
}

fn corrupt_save_path(path: &Path) -> PathBuf {
    append_path_suffix(path, &format!(".corrupt-{}", unix_timestamp_millis()))
}

fn append_path_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(suffix);
    PathBuf::from(value)
}

fn unix_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::process;

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
        let save_context = test_save_context();

        world.insert_chunk(coord, Chunk::new());
        let _ = world.take_dirty();
        assert!(world.set_block_world(IVec3::new(2, 3, 4), stone));

        save_block_edits(&save_path, &save_context, &world, &registry)?;
        let edits = load_block_edits(&save_path, &save_context, &registry)?;

        assert_eq!(edits, vec![(WorldVoxelPos::new(2, 3, 4), stone)]);
        let contents = fs::read_to_string(&save_path)?;
        let saved: WorldSaveFile = toml::from_str(&contents)?;
        assert_eq!(saved.version, WORLD_SAVE_VERSION);
        assert_eq!(saved.metadata.seed, save_context.seed);
        assert_eq!(saved.metadata.biomes_file, save_context.biomes_file);

        cleanup_save_family(&save_path)?;
        Ok(())
    }

    #[test]
    fn legacy_v1_save_loads_and_upgrades_on_next_write() -> anyhow::Result<()> {
        let registry = create_default_block_registry();
        let stone = registry.must_get_id("stone");
        let save_path = unique_test_save_path();
        let save_context = test_save_context();

        fs::write(
            &save_path,
            r#"
version = 1

[[edits]]
x = 2
y = 3
z = 4
block = "stone"
"#,
        )?;

        let edits = load_block_edits(&save_path, &save_context, &registry)?;
        assert_eq!(edits, vec![(WorldVoxelPos::new(2, 3, 4), stone)]);

        let mut world = World::new();
        let coord = ChunkCoord(IVec3::ZERO);
        world.insert_chunk(coord, Chunk::new());
        let _ = world.take_dirty();
        assert!(world.set_block_world(IVec3::new(2, 3, 4), stone));
        save_block_edits(&save_path, &save_context, &world, &registry)?;

        let contents = fs::read_to_string(&save_path)?;
        let saved: WorldSaveFile = toml::from_str(&contents)?;
        assert_eq!(saved.version, WORLD_SAVE_VERSION);
        assert_eq!(saved.metadata.kind, WORLD_SAVE_KIND);
        assert_eq!(saved.metadata.seed, save_context.seed);

        cleanup_save_family(&save_path)?;
        Ok(())
    }

    #[test]
    fn load_falls_back_to_backup_when_primary_is_corrupt() -> anyhow::Result<()> {
        let registry = create_default_block_registry();
        let stone = registry.must_get_id("stone");
        let save_path = unique_test_save_path();
        let backup_path = backup_path(&save_path);
        let save_context = test_save_context();

        let mut world = World::new();
        let coord = ChunkCoord(IVec3::ZERO);
        world.insert_chunk(coord, Chunk::new());
        let _ = world.take_dirty();
        assert!(world.set_block_world(IVec3::new(2, 3, 4), stone));
        save_block_edits(&save_path, &save_context, &world, &registry)?;
        fs::copy(&save_path, &backup_path)?;
        fs::write(&save_path, "not valid toml at all")?;

        let edits = load_block_edits(&save_path, &save_context, &registry)?;
        assert_eq!(edits, vec![(WorldVoxelPos::new(2, 3, 4), stone)]);
        assert!(!save_path.exists());
        assert!(backup_path.exists());

        cleanup_save_family(&save_path)?;
        Ok(())
    }

    #[test]
    fn save_rotates_last_good_primary_to_backup() -> anyhow::Result<()> {
        let registry = create_default_block_registry();
        let stone = registry.must_get_id("stone");
        let oak = registry.must_get_id("oak_planks");
        let save_path = unique_test_save_path();
        let backup_path = backup_path(&save_path);
        let save_context = test_save_context();

        let mut world = World::new();
        let coord = ChunkCoord(IVec3::ZERO);
        world.insert_chunk(coord, Chunk::new());
        let _ = world.take_dirty();
        assert!(world.set_block_world(IVec3::new(2, 3, 4), stone));
        save_block_edits(&save_path, &save_context, &world, &registry)?;

        assert!(world.set_block_world(IVec3::new(5, 6, 7), oak));
        save_block_edits(&save_path, &save_context, &world, &registry)?;

        let current_edits = load_block_edits(&save_path, &save_context, &registry)?;
        let backup_edits = load_block_edits(&backup_path, &save_context, &registry)?;

        assert_eq!(
            current_edits,
            vec![(WorldVoxelPos::new(2, 3, 4), stone), (WorldVoxelPos::new(5, 6, 7), oak),]
        );
        assert_eq!(backup_edits, vec![(WorldVoxelPos::new(2, 3, 4), stone)]);

        cleanup_save_family(&save_path)?;
        Ok(())
    }

    fn unique_test_save_path() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("decaf-world-save-{}-{timestamp}.toml", process::id()))
    }

    fn test_save_context() -> WorldSaveContext {
        WorldSaveContext { seed: 12345, biomes_file: "biomes.toml".to_string() }
    }

    fn cleanup_save_family(save_path: &Path) -> anyhow::Result<()> {
        let parent = save_path.parent().context("save path should have a parent")?;
        let prefix = save_path
            .file_name()
            .context("save path should have a file name")?
            .to_string_lossy()
            .to_string();

        for entry in fs::read_dir(parent)? {
            let entry = entry?;
            let path = entry.path();
            let Some(file_name) = path.file_name() else {
                continue;
            };
            if file_name.to_string_lossy().starts_with(&prefix) && path.is_file() {
                let _ = fs::remove_file(path);
            }
        }

        Ok(())
    }
}
