use crate::{
    config::PlayerConfig,
    engine::{core::math::Vec3, world::generator::StagedGenerator},
};

pub(super) fn spawn_position_at_world_origin(
    generator: &StagedGenerator,
    player_config: &PlayerConfig,
) -> Vec3 {
    let spawn_x = 0.0;
    let spawn_z = 0.0;
    let min_x = (spawn_x - player_config.radius).floor() as i32;
    let max_x = (spawn_x + player_config.radius).floor() as i32;
    let min_z = (spawn_z - player_config.radius).floor() as i32;
    let max_z = (spawn_z + player_config.radius).floor() as i32;

    let support_top = (min_z..=max_z)
        .flat_map(|world_z| {
            (min_x..=max_x).map(move |world_x| generator.top_occupied_y_at(world_x, world_z))
        })
        .max()
        .unwrap_or(generator.terrain.sea_level);

    Vec3::new(spawn_x, support_top as f32 + 1.0, spawn_z)
}

#[cfg(test)]
mod tests {
    use crate::{
        config::{PlayerConfig, TerrainConfig},
        engine::world::{biome::BiomeTable, block::id::BlockId, generator::StagedGenerator},
    };

    use super::*;

    #[test]
    fn spawn_position_sits_above_highest_column_under_player() {
        let generator = StagedGenerator::new(
            12345,
            BlockId(4),
            TerrainConfig::default(),
            BiomeTable::single(BlockId(1), BlockId(2), BlockId(3)),
        );
        let player_config = PlayerConfig { radius: 0.3, ..PlayerConfig::default() };
        let spawn = spawn_position_at_world_origin(&generator, &player_config);

        let mut expected_support = i32::MIN;
        for world_z in -1..=0 {
            for world_x in -1..=0 {
                expected_support =
                    expected_support.max(generator.top_occupied_y_at(world_x, world_z));
            }
        }

        assert_eq!(spawn.x, 0.0);
        assert_eq!(spawn.z, 0.0);
        assert_eq!(spawn.y, expected_support as f32 + 1.0);
    }
}
