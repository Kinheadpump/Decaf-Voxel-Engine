use glam::IVec3;

use crate::config::{ContinentalRegionsConfig, Density3DConfig, RiverConfig, TerrainConfig};
use crate::engine::world::{biome::BiomeTable, block::create_default_block_registry};

use super::*;

fn terrain() -> TerrainConfig {
    TerrainConfig::default()
}

fn terrain_without_density() -> TerrainConfig {
    TerrainConfig {
        density_3d: Density3DConfig { weight: 0.0, ..Density3DConfig::default() },
        ..TerrainConfig::default()
    }
}

fn terrain_without_rivers() -> TerrainConfig {
    TerrainConfig {
        rivers: RiverConfig { depth: 0.0, ..RiverConfig::default() },
        ..TerrainConfig::default()
    }
}

fn generator(seed: u64) -> StagedGenerator {
    StagedGenerator::new(
        seed,
        BlockId(4),
        terrain(),
        BiomeTable::single(BlockId(1), BlockId(2), BlockId(3)),
    )
}

fn generator_without_density(seed: u64) -> StagedGenerator {
    StagedGenerator::new(
        seed,
        BlockId(4),
        terrain_without_density(),
        BiomeTable::single(BlockId(1), BlockId(2), BlockId(3)),
    )
}

fn generator_without_rivers(seed: u64) -> StagedGenerator {
    StagedGenerator::new(
        seed,
        BlockId(4),
        terrain_without_rivers(),
        BiomeTable::single(BlockId(1), BlockId(2), BlockId(3)),
    )
}

fn generator_without_density_or_rivers(seed: u64) -> StagedGenerator {
    let mut terrain = terrain_without_density();
    terrain.rivers.depth = 0.0;

    StagedGenerator::new(
        seed,
        BlockId(4),
        terrain,
        BiomeTable::single(BlockId(1), BlockId(2), BlockId(3)),
    )
}

#[test]
fn staged_generator_is_deterministic_for_same_seed_and_chunk() {
    let generator = generator(12345);
    let coord = ChunkCoord(IVec3::new(3, -1, -2));
    let mut first = Chunk::new();
    let mut second = Chunk::new();

    generator.generate(coord, &mut first);
    generator.generate(coord, &mut second);

    assert_eq!(first.voxels, second.voxels);
}

#[test]
fn staged_generator_changes_output_when_seed_changes() {
    let first = generator(1);
    let second = generator(2);

    let first_heights: Vec<_> = (-128..=128)
        .step_by(32)
        .map(|sample_x| first.sample_column(sample_x, sample_x / 2).surface_height.round() as i32)
        .collect();
    let second_heights: Vec<_> = (-128..=128)
        .step_by(32)
        .map(|sample_x| second.sample_column(sample_x, sample_x / 2).surface_height.round() as i32)
        .collect();

    assert_ne!(first_heights, second_heights);
}

#[test]
fn staged_generator_produces_surface_and_air_in_spawn_chunk() {
    let generator = generator(12345);
    let surface_y = generator.top_solid_y_at(0, 0);
    let coord = ChunkCoord::from_world_voxel(IVec3::new(0, surface_y + 1, 0));
    let mut chunk = Chunk::new();

    generator.generate(coord, &mut chunk);

    let has_open_space = chunk
        .voxels
        .iter()
        .any(|voxel| voxel.block_id() == BlockId::AIR || voxel.block_id() == BlockId(4));
    let has_surface = chunk
        .voxels
        .iter()
        .any(|voxel| voxel.block_id() == BlockId(1) || voxel.block_id() == BlockId(2));
    let has_ground = chunk
        .voxels
        .iter()
        .any(|voxel| voxel.block_id() != BlockId::AIR && voxel.block_id() != BlockId(4));

    assert!(has_open_space);
    assert!(has_surface);
    assert!(has_ground);
}

#[test]
fn staged_generator_without_density_has_no_hollow_columns() {
    let generator = generator_without_density(12345);
    let mut chunk = Chunk::new();
    let coord = ChunkCoord(IVec3::ZERO);

    generator.generate(coord, &mut chunk);

    for z in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            let mut hit_solid = false;

            for y in (0..CHUNK_SIZE).rev() {
                let voxel = chunk.get(x, y, z);
                let is_solid = voxel.block_id() != BlockId::AIR && voxel.block_id() != BlockId(4);

                if is_solid {
                    hit_solid = true;
                    continue;
                }

                assert!(!hit_solid, "found hollow terrain column at local ({x}, {y}, {z})");
            }
        }
    }
}

