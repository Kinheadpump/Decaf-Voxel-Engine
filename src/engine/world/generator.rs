use std::{cell::RefCell, sync::Arc, thread_local};

use crate::config::{
    ContinentalRegionConfig, ContinentalRegionsConfig, NoiseConfig, TerrainConfig,
};
use crate::engine::{
    core::types::CHUNK_SIZE,
    world::{
        biome::{BiomeBlendSample, BiomeTable},
        block::id::BlockId,
        chunk::{Chunk, voxel_index},
        coord::ChunkCoord,
        voxel::Voxel,
    },
};

pub trait ChunkGenerator: Send + Sync {
    fn generate(&self, coord: ChunkCoord, chunk: &mut Chunk);
}

const COLUMN_STRIDE: usize = CHUNK_SIZE;
const COLUMN_SAMPLE_COUNT: usize = COLUMN_STRIDE * COLUMN_STRIDE;

#[derive(Clone, Copy, Debug, Default)]
struct ColumnSample {
    surface_height: f32,
    surface_block: BlockId,
    soil_block: BlockId,
    deep_block: BlockId,
}

#[derive(Clone, Copy, Debug, Default)]
struct BlueprintSample {
    base_height: f32,
    roughness: f32,
    mountainness: f32,
    region_name: &'static str,
}

#[derive(Clone, Copy, Debug, Default)]
struct ClimateSample {
    temperature: f32,
    humidity: f32,
}

#[derive(Default)]
struct StagedGeneratorScratch {
    columns: Vec<ColumnSample>,
}

impl StagedGeneratorScratch {
    fn columns_mut(&mut self) -> &mut [ColumnSample] {
        if self.columns.len() != COLUMN_SAMPLE_COUNT {
            self.columns.resize(COLUMN_SAMPLE_COUNT, ColumnSample::default());
        }

        &mut self.columns
    }
}

thread_local! {
    static STAGED_GENERATOR_SCRATCH: RefCell<StagedGeneratorScratch> =
        RefCell::new(StagedGeneratorScratch::default());
}

#[derive(Clone, Debug)]
pub struct StagedGenerator {
    pub seed: u64,
    pub water_block: BlockId,
    pub terrain: TerrainConfig,
    biomes: BiomeTable,
}

#[derive(Clone, Debug)]
pub struct TerrainDebugSample {
    pub biome_name: Arc<str>,
    pub region_name: &'static str,
    pub surface_y: i32,
    pub temperature_percent: u8,
    pub humidity_percent: u8,
}

impl StagedGenerator {
    pub fn new(
        seed: u64,
        water_block: BlockId,
        terrain: TerrainConfig,
        biomes: BiomeTable,
    ) -> Self {
        Self { seed, water_block, terrain, biomes }
    }

    fn generate_with_scratch(
        &self,
        coord: ChunkCoord,
        chunk: &mut Chunk,
        scratch: &mut StagedGeneratorScratch,
    ) {
        let origin = coord.world_origin();
        let columns = scratch.columns_mut();
        self.prepare_columns(origin.x, origin.z, columns);

        for local_z in 0..CHUNK_SIZE {
            for local_x in 0..CHUNK_SIZE {
                let column = columns[column_index(local_x, local_z)];
                let top_solid_y = column.surface_height.floor() as i32;
                let dirt_depth = self.terrain.dirt_depth as i32;

                for local_y in (0..CHUNK_SIZE).rev() {
                    let world_y = origin.y + local_y as i32;
                    let voxel = if world_y <= top_solid_y {
                        let depth_below_surface = top_solid_y - world_y;

                        if depth_below_surface == 0 {
                            Voxel::from_block_id(column.surface_block)
                        } else if depth_below_surface <= dirt_depth {
                            Voxel::from_block_id(column.soil_block)
                        } else {
                            Voxel::from_block_id(column.deep_block)
                        }
                    } else if world_y <= self.terrain.sea_level {
                        Voxel::from_block_id(self.water_block)
                    } else {
                        Voxel::AIR
                    };

                    chunk.voxels[voxel_index(local_x, local_y, local_z)] = voxel;
                }
            }
        }

        chunk.dirty = true;
        chunk.generation = chunk.generation.wrapping_add(1);
    }

