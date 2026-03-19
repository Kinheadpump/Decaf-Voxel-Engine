use crate::config::{
    ContinentalRegionConfig, ContinentalRegionsConfig, Density3DConfig, NoiseConfig, TerrainConfig,
};

use super::BlueprintSample;

pub(super) fn sample_noise_signed(
    seed: u64,
    world_x: i32,
    world_z: i32,
    config: NoiseConfig,
) -> f32 {
    let sample_x = world_x as f32 * config.scale;
    let sample_z = world_z as f32 * config.scale;
    fbm_perlin_2d(seed, sample_x, sample_z, config.octaves, config.persistence, config.lacunarity)
}

pub(super) fn sample_noise01(seed: u64, world_x: i32, world_z: i32, config: NoiseConfig) -> f32 {
    remap_to_unit_interval(sample_noise_signed(seed, world_x, world_z, config))
}

pub(super) fn sample_noise_3d_signed(
    seed: u64,
    sample_x: f32,
    sample_y: f32,
    sample_z: f32,
    config: Density3DConfig,
) -> f32 {
    fbm_perlin_3d(
        seed,
        sample_x * config.scale,
        sample_y * config.scale,
        sample_z * config.scale,
        config.octaves,
        config.persistence,
        config.lacunarity,
    )
}

pub(super) fn sample_continental_region(
    terrain: TerrainConfig,
    regions: ContinentalRegionsConfig,
    continentalness: f32,
) -> BlueprintSample {
    let sample = continentalness.clamp(0.0, 1.0);
    let ordered_regions = [
        ("DEEP OCEAN", sanitize_continental_region(regions.deep_ocean)),
        ("OCEAN", sanitize_continental_region(regions.ocean)),
        ("COAST", sanitize_continental_region(regions.coast)),
        ("PLAINS", sanitize_continental_region(regions.plains)),
        ("HIGHLANDS", sanitize_continental_region(regions.highlands)),
        ("MOUNTAINS", sanitize_continental_region(regions.mountains)),
    ];

    let mut min_value = 0.0;
    let mut spline_points = [SplinePoint::default(); 6];
    for (index, (region_name, region)) in ordered_regions.into_iter().enumerate() {
        spline_points[index] =
            SplinePoint { center: (min_value + region.max_value) * 0.5, name: region_name, region };
        min_value = region.max_value;
    }

    if sample <= spline_points[0].center {
        let region = spline_points[0].region;
        return BlueprintSample {
            base_height: region.base_height,
            roughness: region.roughness,
            mountainness: mountainness_from_roughness(
                terrain,
                spline_points[spline_points.len() - 1].region.roughness,
                region.roughness,
            ),
            continentalness: sample,
            region_name: spline_points[0].name,
        };
    }

    for spline_window in spline_points.windows(2) {
        let left_point = spline_window[0];
        let right_point = spline_window[1];

        if sample <= right_point.center {
            let blend_factor = smoothstep_range(left_point.center, right_point.center, sample);
            let blended_region =
                blend_continental_region(left_point.region, right_point.region, blend_factor);
            let dominant_region_name =
                if blend_factor < 0.5 { left_point.name } else { right_point.name };

            return BlueprintSample {
                base_height: blended_region.base_height,
                roughness: blended_region.roughness,
                mountainness: mountainness_from_roughness(
                    terrain,
                    spline_points[spline_points.len() - 1].region.roughness,
                    blended_region.roughness,
                ),
                continentalness: sample,
                region_name: dominant_region_name,
            };
        }
    }

    let last_point = spline_points[spline_points.len() - 1];
    BlueprintSample {
        base_height: last_point.region.base_height,
        roughness: last_point.region.roughness,
        mountainness: mountainness_from_roughness(
            terrain,
            last_point.region.roughness,
            last_point.region.roughness,
        ),
        continentalness: sample,
        region_name: last_point.name,
    }
}

fn sanitize_continental_region(region: ContinentalRegionConfig) -> ContinentalRegionConfig {
    ContinentalRegionConfig {
        max_value: region.max_value.clamp(0.0, 1.0),
        base_height: region.base_height,
        roughness: region.roughness.max(0.0),
    }
}