#[test]
fn density_function_can_create_hollow_columns() {
    let terrain = TerrainConfig {
        density_3d: Density3DConfig {
            scale: 0.04,
            octaves: 3,
            persistence: 0.6,
            lacunarity: 2.0,
            weight: 28.0,
        },
        ..TerrainConfig::default()
    };
    let generator = StagedGenerator::new(
        12345,
        BlockId(4),
        terrain,
        BiomeTable::single(BlockId(1), BlockId(2), BlockId(3)),
    );
    let column = ColumnSample {
        surface_height: 64.0,
        surface_block: BlockId(1),
        soil_block: BlockId(2),
        deep_block: BlockId(3),
        ocean_floor_block: BlockId(2),
        grass_color: [255, 255, 255],
        foliage_color: [255, 255, 255],
    };
    let mut found_hollow = false;

    for sample_z in (-64..=64).step_by(8) {
        for sample_x in (-64..=64).step_by(8) {
            let mut hit_solid = false;
            let mut hit_air_after_solid = false;

            for sample_y in (24..=104).rev() {
                let solid = generator.is_solid(column, sample_x, sample_y, sample_z);

                if solid {
                    if hit_air_after_solid {
                        found_hollow = true;
                        break;
                    }

                    hit_solid = true;
                } else if hit_solid {
                    hit_air_after_solid = true;
                }
            }

            if found_hollow {
                break;
            }
        }

        if found_hollow {
            break;
        }
    }

    assert!(found_hollow, "expected 3D density to create at least one hollow column");
}

#[test]
fn staged_generator_creates_large_elevation_range() {
    let generator = generator(12345);
    let mut min_height = f32::INFINITY;
    let mut max_height = f32::NEG_INFINITY;

    for sample_z in (-4096..=4096).step_by(256) {
        for sample_x in (-4096..=4096).step_by(256) {
            let height = generator.sample_column(sample_x, sample_z).surface_height;
            min_height = min_height.min(height);
            max_height = max_height.max(height);
        }
    }

    assert!(
        max_height - min_height > 90.0,
        "expected a large terrain range, got {}",
        max_height - min_height
    );
}

#[test]
fn rivers_carve_down_some_land_columns() {
    let with_rivers = generator_without_density(12345);
    let without_rivers = generator_without_rivers(12345);
    let mut deepest_carve = 0.0f32;

    for sample_z in (-2048..=2048).step_by(96) {
        for sample_x in (-2048..=2048).step_by(96) {
            let with_river_height = with_rivers.sample_column(sample_x, sample_z).surface_height;
            let without_river_height =
                without_rivers.sample_column(sample_x, sample_z).surface_height;
            deepest_carve = deepest_carve.max(without_river_height - with_river_height);
        }
    }

    assert!(
        deepest_carve > 6.0,
        "expected rivers to carve down terrain noticeably, max carve was {deepest_carve}"
    );
}

#[test]
fn staged_generator_samples_multiple_continental_regions() {
    let generator = generator(12345);
    let mut base_heights = std::collections::BTreeSet::new();

    for sample_z in (-4096..=4096).step_by(512) {
        for sample_x in (-4096..=4096).step_by(512) {
            let blueprint = generator.sample_blueprint(sample_x, sample_z);
            base_heights.insert(blueprint.base_height.round() as i32);
        }
    }

    assert!(
        base_heights.len() >= 4,
        "expected at least four continental regions to appear, got {:?}",
        base_heights
    );
}

#[test]
fn staged_generator_samples_multiple_biomes() {
    let registry = create_default_block_registry();
    let biomes = BiomeTable::load_from_file("biomes.toml", &registry)
        .expect("default biome file should load for generator tests");
    let generator = StagedGenerator::new(12345, BlockId(4), terrain(), biomes);
    let mut seen_surface_blocks = std::collections::BTreeSet::new();

    for sample_z in (-4096..=4096).step_by(512) {
        for sample_x in (-4096..=4096).step_by(512) {
            let column = generator.sample_column(sample_x, sample_z);
            seen_surface_blocks.insert(column.surface_block.0);
        }
    }

    assert!(
        seen_surface_blocks.len() >= 2,
        "expected multiple biome material choices, got {:?}",
        seen_surface_blocks
    );
}

#[test]
fn staged_generator_debug_sample_reports_biome_and_region() {
    let registry = create_default_block_registry();
    let biomes = BiomeTable::load_from_file("biomes.toml", &registry)
        .expect("default biome file should load for generator tests");
    let generator = StagedGenerator::new(12345, BlockId(4), terrain(), biomes);
    let debug_sample = generator.debug_sample_at(0, 0);

    assert!(!debug_sample.biome_name.is_empty());
    assert!(!debug_sample.region_name.is_empty());
    assert!(debug_sample.biome_priority >= 0);
    assert!(debug_sample.ground_y >= debug_sample.biome_altitude_y - 64);
    assert!(debug_sample.temperature_percent <= 100);
    assert!(debug_sample.humidity_percent <= 100);
    assert!(debug_sample.continentalness_percent <= 100);
    assert!(
        debug_sample.biome_temperature_min_percent <= debug_sample.biome_temperature_max_percent
    );
    assert!(debug_sample.biome_humidity_min_percent <= debug_sample.biome_humidity_max_percent);
    if let (Some(min), Some(max)) = (
        debug_sample.biome_continentalness_min_percent,
        debug_sample.biome_continentalness_max_percent,
    ) {
        assert!(min <= max);
    }
}