    fn prepare_columns(&self, origin_x: i32, origin_z: i32, columns: &mut [ColumnSample]) {
        debug_assert_eq!(columns.len(), COLUMN_SAMPLE_COUNT);

        for local_z in 0..CHUNK_SIZE {
            for local_x in 0..CHUNK_SIZE {
                let world_x = origin_x + local_x as i32;
                let world_z = origin_z + local_z as i32;
                columns[column_index(local_x, local_z)] = self.sample_column(world_x, world_z);
            }
        }
    }

    fn sample_column(&self, world_x: i32, world_z: i32) -> ColumnSample {
        let blueprint = self.sample_blueprint(world_x, world_z);
        let climate = self.sample_climate(world_x, world_z);
        let (surface_height, biome) =
            self.sample_surface_height(world_x, world_z, blueprint, climate);

        ColumnSample {
            surface_height,
            surface_block: biome.dominant.surface_block,
            soil_block: biome.dominant.soil_block,
            deep_block: biome.dominant.deep_block,
        }
    }

    fn sample_blueprint(&self, world_x: i32, world_z: i32) -> BlueprintSample {
        let continentalness = contrast_unit(
            sample_noise01(
                self.seed ^ 0xC017_1E17u64,
                world_x,
                world_z,
                self.terrain.continentalness,
            ),
            self.terrain.continentalness_contrast,
        );
        sample_continental_region(self.terrain, self.terrain.continental_regions, continentalness)
    }

    fn sample_climate(&self, world_x: i32, world_z: i32) -> ClimateSample {
        ClimateSample {
            temperature: contrast_unit(
                sample_noise01(
                    self.seed ^ 0x71CE_5511u64,
                    world_x,
                    world_z,
                    self.terrain.temperature,
                ),
                self.terrain.climate_contrast,
            ),
            humidity: contrast_unit(
                sample_noise01(self.seed ^ 0x35F0_12D1u64, world_x, world_z, self.terrain.humidity),
                self.terrain.climate_contrast,
            ),
        }
    }

    fn sample_detail(&self, world_x: i32, world_z: i32) -> f32 {
        sample_noise_signed(self.seed ^ 0xDE7A_11C1u64, world_x, world_z, self.terrain.detail)
    }

    fn sample_surface_height(
        &self,
        world_x: i32,
        world_z: i32,
        blueprint: BlueprintSample,
        climate: ClimateSample,
    ) -> (f32, BiomeBlendSample<'_>) {
        let biome = self.biomes.sample_blended(
            climate.temperature,
            climate.humidity,
            self.terrain.biome_blend,
        );
        let detail = self.sample_detail(world_x, world_z);
        // Keep hills signed so valleys still exist, but add a positive-only mountain term so
        // high-roughness regions can form tall peaks instead of just deeper rolling noise.
        let signed_relief = detail
            * self.terrain.detail_amplitude
            * blueprint.roughness
            * biome.roughness_multiplier;
        let peak_signal = detail.max(0.0).powf(self.terrain.mountain_peak_sharpness.max(1.0));
        let mountain_peak_boost = peak_signal
            * self.terrain.mountain_peak_boost
            * blueprint.mountainness
            * biome.roughness_multiplier;
        let surface_height = self.terrain.sea_level as f32
            + blueprint.base_height
            + biome.height_offset
            + signed_relief
            + mountain_peak_boost;

        (surface_height, biome)
    }

    pub fn top_solid_y_at(&self, world_x: i32, world_z: i32) -> i32 {
        self.sample_column(world_x, world_z).surface_height.floor() as i32
    }

    pub fn top_occupied_y_at(&self, world_x: i32, world_z: i32) -> i32 {
        self.top_solid_y_at(world_x, world_z).max(self.terrain.sea_level)
    }

