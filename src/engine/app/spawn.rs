use crate::{
    config::{PlayerConfig, WorldConfig},
    engine::{
        core::{math::Vec3, types::CHUNK_SIZE_I32},
        world::generator::StagedGenerator,
    },
};

const MAX_SPAWN_STEP_CHUNKS: i32 = 3;

pub(super) fn spawn_position_near_world_origin(
    generator: &StagedGenerator,
    world_config: &WorldConfig,
    player_config: &PlayerConfig,
) -> Vec3 {
    let (spawn_x, spawn_z) = find_non_water_spawn_target(
        generator.seed,
        world_config.spawn_search_attempts,
        |candidate_x, candidate_z| {
            spawn_surface_sample(generator, player_config, candidate_x as f32, candidate_z as f32)
                .surface_is_water
        },
    )
    .unwrap_or((0, 0));

    let spawn_surface =
        spawn_surface_sample(generator, player_config, spawn_x as f32, spawn_z as f32);

    Vec3::new(spawn_x as f32, spawn_surface.support_top as f32 + 1.0, spawn_z as f32)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SpawnSurfaceSample {
    support_top: i32,
    surface_is_water: bool,
}

fn spawn_surface_sample(
    generator: &StagedGenerator,
    player_config: &PlayerConfig,
    spawn_x: f32,
    spawn_z: f32,
) -> SpawnSurfaceSample {
    let min_x = (spawn_x - player_config.radius).floor() as i32;
    let max_x = (spawn_x + player_config.radius).floor() as i32;
    let min_z = (spawn_z - player_config.radius).floor() as i32;
    let max_z = (spawn_z + player_config.radius).floor() as i32;
    let mut support_top = i32::MIN;
    let mut surface_is_water = false;

    for world_z in min_z..=max_z {
        for world_x in min_x..=max_x {
            let top_solid = generator.top_solid_y_at(world_x, world_z);
            let top_occupied = generator.top_occupied_y_at(world_x, world_z);

            support_top = support_top.max(top_solid);
            surface_is_water |= top_occupied > top_solid;
        }
    }

    if support_top == i32::MIN {
        support_top = generator.terrain.sea_level;
    }

    SpawnSurfaceSample { support_top, surface_is_water }
}

fn find_non_water_spawn_target(
    seed: u64,
    search_attempts: u32,
    mut is_surface_water: impl FnMut(i32, i32) -> bool,
) -> Option<(i32, i32)> {
    if !is_surface_water(0, 0) {
        return Some((0, 0));
    }

    let mut rng = SpawnSearchRng::new(seed);
    let mut current_x = 0;
    let mut current_z = 0;

    for _ in 0..search_attempts {
        let (chunk_dx, chunk_dz) = random_spawn_step_chunks(&mut rng);
        current_x += chunk_dx * CHUNK_SIZE_I32;
        current_z += chunk_dz * CHUNK_SIZE_I32;

        if !is_surface_water(current_x, current_z) {
            return Some((current_x, current_z));
        }
    }

    None
}

fn random_spawn_step_chunks(rng: &mut SpawnSearchRng) -> (i32, i32) {
    loop {
        let chunk_dx = rng.range_i32_inclusive(-MAX_SPAWN_STEP_CHUNKS, MAX_SPAWN_STEP_CHUNKS);
        let chunk_dz = rng.range_i32_inclusive(-MAX_SPAWN_STEP_CHUNKS, MAX_SPAWN_STEP_CHUNKS);

        if (chunk_dx != 0 || chunk_dz != 0)
            && chunk_dx * chunk_dx + chunk_dz * chunk_dz
                <= MAX_SPAWN_STEP_CHUNKS * MAX_SPAWN_STEP_CHUNKS
        {
            return (chunk_dx, chunk_dz);
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SpawnSearchRng {
    state: u64,
}

impl SpawnSearchRng {
    fn new(seed: u64) -> Self {
        Self { state: seed ^ 0x5EED_5A17_4F0Eu64 }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (mixed ^ (mixed >> 31)) as u32
    }

    fn range_i32_inclusive(&mut self, min: i32, max: i32) -> i32 {
        debug_assert!(min <= max);
        let span = (max - min + 1) as u32;
        min + (self.next_u32() % span) as i32
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        config::{
            ContinentalRegionsConfig, Density3DConfig, PlayerConfig, RiverConfig, TerrainConfig,
            WorldConfig,
        },
        engine::world::{biome::BiomeTable, block::id::BlockId, generator::StagedGenerator},
    };

    use super::*;

    #[test]
    fn spawn_position_stays_at_origin_when_origin_is_dry() {
        let generator = flat_generator(12345, 18.0);
        let player_config = PlayerConfig { radius: 0.3, ..PlayerConfig::default() };
        let world_config = WorldConfig::default();

        let spawn = spawn_position_near_world_origin(&generator, &world_config, &player_config);
        let expected_support = generator.top_solid_y_at(0, 0);

        assert_eq!(spawn.x, 0.0);
        assert_eq!(spawn.z, 0.0);
        assert_eq!(spawn.y, expected_support as f32 + 1.0);
    }

    #[test]
    fn spawn_search_finds_non_water_target_when_origin_is_wet() {
        let target = find_non_water_spawn_target(12345, 10, |x, z| x == 0 && z == 0);

        assert!(target.is_some());
        assert_ne!(target, Some((0, 0)));
    }

    #[test]
    fn spawn_search_falls_back_to_origin_when_every_attempt_is_water() {
        let target = find_non_water_spawn_target(12345, 10, |_x, _z| true);

        assert_eq!(target, None);
    }

    #[test]
    fn fallback_origin_uses_solid_support_instead_of_water_surface() {
        let generator = flat_generator(12345, -12.0);
        let player_config = PlayerConfig { radius: 0.3, ..PlayerConfig::default() };
        let world_config = WorldConfig { spawn_search_attempts: 0, ..WorldConfig::default() };

        let spawn = spawn_position_near_world_origin(&generator, &world_config, &player_config);
        let expected_support = generator.top_solid_y_at(0, 0);

        assert_eq!(spawn.x, 0.0);
        assert_eq!(spawn.z, 0.0);
        assert_eq!(spawn.y, expected_support as f32 + 1.0);
    }

    fn flat_generator(seed: u64, base_height: f32) -> StagedGenerator {
        let terrain = TerrainConfig {
            density_3d: Density3DConfig { weight: 0.0, ..Density3DConfig::default() },
            rivers: RiverConfig { depth: 0.0, ..RiverConfig::default() },
            detail_amplitude: 0.0,
            mountain_peak_boost: 0.0,
            continental_regions: flat_regions(base_height),
            ..TerrainConfig::default()
        };

        StagedGenerator::new(
            seed,
            BlockId(4),
            terrain,
            BiomeTable::single(BlockId(1), BlockId(2), BlockId(3)),
        )
    }

    fn flat_regions(base_height: f32) -> ContinentalRegionsConfig {
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
}
