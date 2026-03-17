use crate::engine::{
    core::{
        collision::aabb_intersects,
        math::{IVec3, Vec3},
    },
    player::controller::Player,
    world::{
        accessor::{VoxelAccessor, WorldVoxelReader},
        block::{id::BlockId, resolved::ResolvedBlockRegistry},
        storage::World,
    },
};

const TIE_EPSILON: f32 = 1.0e-6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockRayHit {
    pub block: IVec3,
    pub placement: Option<IVec3>,
}

pub fn place_block_in_front(
    world: &mut World,
    resolved_blocks: &ResolvedBlockRegistry,
    player: &Player,
    origin: Vec3,
    direction: Vec3,
    reach_distance: f32,
    block_id: BlockId,
) -> bool {
    let hit = {
        let accessor = VoxelAccessor { world };
        raycast_blocks(&accessor, resolved_blocks, origin, direction, reach_distance)
    };

    let Some(hit) = hit else {
        return false;
    };
    let Some(placement) = hit.placement else {
        return false;
    };

    if !can_place_block_at(world, resolved_blocks, player, placement) {
        return false;
    }

    world.set_block_world(placement, block_id)
}

pub fn remove_block_in_front(
    world: &mut World,
    resolved_blocks: &ResolvedBlockRegistry,
    origin: Vec3,
    direction: Vec3,
    reach_distance: f32,
) -> bool {
    let hit = {
        let accessor = VoxelAccessor { world };
        raycast_blocks(&accessor, resolved_blocks, origin, direction, reach_distance)
    };

    let Some(hit) = hit else {
        return false;
    };

    world.set_block_world(hit.block, BlockId::AIR)
}

pub fn raycast_blocks(
    world: &impl WorldVoxelReader,
    resolved_blocks: &ResolvedBlockRegistry,
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
) -> Option<BlockRayHit> {
    let direction = direction.normalize_or_zero();
    if direction == Vec3::ZERO || max_distance <= 0.0 {
        return None;
    }

    let step = IVec3::new(axis_step(direction.x), axis_step(direction.y), axis_step(direction.z));
    let mut voxel = origin.floor().as_ivec3();
    let mut previous_empty =
        if is_hit_block(world, resolved_blocks, voxel) { None } else { Some(voxel) };

    if previous_empty.is_none() {
        return Some(BlockRayHit { block: voxel, placement: None });
    }

    let mut t_max = Vec3::new(
        initial_t_max(origin.x, voxel.x, direction.x),
        initial_t_max(origin.y, voxel.y, direction.y),
        initial_t_max(origin.z, voxel.z, direction.z),
    );
    let t_delta =
        Vec3::new(axis_t_delta(direction.x), axis_t_delta(direction.y), axis_t_delta(direction.z));

    loop {
        let next_t = t_max.min_element();
        if !next_t.is_finite() || next_t > max_distance {
            break;
        }

        let step_x = axis_hits_boundary(t_max.x, next_t);
        let step_y = axis_hits_boundary(t_max.y, next_t);
        let step_z = axis_hits_boundary(t_max.z, next_t);

        for candidate in boundary_candidates(voxel, step, step_x, step_y, step_z) {
            if is_hit_block(world, resolved_blocks, candidate) {
                return Some(BlockRayHit { block: candidate, placement: previous_empty });
            }
        }

        if step_x {
            voxel.x += step.x;
            t_max.x += t_delta.x;
        }
        if step_y {
            voxel.y += step.y;
            t_max.y += t_delta.y;
        }
        if step_z {
            voxel.z += step.z;
            t_max.z += t_delta.z;
        }

        previous_empty = Some(voxel);
    }

    None
}

fn is_hit_block(
    world: &impl WorldVoxelReader,
    resolved_blocks: &ResolvedBlockRegistry,
    voxel: IVec3,
) -> bool {
    !resolved_blocks.get_voxel(world.get_world_voxel(voxel)).is_air()
}

fn axis_step(component: f32) -> i32 {
    if component > 0.0 {
        1
    } else if component < 0.0 {
        -1
    } else {
        0
    }
}

fn initial_t_max(origin_component: f32, voxel_component: i32, direction_component: f32) -> f32 {
    if direction_component > 0.0 {
        ((voxel_component + 1) as f32 - origin_component) / direction_component
    } else if direction_component < 0.0 {
        (origin_component - voxel_component as f32) / -direction_component
    } else {
        f32::INFINITY
    }
}

fn axis_t_delta(direction_component: f32) -> f32 {
    if direction_component == 0.0 { f32::INFINITY } else { 1.0 / direction_component.abs() }
}

fn axis_hits_boundary(t_axis: f32, next_t: f32) -> bool {
    (t_axis - next_t).abs() <= TIE_EPSILON
}