fn fbm_perlin_2d(
    seed: u64,
    sample_x: f32,
    sample_z: f32,
    octaves: u32,
    persistence: f32,
    lacunarity: f32,
) -> f32 {
    let octave_count = octaves.max(1);
    let persistence = persistence.clamp(0.0, 1.0);
    let lacunarity = lacunarity.max(1.0);
    let mut total = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    let mut normalization = 0.0;

    for octave in 0..octave_count {
        total += perlin_2d(
            seed.wrapping_add((octave as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)),
            sample_x * frequency,
            sample_z * frequency,
        ) * amplitude;
        normalization += amplitude;
        amplitude *= persistence;
        frequency *= lacunarity;
    }

    total / normalization.max(f32::EPSILON)
}

fn fbm_perlin_3d(
    seed: u64,
    sample_x: f32,
    sample_y: f32,
    sample_z: f32,
    octaves: u32,
    persistence: f32,
    lacunarity: f32,
) -> f32 {
    let octave_count = octaves.max(1);
    let persistence = persistence.clamp(0.0, 1.0);
    let lacunarity = lacunarity.max(1.0);
    let mut total = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    let mut normalization = 0.0;

    for octave in 0..octave_count {
        total += perlin_3d(
            seed.wrapping_add((octave as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)),
            sample_x * frequency,
            sample_y * frequency,
            sample_z * frequency,
        ) * amplitude;
        normalization += amplitude;
        amplitude *= persistence;
        frequency *= lacunarity;
    }

    total / normalization.max(f32::EPSILON)
}

fn perlin_2d(seed: u64, sample_x: f32, sample_z: f32) -> f32 {
    let x0 = sample_x.floor() as i32;
    let z0 = sample_z.floor() as i32;
    let x1 = x0 + 1;
    let z1 = z0 + 1;
    let x_fraction = sample_x - x0 as f32;
    let z_fraction = sample_z - z0 as f32;
    let x_blend = fade(x_fraction);
    let z_blend = fade(z_fraction);

    let bottom_left = gradient_dot_2d(hash_2d(seed, x0, z0), x_fraction, z_fraction);
    let bottom_right = gradient_dot_2d(hash_2d(seed, x1, z0), x_fraction - 1.0, z_fraction);
    let top_left = gradient_dot_2d(hash_2d(seed, x0, z1), x_fraction, z_fraction - 1.0);
    let top_right = gradient_dot_2d(hash_2d(seed, x1, z1), x_fraction - 1.0, z_fraction - 1.0);

    lerp(lerp(bottom_left, bottom_right, x_blend), lerp(top_left, top_right, x_blend), z_blend)
}

fn perlin_3d(seed: u64, sample_x: f32, sample_y: f32, sample_z: f32) -> f32 {
    let x0 = sample_x.floor() as i32;
    let y0 = sample_y.floor() as i32;
    let z0 = sample_z.floor() as i32;
    let x1 = x0 + 1;
    let y1 = y0 + 1;
    let z1 = z0 + 1;
    let x_fraction = sample_x - x0 as f32;
    let y_fraction = sample_y - y0 as f32;
    let z_fraction = sample_z - z0 as f32;
    let x_blend = fade(x_fraction);
    let y_blend = fade(y_fraction);
    let z_blend = fade(z_fraction);

    let lower_near_left =
        gradient_dot_3d(hash_3d(seed, x0, y0, z0), x_fraction, y_fraction, z_fraction);
    let lower_near_right =
        gradient_dot_3d(hash_3d(seed, x1, y0, z0), x_fraction - 1.0, y_fraction, z_fraction);
    let upper_near_left =
        gradient_dot_3d(hash_3d(seed, x0, y1, z0), x_fraction, y_fraction - 1.0, z_fraction);
    let upper_near_right =
        gradient_dot_3d(hash_3d(seed, x1, y1, z0), x_fraction - 1.0, y_fraction - 1.0, z_fraction);
    let lower_far_left =
        gradient_dot_3d(hash_3d(seed, x0, y0, z1), x_fraction, y_fraction, z_fraction - 1.0);
    let lower_far_right =
        gradient_dot_3d(hash_3d(seed, x1, y0, z1), x_fraction - 1.0, y_fraction, z_fraction - 1.0);
    let upper_far_left =
        gradient_dot_3d(hash_3d(seed, x0, y1, z1), x_fraction, y_fraction - 1.0, z_fraction - 1.0);
    let upper_far_right = gradient_dot_3d(
        hash_3d(seed, x1, y1, z1),
        x_fraction - 1.0,
        y_fraction - 1.0,
        z_fraction - 1.0,
    );

    let near_lower = lerp(lower_near_left, lower_near_right, x_blend);
    let near_upper = lerp(upper_near_left, upper_near_right, x_blend);
    let far_lower = lerp(lower_far_left, lower_far_right, x_blend);
    let far_upper = lerp(upper_far_left, upper_far_right, x_blend);
    let near_plane = lerp(near_lower, near_upper, y_blend);
    let far_plane = lerp(far_lower, far_upper, y_blend);

    lerp(near_plane, far_plane, z_blend)
}

