mod noise;

#[cfg(test)]
mod tests;

use std::{cell::RefCell, sync::Arc, thread_local};

use crate::config::TerrainConfig;
use crate::engine::{
    core::types::{CHUNK_SIZE, CHUNK_VOLUME},
    world::{
        biome::{BiomeBlendSample, BiomeSamplePoint, BiomeTable},
        block::id::BlockId,
        chunk::{Chunk, ColumnBiomeTints, column_index, voxel_index},
        coord::ChunkCoord,
        voxel::Voxel,
    },
};

use self::noise::{
    sample_continental_region, sample_noise_3d_signed, sample_noise_signed, sample_noise01,
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
    ocean_floor_block: BlockId,
    grass_color: [u8; 3],
    foliage_color: [u8; 3],
}

#[derive(Clone, Copy, Debug, Default)]
struct BlueprintSample {
    base_height: f32,
    roughness: f32,
    mountainness: f32,
    continentalness: f32,
    region_name: &'static str,
}

#[derive(Clone, Copy, Debug, Default)]
struct ClimateSample {
    temperature: f32,
    humidity: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct SurfaceShapeSample {
    biome_altitude: f32,
    detail: f32,
    peak_signal: f32,
    river_carve: f32,
}

#[derive(Clone, Copy, Debug)]
struct SurfaceSample<'a> {
    surface_height: f32,
    biome_altitude: f32,
    biome: BiomeBlendSample<'a>,
}

#[derive(Default)]
struct StagedGeneratorScratch {
    columns: Vec<ColumnSample>,
    solid_mask: Vec<bool>,
}

