struct OverlayUniform {
    screen_size: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> overlay: OverlayUniform;

@group(0) @binding(1)
var hud_textures: texture_2d_array<f32>;

@group(0) @binding(2)
var hud_sampler: sampler;

struct HudSpriteInstance {
    @location(0) origin_px: vec2<f32>,
    @location(1) size_px: vec2<f32>,
    @location(2) uv_min: vec2<f32>,
    @location(3) uv_max: vec2<f32>,
    @location(4) tint: vec4<f32>,
    @location(5) texture_layer: u32,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) sample_uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
    @interpolate(flat) @location(2) texture_layer: u32,
};

fn quad_uv(vertex_index: u32) -> vec2<f32> {
    switch vertex_index {
        case 0u { return vec2<f32>(0.0, 0.0); }
        case 1u { return vec2<f32>(1.0, 0.0); }
        case 2u { return vec2<f32>(0.0, 1.0); }
        default { return vec2<f32>(1.0, 1.0); }
    }
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    sprite: HudSpriteInstance,
) -> VsOut {
    let uv = quad_uv(vertex_index);
    let pos_px = sprite.origin_px + uv * sprite.size_px;
    let clip_pos = vec2<f32>(
        pos_px.x / overlay.screen_size.x * 2.0 - 1.0,
        1.0 - pos_px.y / overlay.screen_size.y * 2.0,
    );

    var out: VsOut;
    out.clip_pos = vec4<f32>(clip_pos, 0.0, 1.0);
    out.sample_uv = mix(sprite.uv_min, sprite.uv_max, uv);
    out.tint = sprite.tint;
    out.texture_layer = sprite.texture_layer;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let sampled = textureSample(hud_textures, hud_sampler, in.sample_uv, i32(in.texture_layer));
    let color = sampled * in.tint;
    if color.a <= 0.01 {
        discard;
    }
    return color;
}
