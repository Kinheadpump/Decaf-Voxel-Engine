use winit::keyboard::KeyCode;

use crate::{
    config::PlayerConfig,
    engine::{
        core::math::{IVec3, Vec3},
        input::InputState,
        player::controller::{MovementMode, Player},
        world::{accessor::VoxelAccessor, block::resolved::ResolvedBlockRegistry, storage::World},
    },
};

pub fn update_player(
    player: &mut Player,
    input: &InputState,
    world: &World,
    resolved_blocks: &ResolvedBlockRegistry,
    total_time: f32,
    config: &PlayerConfig,
) {
    let _span = crate::profile_span!("player::update");
    let accessor = VoxelAccessor { world };

    update_look(player, input, config);
    update_mode_toggles(player, input, total_time, config);

    match player.movement_mode {
        MovementMode::Walking => update_walking(player, input, &accessor, resolved_blocks, config),
        MovementMode::Flying => update_flying(player, input, &accessor, resolved_blocks, config),
    }
}

fn update_look(player: &mut Player, input: &InputState, config: &PlayerConfig) {
    let (dx, dy) = input.mouse_delta;

    player.yaw += dx * config.mouse_sensitivity;
    player.pitch -= dy * config.mouse_sensitivity;

    let max_pitch = std::f32::consts::FRAC_PI_2 - 0.001;
    player.pitch = player.pitch.clamp(-max_pitch, max_pitch);
}

fn update_mode_toggles(
    player: &mut Player,
    input: &InputState,
    total_time: f32,
    config: &PlayerConfig,
) {
    if input.key_pressed(KeyCode::Space) {
        let dt = total_time - player.last_space_press_time;

        if dt <= config.double_tap_window {
            player.space_press_count = player.space_press_count.saturating_add(1);
        } else {
            player.space_press_count = 1;
        }

        player.last_space_press_time = total_time;

        if player.space_press_count >= 2 {
            player.space_press_count = 0;
            player.movement_mode = match player.movement_mode {
                MovementMode::Walking => {
                    player.velocity.y = 0.0;
                    MovementMode::Flying
                }
                MovementMode::Flying => MovementMode::Walking,
            };
        }
    }

    player.wants_jump_hold = input.key_held(KeyCode::Space);
}

#[inline]
fn is_sprinting(input: &InputState) -> bool {
    input.key_held(KeyCode::ShiftLeft) || input.key_held(KeyCode::ShiftRight)
}

fn movement_wish_dir_flat(player: &Player, input: &InputState) -> Vec3 {
    let mut dir = Vec3::ZERO;

    if input.key_held(KeyCode::KeyW) {
        dir += player.forward_flat();
    }
    if input.key_held(KeyCode::KeyS) {
        dir -= player.forward_flat();
    }
    if input.key_held(KeyCode::KeyD) {
        dir += player.right_flat();
    }
    if input.key_held(KeyCode::KeyA) {
        dir -= player.right_flat();
    }

    dir.normalize_or_zero()
}

fn movement_wish_dir_flying(player: &Player, input: &InputState) -> Vec3 {
    let mut dir = Vec3::ZERO;
    let forward = player.forward_flat();
    let right = player.right_flat();

    if input.key_held(KeyCode::KeyW) {
        dir += forward;
    }
    if input.key_held(KeyCode::KeyS) {
        dir -= forward;
    }
    if input.key_held(KeyCode::KeyD) {
        dir += right;
    }
    if input.key_held(KeyCode::KeyA) {
        dir -= right;
    }
    if input.key_held(KeyCode::Space) {
        dir += Vec3::Y;
    }
    if input.key_held(KeyCode::ControlLeft) || input.key_held(KeyCode::ControlRight) {
        dir -= Vec3::Y;
    }

    dir.normalize_or_zero()
}

fn accelerate(current: Vec3, wish_dir: Vec3, wish_speed: f32, accel: f32, dt: f32) -> Vec3 {
    if wish_dir == Vec3::ZERO {
        return current;
    }

    let current_speed_along = current.dot(wish_dir);
    let add_speed = wish_speed - current_speed_along;

    if add_speed <= 0.0 {
        return current;
    }

    let accel_speed = (accel * dt * wish_speed).min(add_speed);
    current + wish_dir * accel_speed
}

fn apply_friction(vel: Vec3, friction: f32, dt: f32, horizontal_only: bool) -> Vec3 {
    if horizontal_only {
        let horizontal = Vec3::new(vel.x, 0.0, vel.z);
        let speed = horizontal.length();
        if speed <= 0.0001 {
            return Vec3::new(0.0, vel.y, 0.0);
        }

        let drop = speed * friction * dt;
        let new_speed = (speed - drop).max(0.0);
        let scale = new_speed / speed;

        Vec3::new(horizontal.x * scale, vel.y, horizontal.z * scale)
    } else {
        let speed = vel.length();
        if speed <= 0.0001 {
            return Vec3::ZERO;
        }

        let drop = speed * friction * dt;
        let new_speed = (speed - drop).max(0.0);
        vel * (new_speed / speed)
    }
}

fn update_walking(
    player: &mut Player,
    input: &InputState,
    accessor: &VoxelAccessor,
    resolved_blocks: &ResolvedBlockRegistry,
    config: &PlayerConfig,
) {
    let dt = input.dt;

    let wish_dir = movement_wish_dir_flat(player, input);
    let sprint = is_sprinting(input);
    let max_speed =
        if sprint { config.walk_speed * config.walk_sprint_multiplier } else { config.walk_speed };

    if player.on_ground {
        player.velocity = apply_friction(player.velocity, config.ground_friction, dt, true);
        player.velocity = accelerate(player.velocity, wish_dir, max_speed, config.walk_accel, dt);

        if player.wants_jump_hold {
            player.velocity.y = config.jump_speed;
            player.on_ground = false;
        }
    } else {
        player.velocity = apply_friction(player.velocity, config.air_friction, dt, true);
        player.velocity = accelerate(player.velocity, wish_dir, max_speed, config.air_accel, dt);
        player.velocity.y -= config.gravity * dt;
    }

    move_and_collide(player, accessor, resolved_blocks, dt, config.collision_steps);
}