    pub fn debug_sample_at(&self, world_x: i32, world_z: i32) -> TerrainDebugSample {
        let blueprint = self.sample_blueprint(world_x, world_z);
        let climate = self.sample_climate(world_x, world_z);
        let (surface_height, biome) =
            self.sample_surface_height(world_x, world_z, blueprint, climate);

        TerrainDebugSample {
            biome_name: biome.dominant.name.clone(),
            region_name: blueprint.region_name,
            surface_y: surface_height.floor() as i32,
            temperature_percent: unit_to_percent(climate.temperature),
            humidity_percent: unit_to_percent(climate.humidity),
        }
    }
}

impl ChunkGenerator for StagedGenerator {
    fn generate(&self, coord: ChunkCoord, chunk: &mut Chunk) {
        STAGED_GENERATOR_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            self.generate_with_scratch(coord, chunk, &mut scratch);
        });
    }
}

#[inline]
fn column_index(x: usize, z: usize) -> usize {
    x + z * COLUMN_STRIDE
}

#[inline]
fn sample_noise_signed(seed: u64, world_x: i32, world_z: i32, config: NoiseConfig) -> f32 {
    let x = world_x as f32 * config.scale;
    let z = world_z as f32 * config.scale;
    fbm_perlin_2d(seed, x, z, config.octaves, config.persistence, config.lacunarity)
}

#[inline]
fn sample_noise01(seed: u64, world_x: i32, world_z: i32, config: NoiseConfig) -> f32 {
    remap01(sample_noise_signed(seed, world_x, world_z, config))
}

fn sample_continental_region(
    terrain: TerrainConfig,
    regions: ContinentalRegionsConfig,
    continentalness: f32,
) -> BlueprintSample {
    let sample = continentalness.clamp(0.0, 1.0);
    let ordered = [
        ("DEEP OCEAN", sanitize_continental_region(regions.deep_ocean)),
        ("OCEAN", sanitize_continental_region(regions.ocean)),
        ("COAST", sanitize_continental_region(regions.coast)),
        ("PLAINS", sanitize_continental_region(regions.plains)),
        ("HIGHLANDS", sanitize_continental_region(regions.highlands)),
        ("MOUNTAINS", sanitize_continental_region(regions.mountains)),
    ];

    let mut min_value = 0.0;
    let mut control_points = [SplinePoint::default(); 6];
    for (index, (region_name, region)) in ordered.into_iter().enumerate() {
        // Using region midpoints as spline knots avoids the hard "snap" that happens when the
        // continentalness map is treated as a discrete enum instead of a continuous control curve.
        control_points[index] =
            SplinePoint { center: (min_value + region.max_value) * 0.5, name: region_name, region };
        min_value = region.max_value;
    }

    if sample <= control_points[0].center {
        let region = control_points[0].region;
        return BlueprintSample {
            base_height: region.base_height,
            roughness: region.roughness,
            mountainness: mountainness_from_roughness(
                terrain,
                control_points[control_points.len() - 1].region.roughness,
                region.roughness,
            ),
            region_name: control_points[0].name,
        };
    }

    for window in control_points.windows(2) {
        let left = window[0];
        let right = window[1];

        if sample <= right.center {
            let t = smoothstep_range(left.center, right.center, sample);
            let region = blend_continental_region(left.region, right.region, t);
            let dominant_name = if t < 0.5 { left.name } else { right.name };
            return BlueprintSample {
                base_height: region.base_height,
                roughness: region.roughness,
                mountainness: mountainness_from_roughness(
                    terrain,
                    control_points[control_points.len() - 1].region.roughness,
                    region.roughness,
                ),
                region_name: dominant_name,
            };
        }
    }

    let last = control_points[control_points.len() - 1];
    BlueprintSample {
        base_height: last.region.base_height,
        roughness: last.region.roughness,
        mountainness: mountainness_from_roughness(
            terrain,
            last.region.roughness,
            last.region.roughness,
        ),
        region_name: last.name,
    }
}

