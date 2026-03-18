struct Camera {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    view_proj: mat4x4<f32>,
    inv_view: mat4x4<f32>,
    inv_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    near_plane: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

struct RenderSettings {
    debug_view_mode: u32,
    chunk_size: u32,
    draw_index_mode: u32,
    time_seconds: f32,
};

struct SkyUniform {
    zenith_color: vec4<f32>,
    horizon_color: vec4<f32>,
    cloud_color: vec4<f32>,
    sun_color: vec4<f32>,
    sun_direction: vec4<f32>,
    sky_params: vec4<f32>,
    cloud_params: vec4<f32>,
    cloud_style: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: Camera;

@group(0) @binding(1)
var<uniform> render_settings: RenderSettings;

@group(1) @binding(0)
var<uniform> sky: SkyUniform;

struct VsIn {
    @location(0) uv: vec2<f32>,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) screen_uv: vec2<f32>,
};

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let frac_part = fract(p);
    let smooth_frac = frac_part * frac_part * (vec2<f32>(3.0, 3.0) - 2.0 * frac_part);

    let a = hash12(cell);
    let b = hash12(cell + vec2<f32>(1.0, 0.0));
    let c = hash12(cell + vec2<f32>(0.0, 1.0));
    let d = hash12(cell + vec2<f32>(1.0, 1.0));

    return mix(mix(a, b, smooth_frac.x), mix(c, d, smooth_frac.x), smooth_frac.y);
}

fn reconstruct_view_dir(screen_uv: vec2<f32>) -> vec3<f32> {
    let ndc = vec4<f32>(screen_uv.x * 2.0 - 1.0, 1.0 - screen_uv.y * 2.0, 0.0, 1.0);
    let world = camera.inv_view_proj * ndc;
    let world_pos = world.xyz / max(abs(world.w), 0.00001);
    return normalize(world_pos - camera.camera_pos.xyz);
}

fn cloud_alpha(view_dir: vec3<f32>) -> f32 {
    let cloud_height_delta = sky.cloud_params.y - camera.camera_pos.y;
    if view_dir.y <= 0.02 || cloud_height_delta <= 0.0 {
        return 0.0;
    }

    let wind_dir = normalize(vec2<f32>(0.94, 0.35));
    let travel = cloud_height_delta / view_dir.y;
    let world_xz = camera.camera_pos.xz + view_dir.xz * travel;
    let drifted_world_xz =
        world_xz + wind_dir * render_settings.time_seconds * sky.cloud_params.z;
    let cloud_uv = drifted_world_xz * sky.cloud_params.x;

    let base = value_noise(cloud_uv);
    let detail = value_noise(cloud_uv * 2.03 + vec2<f32>(19.7, -11.4));
    let wisps = value_noise(cloud_uv * 4.11 - vec2<f32>(7.9, 5.2));
    let shape = base * 0.62 + detail * 0.28 + wisps * 0.10;

    let softness = max(sky.cloud_style.x, 0.001);
    let density = smoothstep(
        sky.cloud_params.w - softness,
        sky.cloud_params.w + softness,
        shape,
    );
    let horizon_fade = smoothstep(0.03, 0.18, view_dir.y);
    let altitude_fade =
        1.0 - smoothstep(-16.0, 48.0, camera.camera_pos.y - sky.cloud_params.y);

    return density * horizon_fade * altitude_fade * sky.cloud_style.y;
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip_pos = vec4<f32>(in.uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.screen_uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let view_dir = reconstruct_view_dir(in.screen_uv);
    let horizon_blend = smoothstep(-0.08, 0.38, view_dir.y);
    let below_horizon_blend = smoothstep(-0.30, 0.04, view_dir.y);

    var sky_color = mix(sky.horizon_color.rgb, sky.zenith_color.rgb, horizon_blend);
    sky_color = mix(sky.horizon_color.rgb * 0.28, sky_color, below_horizon_blend);

    let sun_dir = normalize(sky.sun_direction.xyz);
    let sun_alignment = max(dot(view_dir, sun_dir), 0.0);
    let sun_glow = pow(sun_alignment, sky.sky_params.y) * sky.sky_params.z;
    let sun_disc = smoothstep(sky.sky_params.x, 1.0, sun_alignment);
    sky_color += sky.sun_color.rgb * (sun_glow + sun_disc);

    let cloud_alpha_value = cloud_alpha(view_dir);
    if cloud_alpha_value > 0.0 {
        let cloud_sun = pow(sun_alignment, 6.0);
        let cloud_color = mix(
            sky.cloud_color.rgb * 0.92,
            sky.sun_color.rgb,
            cloud_sun * 0.25,
        ) * (0.82 + cloud_sun * 0.18);
        sky_color = mix(sky_color, cloud_color, cloud_alpha_value);
    }

    return vec4<f32>(sky_color, 1.0);
}