fn boundary_candidates(
    voxel: IVec3,
    step: IVec3,
    step_x: bool,
    step_y: bool,
    step_z: bool,
) -> [IVec3; 7] {
    let sx = if step_x { step.x } else { 0 };
    let sy = if step_y { step.y } else { 0 };
    let sz = if step_z { step.z } else { 0 };

    [
        voxel + IVec3::new(sx, 0, 0),
        voxel + IVec3::new(0, sy, 0),
        voxel + IVec3::new(0, 0, sz),
        voxel + IVec3::new(sx, sy, 0),
        voxel + IVec3::new(sx, 0, sz),
        voxel + IVec3::new(0, sy, sz),
        voxel + IVec3::new(sx, sy, sz),
    ]
}

fn can_place_block_at(
    world: &World,
    resolved_blocks: &ResolvedBlockRegistry,
    player: &Player,
    placement: IVec3,
) -> bool {
    let placement_is_air = {
        let accessor = VoxelAccessor { world };
        resolved_blocks.get_voxel(accessor.get_world_voxel(placement)).is_air()
    };

    placement_is_air && !player_intersects_voxel(player, placement)
}

fn player_intersects_voxel(player: &Player, voxel: IVec3) -> bool {
    let (player_min, player_max) = player.aabb();
    let voxel_min = voxel.as_vec3();
    let voxel_max = voxel_min + Vec3::ONE;
    aabb_intersects(player_min, player_max, voxel_min, voxel_max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::PlayerConfig,
        engine::{
            player::controller::Player,
            render::materials::create_texture_registry,
            world::{
                block::{create_default_block_registry, resolved::ResolvedBlockRegistry},
                storage::World,
            },
        },
    };

    fn test_blocks() -> (ResolvedBlockRegistry, BlockId) {
        let registry = create_default_block_registry();
        let textures = create_texture_registry(&registry);
        let resolved = ResolvedBlockRegistry::build(&registry, textures.layer_map());
        let stone = registry.must_get_id("stone");
        (resolved, stone)
    }

    fn test_player() -> Player {
        let config = PlayerConfig { spawn_y: 0.0, ..PlayerConfig::default() };
        Player::from_config(&config)
    }

    #[test]
    fn raycast_hits_first_block_and_returns_adjacent_placement() {
        let (resolved, stone) = test_blocks();
        let mut world = World::new();
        let chunk = crate::engine::world::coord::ChunkCoord(IVec3::ZERO);

        world.insert_chunk(chunk, crate::engine::world::chunk::Chunk::new());
        let _ = world.take_dirty();
        assert!(world.set_block_world(IVec3::new(0, 0, 2), stone));

        let hit = raycast_blocks(
            &VoxelAccessor { world: &world },
            &resolved,
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::new(0.0, 0.0, 1.0),
            8.0,
        )
        .expect("expected the raycast to hit the first stone block");

        assert_eq!(hit.block, IVec3::new(0, 0, 2));
        assert_eq!(hit.placement, Some(IVec3::new(0, 0, 1)));
    }

    #[test]
    fn raycast_does_not_skip_front_blocks_at_voxel_seams() {
        let (resolved, stone) = test_blocks();
        let mut world = World::new();
        let chunk = crate::engine::world::coord::ChunkCoord(IVec3::ZERO);

        world.insert_chunk(chunk, crate::engine::world::chunk::Chunk::new());
        let _ = world.take_dirty();
        assert!(world.set_block_world(IVec3::new(1, 0, 0), stone));
        assert!(world.set_block_world(IVec3::new(0, 1, 0), stone));
        assert!(world.set_block_world(IVec3::new(1, 1, 0), stone));

        let hit = raycast_blocks(
            &VoxelAccessor { world: &world },
            &resolved,
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::new(1.0, 1.0, 0.0),
            8.0,
        )
        .expect("expected the raycast to hit one of the seam-adjacent blocks");

        assert_eq!(hit.block, IVec3::new(1, 0, 0));
        assert_ne!(hit.block, IVec3::new(1, 1, 0));
    }

    #[test]
    fn place_block_rejects_player_intersection() {
        let (resolved, stone) = test_blocks();
        let mut world = World::new();
        let chunk = crate::engine::world::coord::ChunkCoord(IVec3::ZERO);
        let mut player = test_player();

        world.insert_chunk(chunk, crate::engine::world::chunk::Chunk::new());
        let _ = world.take_dirty();
        assert!(world.set_block_world(IVec3::new(0, 0, 2), stone));

        player.position = Vec3::new(0.5, 0.0, 1.5);

        assert!(!place_block_in_front(
            &mut world,
            &resolved,
            &player,
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::new(0.0, 0.0, 1.0),
            8.0,
            stone,
        ));
    }

    #[test]
    fn place_block_rejects_non_air_space() {
        let (resolved, stone) = test_blocks();
        let mut world = World::new();
        let mut player = test_player();

        world.insert_chunk(
            crate::engine::world::coord::ChunkCoord(IVec3::ZERO),
            crate::engine::world::chunk::Chunk::new(),
        );
        let _ = world.take_dirty();
        assert!(world.set_block_world(IVec3::ZERO, stone));
        player.position = Vec3::new(10.0, 10.0, 10.0);

        assert!(!can_place_block_at(&world, &resolved, &player, IVec3::ZERO));
    }
}