#[inline]
fn sanitize_continental_region(region: ContinentalRegionConfig) -> ContinentalRegionConfig {
    ContinentalRegionConfig {
        max_value: region.max_value.clamp(0.0, 1.0),
        base_height: region.base_height,
        roughness: region.roughness.max(0.0),
    }
}

fn fbm_perlin_2d(
    seed: u64,
    x: f32,
    z: f32,
    octaves: u32,
    persistence: f32,
    lacunarity: f32,
) -> f32 {
    let octaves = octaves.max(1);
    let persistence = persistence.clamp(0.0, 1.0);
    let lacunarity = lacunarity.max(1.0);
    let mut total = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    let mut normalization = 0.0;

    for octave in 0..octaves {
        total += perlin_2d(
            seed.wrapping_add((octave as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)),
            x * frequency,
            z * frequency,
        ) * amplitude;
        normalization += amplitude;
        amplitude *= persistence;
        frequency *= lacunarity;
    }

    total / normalization.max(f32::EPSILON)
}

fn perlin_2d(seed: u64, x: f32, z: f32) -> f32 {
    let x0 = x.floor() as i32;
    let z0 = z.floor() as i32;
    let x1 = x0 + 1;
    let z1 = z0 + 1;
    let tx = x - x0 as f32;
    let tz = z - z0 as f32;
    let u = fade(tx);
    let v = fade(tz);

    let v00 = gradient_dot(hash_2d(seed, x0, z0), tx, tz);
    let v10 = gradient_dot(hash_2d(seed, x1, z0), tx - 1.0, tz);
    let v01 = gradient_dot(hash_2d(seed, x0, z1), tx, tz - 1.0);
    let v11 = gradient_dot(hash_2d(seed, x1, z1), tx - 1.0, tz - 1.0);

    lerp(lerp(v00, v10, u), lerp(v01, v11, u), v)
}

#[inline]
fn gradient_dot(hash: u64, x: f32, z: f32) -> f32 {
    const DIAGONAL: f32 = 0.70710677;

    match hash & 7 {
        0 => x,
        1 => -x,
        2 => z,
        3 => -z,
        4 => (x + z) * DIAGONAL,
        5 => (x - z) * DIAGONAL,
        6 => (-x + z) * DIAGONAL,
        _ => (-x - z) * DIAGONAL,
    }
}