impl StagedGeneratorScratch {
    fn buffers_mut(&mut self) -> (&mut [ColumnSample], &mut [bool]) {
        if self.columns.len() != COLUMN_SAMPLE_COUNT {
            self.columns.resize(COLUMN_SAMPLE_COUNT, ColumnSample::default());
        }
        if self.solid_mask.len() != CHUNK_VOLUME {
            self.solid_mask.resize(CHUNK_VOLUME, false);
        }

        (self.columns.as_mut_slice(), self.solid_mask.as_mut_slice())
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
    pub biome_priority: i32,
    pub region_name: &'static str,
    pub ground_y: i32,
    pub biome_altitude_y: i32,
    pub temperature_percent: u8,
    pub humidity_percent: u8,
    pub continentalness_percent: u8,
    pub biome_temperature_min_percent: u8,
    pub biome_temperature_max_percent: u8,
    pub biome_humidity_min_percent: u8,
    pub biome_humidity_max_percent: u8,
    pub biome_altitude_min: Option<i32>,
    pub biome_altitude_max: Option<i32>,
    pub biome_continentalness_min_percent: Option<u8>,
    pub biome_continentalness_max_percent: Option<u8>,
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
        let origin = coord.world_origin().as_ivec3();
        let (columns, solid_mask) = scratch.buffers_mut();
        let (voxels, column_biome_tints) = chunk.storage_mut();
        self.prepare_columns(origin.x, origin.z, columns);
        self.populate_solid_mask(origin.x, origin.y, origin.z, columns, solid_mask);

        for local_z in 0..CHUNK_SIZE {
            for local_x in 0..CHUNK_SIZE {
                let column = columns[column_index(local_x, local_z)];
                let dirt_depth = self.terrain.dirt_depth as i32;
                let world_x = origin.x + local_x as i32;
                let world_z = origin.z + local_z as i32;
                let surface_material_min_y = column.surface_height.floor() as i32 - dirt_depth;
                let mut material_depth = None;
                column_biome_tints[column_index(local_x, local_z)] =
                    ColumnBiomeTints { grass: column.grass_color, foliage: column.foliage_color };

                for local_y in (0..CHUNK_SIZE).rev() {
                    let world_y = origin.y + local_y as i32;
                    let voxel = if solid_mask[voxel_index(local_x, local_y, local_z)] {
                        let solid_above = if local_y + 1 < CHUNK_SIZE {
                            solid_mask[voxel_index(local_x, local_y + 1, local_z)]
                        } else {
                            self.is_solid(column, world_x, world_y + 1, world_z)
                        };
                        let top_exposed = !solid_above;
                        let submerged_surface = top_exposed && world_y < self.terrain.sea_level;

                        if top_exposed {
                            material_depth = if submerged_surface {
                                Some(0)
                            } else {
                                (world_y >= surface_material_min_y).then_some(0)
                            };
                        }

                        let block_id = match material_depth {
                            Some(0) if submerged_surface => column.ocean_floor_block,
                            Some(0) => column.surface_block,
                            Some(depth) if depth <= dirt_depth => column.soil_block,
                            _ => column.deep_block,
                        };

                        if let Some(depth) = &mut material_depth {
                            *depth += 1;
                        }

                        Voxel::from_block_id(block_id)
                    } else if world_y <= self.terrain.sea_level {
                        material_depth = None;
                        Voxel::from_block_id(self.water_block)
                    } else {
                        material_depth = None;
                        Voxel::AIR
                    };

                    voxels[voxel_index(local_x, local_y, local_z)] = voxel;
                }
            }
        }

        chunk.bump_generation();
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

    fn populate_solid_mask(
        &self,
        origin_x: i32,
        origin_y: i32,
        origin_z: i32,
        columns: &[ColumnSample],
        solid_mask: &mut [bool],
    ) {
        debug_assert_eq!(columns.len(), COLUMN_SAMPLE_COUNT);
        debug_assert_eq!(solid_mask.len(), CHUNK_VOLUME);

        for local_z in 0..CHUNK_SIZE {
            for local_x in 0..CHUNK_SIZE {
                let column = columns[column_index(local_x, local_z)];
                let world_x = origin_x + local_x as i32;
                let world_z = origin_z + local_z as i32;

                for local_y in 0..CHUNK_SIZE {
                    let world_y = origin_y + local_y as i32;
                    solid_mask[voxel_index(local_x, local_y, local_z)] =
                        self.is_solid(column, world_x, world_y, world_z);
                }
            }
        }
    }

    fn sample_column(&self, world_x: i32, world_z: i32) -> ColumnSample {
        let blueprint = self.sample_blueprint(world_x, world_z);
        let climate = self.sample_climate(world_x, world_z);
        let surface = self.sample_surface(world_x, world_z, blueprint, climate);

        ColumnSample {
            surface_height: surface.surface_height,
            surface_block: surface.biome.dominant.surface_block,
            soil_block: surface.biome.dominant.soil_block,
            deep_block: surface.biome.dominant.deep_block,
            ocean_floor_block: surface.biome.dominant.ocean_floor_block,
            grass_color: surface.biome.dominant.grass_color,
            foliage_color: surface.biome.dominant.foliage_color,
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

    fn sample_density(
        &self,
        column: ColumnSample,
        world_x: i32,
        world_y: i32,
        world_z: i32,
    ) -> f32 {
        let density_noise = sample_noise_3d_signed(
            self.seed ^ 0xD3A5_1EEDu64,
            world_x as f32,
            world_y as f32,
            world_z as f32,
            self.terrain.density_3d,
        );

        (column.surface_height - world_y as f32) + density_noise * self.terrain.density_3d.weight
    }

    fn is_solid(&self, column: ColumnSample, world_x: i32, world_y: i32, world_z: i32) -> bool {
        self.sample_density(column, world_x, world_y, world_z) > 0.0
    }

    fn top_solid_y_for_column(&self, column: ColumnSample, world_x: i32, world_z: i32) -> i32 {
        let density_weight = self.terrain.density_3d.weight.abs();
        let max_y = (column.surface_height + density_weight + 1.0).ceil() as i32;
        let min_y = (column.surface_height - density_weight - 2.0).floor() as i32;

        for world_y in (min_y..=max_y).rev() {
            if self.is_solid(column, world_x, world_y, world_z) {
                return world_y;
            }
        }

        min_y
    }

    fn sample_surface(
        &self,
        world_x: i32,
        world_z: i32,
        blueprint: BlueprintSample,
        climate: ClimateSample,
    ) -> SurfaceSample<'_> {
        let surface_shape = self.sample_surface_shape(world_x, world_z, blueprint);
        let biome = self.biomes.sample_blended(
            BiomeSamplePoint {
                temperature: climate.temperature,
                humidity: climate.humidity,
                altitude: surface_shape.biome_altitude,
                continentalness: blueprint.continentalness,
            },
            self.terrain.biome_blend,
        );
        let signed_relief = surface_shape.detail
            * self.terrain.detail_amplitude
            * blueprint.roughness
            * biome.roughness_multiplier;
        let mountain_peak_boost = surface_shape.peak_signal
            * self.terrain.mountain_peak_boost
            * blueprint.mountainness
            * biome.roughness_multiplier;
        let surface_height = self.terrain.sea_level as f32
            + blueprint.base_height
            + biome.height_offset
            + signed_relief
            + mountain_peak_boost
            - surface_shape.river_carve;

        SurfaceSample { surface_height, biome_altitude: surface_shape.biome_altitude, biome }
    }

    fn sample_surface_shape(
        &self,
        world_x: i32,
        world_z: i32,
        blueprint: BlueprintSample,
    ) -> SurfaceShapeSample {
        let detail = self.sample_detail(world_x, world_z);
        let peak_signal = detail.max(0.0).powf(self.terrain.mountain_peak_sharpness.max(1.0));
        let biome_altitude = self.terrain.sea_level as f32
            + blueprint.base_height
            + detail * self.terrain.detail_amplitude * blueprint.roughness
            + peak_signal * self.terrain.mountain_peak_boost * blueprint.mountainness;
        let river_carve = self.sample_river_carve(world_x, world_z, biome_altitude);

        SurfaceShapeSample { biome_altitude, detail, peak_signal, river_carve }
    }

    fn sample_river_carve(&self, world_x: i32, world_z: i32, biome_altitude: f32) -> f32 {
        let rivers = self.terrain.rivers;
        if rivers.depth <= f32::EPSILON || rivers.valley_width <= f32::EPSILON {
            return 0.0;
        }

        let river_noise =
            sample_noise_signed(self.seed ^ 0xA11C_E551u64, world_x, world_z, rivers.noise);
        let ridge_distance = river_noise.abs().clamp(0.0, 1.0);
        let valley_mask =
            1.0 - (ridge_distance / rivers.valley_width.max(f32::EPSILON)).clamp(0.0, 1.0);
        let valley_depth = valley_mask.powf(rivers.bank_sharpness.max(1.0)) * rivers.depth;
        let land_mask = smoothstep(
            self.terrain.sea_level as f32 - rivers.depth * 0.25,
            self.terrain.sea_level as f32 + rivers.depth * 0.5,
            biome_altitude,
        );

        valley_depth * land_mask
    }

    pub fn top_solid_y_at(&self, world_x: i32, world_z: i32) -> i32 {
        let column = self.sample_column(world_x, world_z);
        self.top_solid_y_for_column(column, world_x, world_z)
    }

    pub fn top_occupied_y_at(&self, world_x: i32, world_z: i32) -> i32 {
        self.top_solid_y_at(world_x, world_z).max(self.terrain.sea_level)
    }

    pub fn debug_sample_at(&self, world_x: i32, world_z: i32) -> TerrainDebugSample {
        let blueprint = self.sample_blueprint(world_x, world_z);
        let climate = self.sample_climate(world_x, world_z);
        let surface = self.sample_surface(world_x, world_z, blueprint, climate);
        let column = ColumnSample {
            surface_height: surface.surface_height,
            surface_block: surface.biome.dominant.surface_block,
            soil_block: surface.biome.dominant.soil_block,
            deep_block: surface.biome.dominant.deep_block,
            ocean_floor_block: surface.biome.dominant.ocean_floor_block,
            grass_color: surface.biome.dominant.grass_color,
            foliage_color: surface.biome.dominant.foliage_color,
        };
        let dominant_biome = surface.biome.dominant;
        let (biome_temperature_min, biome_temperature_max) = dominant_biome.temperature_range();
        let (biome_humidity_min, biome_humidity_max) = dominant_biome.humidity_range();
        let (biome_altitude_min, biome_altitude_max) = dominant_biome.altitude_range();
        let (biome_continentalness_min, biome_continentalness_max) =
            dominant_biome.continentalness_range();

        TerrainDebugSample {
            biome_name: dominant_biome.name.clone(),
            biome_priority: dominant_biome.priority(),
            region_name: blueprint.region_name,
            ground_y: self.top_solid_y_for_column(column, world_x, world_z),
            biome_altitude_y: surface.biome_altitude.round() as i32,
            temperature_percent: unit_to_percent(climate.temperature),
            humidity_percent: unit_to_percent(climate.humidity),
            continentalness_percent: unit_to_percent(blueprint.continentalness),
            biome_temperature_min_percent: unit_to_percent(biome_temperature_min),
            biome_temperature_max_percent: unit_to_percent(biome_temperature_max),
            biome_humidity_min_percent: unit_to_percent(biome_humidity_min),
            biome_humidity_max_percent: unit_to_percent(biome_humidity_max),
            biome_altitude_min: biome_altitude_min.map(|value| value.round() as i32),
            biome_altitude_max: biome_altitude_max.map(|value| value.round() as i32),
            biome_continentalness_min_percent: biome_continentalness_min.map(unit_to_percent),
            biome_continentalness_max_percent: biome_continentalness_max.map(unit_to_percent),
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
fn contrast_unit(value: f32, contrast: f32) -> f32 {
    ((value - 0.5) * contrast + 0.5).clamp(0.0, 1.0)
}

#[inline]
fn unit_to_percent(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 100.0).round() as u8
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    if (edge1 - edge0).abs() <= f32::EPSILON {
        return (value >= edge1) as u8 as f32;
    }

    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