fn update_flying(
    player: &mut Player,
    input: &InputState,
    accessor: &VoxelAccessor,
    resolved_blocks: &ResolvedBlockRegistry,
    config: &PlayerConfig,
) {
    let dt = input.dt;

    let wish_dir = movement_wish_dir_flying(player, input);
    let sprint = is_sprinting(input);
    let max_speed =
        if sprint { config.fly_speed * config.fly_sprint_multiplier } else { config.fly_speed };

    player.velocity = apply_friction(player.velocity, config.fly_friction, dt, false);
    player.velocity = accelerate(player.velocity, wish_dir, max_speed, config.fly_accel, dt);

    move_and_collide(player, accessor, resolved_blocks, dt, config.collision_steps);
}

fn player_aabb(player: &Player, pos: Vec3) -> (Vec3, Vec3) {
    let mut moved_player = player.clone();
    moved_player.position = pos;
    moved_player.aabb()
}

#[cfg(test)]
mod tests {
    use super::movement_wish_dir_flying;
    use crate::{
        config::PlayerConfig,
        engine::{input::InputState, player::controller::Player},
    };
    use winit::keyboard::KeyCode;

    fn test_player() -> Player {
        Player::from_config(&PlayerConfig {
            spawn_x: 0.0,
            spawn_y: 0.0,
            spawn_z: 0.0,
            reach_distance: 6.0,
            mouse_sensitivity: 0.0022,
            eye_height: 1.62,
            radius: 0.3,
            height: 1.8,
            double_tap_window: 0.25,
            collision_steps: 3,
            walk_speed: 5.0,
            walk_sprint_multiplier: 1.8,
            walk_accel: 45.0,
            air_accel: 10.0,
            ground_friction: 14.0,
            air_friction: 1.0,
            jump_speed: 8.5,
            gravity: 24.0,
            fly_speed: 7.0,
            fly_sprint_multiplier: 2.5,
            fly_accel: 24.0,
            fly_friction: 10.0,
        })
    }

    #[test]
    fn flying_forward_stays_flat_when_looking_up() {
        let mut player = test_player();
        let mut input = InputState::new();

        player.pitch = 1.0;
        input.set_key_held_for_test(KeyCode::KeyW);

        let wish_dir = movement_wish_dir_flying(&player, &input);

        assert!(wish_dir.y.abs() <= f32::EPSILON);
    }
}

fn is_solid_at_world(
    accessor: &VoxelAccessor,
    resolved_blocks: &ResolvedBlockRegistry,
    wx: i32,
    wy: i32,
    wz: i32,
) -> bool {
    let voxel = accessor.get_world_voxel(IVec3::new(wx, wy, wz));
    resolved_blocks.get_voxel(voxel).is_solid()
}

fn collides_with_world(
    accessor: &VoxelAccessor,
    resolved_blocks: &ResolvedBlockRegistry,
    min: Vec3,
    max: Vec3,
) -> bool {
    let min_x = min.x.floor() as i32;
    let min_y = min.y.floor() as i32;
    let min_z = min.z.floor() as i32;

    let max_x = max.x.floor() as i32;
    let max_y = max.y.floor() as i32;
    let max_z = max.z.floor() as i32;

    for z in min_z..=max_z {
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if is_solid_at_world(accessor, resolved_blocks, x, y, z) {
                    let voxel_min = Vec3::new(x as f32, y as f32, z as f32);
                    let voxel_max = voxel_min + Vec3::ONE;

                    if aabb_intersects(min, max, voxel_min, voxel_max) {
                        return true;
                    }
                }
            }
        }
    }

    false
}

fn aabb_intersects(a_min: Vec3, a_max: Vec3, b_min: Vec3, b_max: Vec3) -> bool {
    a_min.x < b_max.x
        && a_max.x > b_min.x
        && a_min.y < b_max.y
        && a_max.y > b_min.y
        && a_min.z < b_max.z
        && a_max.z > b_min.z
}

fn move_and_collide(
    player: &mut Player,
    accessor: &VoxelAccessor,
    resolved_blocks: &ResolvedBlockRegistry,
    dt: f32,
    collision_steps: usize,
) {
    player.on_ground = false;

    let mut pos = player.position;
    let mut vel = player.velocity;

    let step_count = collision_steps.max(1);
    let step_dt = dt / step_count as f32;

    for _ in 0..step_count {
        pos.x += vel.x * step_dt;
        let (min, max) = player_aabb(player, pos);
        if collides_with_world(accessor, resolved_blocks, min, max) {
            pos.x -= vel.x * step_dt;
            vel.x = 0.0;
        }

        pos.y += vel.y * step_dt;
        let (min, max) = player_aabb(player, pos);
        if collides_with_world(accessor, resolved_blocks, min, max) {
            pos.y -= vel.y * step_dt;

            if vel.y < 0.0 {
                player.on_ground = true;
            }

            vel.y = 0.0;
        }

        pos.z += vel.z * step_dt;
        let (min, max) = player_aabb(player, pos);
        if collides_with_world(accessor, resolved_blocks, min, max) {
            pos.z -= vel.z * step_dt;
            vel.z = 0.0;
        }
    }

    player.position = pos;
    player.velocity = vel;
}