#[inline]
fn fade(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn remap01(value: f32) -> f32 {
    value.mul_add(0.5, 0.5).clamp(0.0, 1.0)
}

#[inline]
fn contrast_unit(value: f32, contrast: f32) -> f32 {
    ((value - 0.5) * contrast + 0.5).clamp(0.0, 1.0)
}

#[inline]
fn smoothstep_range(edge0: f32, edge1: f32, value: f32) -> f32 {
    if (edge1 - edge0).abs() <= f32::EPSILON {
        return (value >= edge1) as u8 as f32;
    }

    fade(((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0))
}

#[inline]
fn unit_to_percent(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 100.0).round() as u8
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn hash_2d(seed: u64, x: i32, z: i32) -> u64 {
    mix64(
        seed ^ (x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (z as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F),
    )
}

#[inline]
fn mix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

#[derive(Clone, Copy, Debug, Default)]
struct SplinePoint {
    center: f32,
    name: &'static str,
    region: ContinentalRegionConfig,
}

fn blend_continental_region(
    left: ContinentalRegionConfig,
    right: ContinentalRegionConfig,
    t: f32,
) -> ContinentalRegionConfig {
    ContinentalRegionConfig {
        max_value: lerp(left.max_value, right.max_value, t),
        base_height: lerp(left.base_height, right.base_height, t),
        roughness: lerp(left.roughness, right.roughness, t),
    }
}

fn mountainness_from_roughness(terrain: TerrainConfig, max_roughness: f32, roughness: f32) -> f32 {
    smoothstep_range(terrain.mountain_start_roughness, max_roughness, roughness)
}

#[cfg(test)]
mod tests {
    use glam::IVec3;

    use super::*;
    use crate::engine::world::{biome::BiomeTable, block::create_default_block_registry};

    fn terrain() -> TerrainConfig {
        TerrainConfig::default()
    }

    fn generator(seed: u64) -> StagedGenerator {
        StagedGenerator::new(
            seed,
            BlockId(4),
            terrain(),
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
            .map(|x| first.sample_column(x, x / 2).surface_height.round() as i32)
            .collect();
        let second_heights: Vec<_> = (-128..=128)
            .step_by(32)
            .map(|x| second.sample_column(x, x / 2).surface_height.round() as i32)
            .collect();

        assert_ne!(first_heights, second_heights);
    }

    #[test]
    fn staged_generator_produces_surface_and_air_in_spawn_chunk() {
        let generator = generator(12345);
        let surface_y = generator.top_solid_y_at(0, 0);
        let coord = ChunkCoord::from_world_voxel(IVec3::new(0, surface_y, 0));
        let mut chunk = Chunk::new();

        generator.generate(coord, &mut chunk);

        let has_air = chunk.voxels.contains(&Voxel::AIR);
        let has_surface = chunk
            .voxels
            .iter()
            .any(|voxel| voxel.block_id() == BlockId(1) || voxel.block_id() == BlockId(2));
        let has_ground = chunk
            .voxels
            .iter()
            .any(|voxel| voxel.block_id() != BlockId::AIR && voxel.block_id() != BlockId(4));

        assert!(has_air);
        assert!(has_surface);
        assert!(has_ground);
    }

    #[test]
    fn staged_generator_has_no_hollow_columns() {
        let generator = generator(12345);
        let mut chunk = Chunk::new();
        let coord = ChunkCoord(IVec3::ZERO);

        generator.generate(coord, &mut chunk);

        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let mut hit_solid = false;

                for y in (0..CHUNK_SIZE).rev() {
                    let voxel = chunk.get(x, y, z);
                    let is_solid =
                        voxel.block_id() != BlockId::AIR && voxel.block_id() != BlockId(4);

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
    fn staged_generator_creates_large_elevation_range() {
        let generator = generator(12345);
        let mut min_height = f32::INFINITY;
        let mut max_height = f32::NEG_INFINITY;

        for world_z in (-4096..=4096).step_by(256) {
            for world_x in (-4096..=4096).step_by(256) {
                let height = generator.sample_column(world_x, world_z).surface_height;
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
    fn staged_generator_samples_multiple_continental_regions() {
        let generator = generator(12345);
        let mut base_heights = std::collections::BTreeSet::new();

        for world_z in (-4096..=4096).step_by(512) {
            for world_x in (-4096..=4096).step_by(512) {
                let blueprint = generator.sample_blueprint(world_x, world_z);
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

        for world_z in (-4096..=4096).step_by(512) {
            for world_x in (-4096..=4096).step_by(512) {
                let column = generator.sample_column(world_x, world_z);
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
        assert!(debug_sample.temperature_percent <= 100);
        assert!(debug_sample.humidity_percent <= 100);
    }

    #[test]
    fn staged_generator_uses_biome_surface_then_two_filler_layers_then_deep_block() {
        let generator = generator(12345);
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

        assert_eq!(
            chunk.get(top_local.x as usize, top_local.y as usize, top_local.z as usize).block_id(),
            BlockId(1)
        );
        assert_eq!(
            chunk
                .get(
                    dirt_one_local.x as usize,
                    dirt_one_local.y as usize,
                    dirt_one_local.z as usize,
                )
                .block_id(),
            BlockId(2)
        );
        assert_eq!(
            chunk
                .get(
                    dirt_two_local.x as usize,
                    dirt_two_local.y as usize,
                    dirt_two_local.z as usize,
                )
                .block_id(),
            BlockId(2)
        );
        assert_eq!(
            chunk
                .get(stone_local.x as usize, stone_local.y as usize, stone_local.z as usize,)
                .block_id(),
            BlockId(3)
        );
    }
}