#[test]
fn river_valleys_can_drop_land_below_sea_level_and_fill_with_water() {
    let mut terrain = terrain_without_density();
    terrain.detail_amplitude = 0.0;
    terrain.mountain_peak_boost = 0.0;
    terrain.continental_regions = flat_land_regions(20.0);
    terrain.rivers = RiverConfig {
        depth: 40.0,
        valley_width: 0.10,
        bank_sharpness: 1.4,
        ..RiverConfig::default()
    };
    let generator = StagedGenerator::new(
        12345,
        BlockId(4),
        terrain,
        BiomeTable::single(BlockId(1), BlockId(2), BlockId(3)),
    );
    let mut found_flooded_river = false;

    for sample_z in (-512..=512).step_by(24) {
        for sample_x in (-512..=512).step_by(24) {
            let top_solid = generator.top_solid_y_at(sample_x, sample_z);
            let top_occupied = generator.top_occupied_y_at(sample_x, sample_z);

            if top_solid < 0 && top_occupied == 0 {
                found_flooded_river = true;
                break;
            }
        }

        if found_flooded_river {
            break;
        }
    }

    assert!(found_flooded_river, "expected a carved river valley to flood up to sea level");
}

#[test]
fn submerged_surface_uses_biome_ocean_floor_block() {
    let terrain = TerrainConfig {
        density_3d: Density3DConfig { weight: 0.0, ..Density3DConfig::default() },
        rivers: RiverConfig { depth: 0.0, ..RiverConfig::default() },
        detail_amplitude: 0.0,
        mountain_peak_boost: 0.0,
        continental_regions: flat_land_regions(-6.0),
        ..TerrainConfig::default()
    };
    let generator = StagedGenerator::new(
        12345,
        BlockId(4),
        terrain,
        BiomeTable::single_with_ocean_floor(BlockId(1), BlockId(2), BlockId(3), BlockId(8)),
    );
    let world_x = 0;
    let world_z = 0;
    let top = generator.top_solid_y_at(world_x, world_z);
    let coord = ChunkCoord::from_world_voxel(IVec3::new(world_x, top, world_z));
    let mut chunk = Chunk::new();

    generator.generate(coord, &mut chunk);

    let top_local = coord.local_voxel(IVec3::new(world_x, top, world_z));
    assert_eq!(chunk.get_local(top_local).block_id(), BlockId(8));
}

#[test]
fn shoreline_surface_without_water_above_keeps_biome_surface_block() {
    let terrain = TerrainConfig {
        density_3d: Density3DConfig { weight: 0.0, ..Density3DConfig::default() },
        rivers: RiverConfig { depth: 0.0, ..RiverConfig::default() },
        detail_amplitude: 0.0,
        mountain_peak_boost: 0.0,
        continental_regions: flat_land_regions(1.0),
        ..TerrainConfig::default()
    };
    let generator = StagedGenerator::new(
        12345,
        BlockId(4),
        terrain,
        BiomeTable::single_with_ocean_floor(BlockId(1), BlockId(2), BlockId(3), BlockId(8)),
    );
    let world_x = 0;
    let world_z = 0;
    let top = generator.top_solid_y_at(world_x, world_z);
    let coord = ChunkCoord::from_world_voxel(IVec3::new(world_x, top, world_z));
    let mut chunk = Chunk::new();

    generator.generate(coord, &mut chunk);

    let top_local = coord.local_voxel(IVec3::new(world_x, top, world_z));
    assert_eq!(chunk.get_local(top_local).block_id(), BlockId(1));
}

#[test]
fn staged_generator_uses_biome_surface_then_two_filler_layers_then_deep_block() {
    let generator = generator_without_density_or_rivers(12345);
    let world_x = 0;
    let world_z = 0;
    let top = generator.top_solid_y_at(world_x, world_z);
    let coord = ChunkCoord::from_world_voxel(IVec3::new(world_x, top, world_z));
    let mut chunk = Chunk::new();

    generator.generate(coord, &mut chunk);

    let top_local = coord.local_voxel(IVec3::new(world_x, top, world_z));
    let dirt_one_local = coord.local_voxel(IVec3::new(world_x, top - 1, world_z));
    let dirt_two_local = coord.local_voxel(IVec3::new(world_x, top - 2, world_z));
    let stone_local = coord.local_voxel(IVec3::new(world_x, top - 3, world_z));

    assert_eq!(chunk.get_local(top_local).block_id(), BlockId(1));
    assert_eq!(chunk.get_local(dirt_one_local).block_id(), BlockId(2));
    assert_eq!(chunk.get_local(dirt_two_local).block_id(), BlockId(2));
    assert_eq!(chunk.get_local(stone_local).block_id(), BlockId(3));
}

fn flat_land_regions(base_height: f32) -> ContinentalRegionsConfig {
    let mut regions = ContinentalRegionsConfig::default();
    regions.deep_ocean.base_height = base_height;
    regions.ocean.base_height = base_height;
    regions.coast.base_height = base_height;
    regions.plains.base_height = base_height;
    regions.highlands.base_height = base_height;
    regions.mountains.base_height = base_height;
    regions.deep_ocean.roughness = 0.0;
    regions.ocean.roughness = 0.0;
    regions.coast.roughness = 0.0;
    regions.plains.roughness = 0.0;
    regions.highlands.roughness = 0.0;
    regions.mountains.roughness = 0.0;
    regions
}