fn gradient_dot_2d(hash: u64, sample_x: f32, sample_z: f32) -> f32 {
    const DIAGONAL_COMPONENT: f32 = 0.707_106_77;

    match hash & 7 {
        0 => sample_x,
        1 => -sample_x,
        2 => sample_z,
        3 => -sample_z,
        4 => (sample_x + sample_z) * DIAGONAL_COMPONENT,
        5 => (sample_x - sample_z) * DIAGONAL_COMPONENT,
        6 => (-sample_x + sample_z) * DIAGONAL_COMPONENT,
        _ => (-sample_x - sample_z) * DIAGONAL_COMPONENT,
    }
}

fn gradient_dot_3d(hash: u64, sample_x: f32, sample_y: f32, sample_z: f32) -> f32 {
    const DIAGONAL_COMPONENT: f32 = 0.707_106_77;

    match hash % 12 {
        0 => (sample_x + sample_y) * DIAGONAL_COMPONENT,
        1 => (sample_x - sample_y) * DIAGONAL_COMPONENT,
        2 => (-sample_x + sample_y) * DIAGONAL_COMPONENT,
        3 => (-sample_x - sample_y) * DIAGONAL_COMPONENT,
        4 => (sample_x + sample_z) * DIAGONAL_COMPONENT,
        5 => (sample_x - sample_z) * DIAGONAL_COMPONENT,
        6 => (-sample_x + sample_z) * DIAGONAL_COMPONENT,
        7 => (-sample_x - sample_z) * DIAGONAL_COMPONENT,
        8 => (sample_y + sample_z) * DIAGONAL_COMPONENT,
        9 => (sample_y - sample_z) * DIAGONAL_COMPONENT,
        10 => (-sample_y + sample_z) * DIAGONAL_COMPONENT,
        _ => (-sample_y - sample_z) * DIAGONAL_COMPONENT,
    }
}

fn fade(value: f32) -> f32 {
    let clamped = value.clamp(0.0, 1.0);
    clamped * clamped * clamped * (clamped * (clamped * 6.0 - 15.0) + 10.0)
}

fn remap_to_unit_interval(value: f32) -> f32 {
    value.mul_add(0.5, 0.5).clamp(0.0, 1.0)
}

fn smoothstep_range(edge0: f32, edge1: f32, value: f32) -> f32 {
    if (edge1 - edge0).abs() <= f32::EPSILON {
        return (value >= edge1) as u8 as f32;
    }

    fade(((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0))
}

fn lerp(start: f32, end: f32, blend_factor: f32) -> f32 {
    start + (end - start) * blend_factor
}

fn hash_2d(seed: u64, sample_x: i32, sample_z: i32) -> u64 {
    mix64(
        seed ^ (sample_x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (sample_z as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F),
    )
}

fn hash_3d(seed: u64, sample_x: i32, sample_y: i32, sample_z: i32) -> u64 {
    mix64(
        seed ^ (sample_x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (sample_y as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ (sample_z as i64 as u64).wrapping_mul(0x1656_67B1_9E37_79F9),
    )
}

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
    blend_factor: f32,
) -> ContinentalRegionConfig {
    ContinentalRegionConfig {
        max_value: lerp(left.max_value, right.max_value, blend_factor),
        base_height: lerp(left.base_height, right.base_height, blend_factor),
        roughness: lerp(left.roughness, right.roughness, blend_factor),
    }
}

fn mountainness_from_roughness(terrain: TerrainConfig, max_roughness: f32, roughness: f32) -> f32 {
    smoothstep_range(terrain.mountain_start_roughness, max_roughness, roughness)
}
