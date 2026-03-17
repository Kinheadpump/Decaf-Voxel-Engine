struct HiZParams {
    src_size: vec2<u32>,
    dst_size: vec2<u32>,
};

@group(0) @binding(0)
var<uniform> depth_params: HiZParams;

@group(0) @binding(1)
var source_depth: texture_depth_2d;

@group(1) @binding(0)
var<uniform> hiz_params: HiZParams;

@group(1) @binding(1)
var source_hiz: texture_2d<f32>;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
};

fn fullscreen_pos(vertex_index: u32) -> vec2<f32> {
    switch vertex_index {
        case 0u { return vec2<f32>(-1.0, -1.0); }
        case 1u { return vec2<f32>( 3.0, -1.0); }
        default { return vec2<f32>(-1.0,  3.0); }
    }
}

fn div_ceil(numerator: vec2<u32>, denominator: vec2<u32>) -> vec2<u32> {
    return (numerator + denominator - vec2<u32>(1u, 1u)) / denominator;
}

fn source_range(params: HiZParams, dst_coord: vec2<u32>) -> vec4<u32> {
    let src_begin = dst_coord * params.src_size / params.dst_size;
    let src_end = min(
        div_ceil((dst_coord + vec2<u32>(1u, 1u)) * params.src_size, params.dst_size),
        params.src_size,
    );
    let clamped_end = max(src_end, src_begin + vec2<u32>(1u, 1u));
    return vec4<u32>(src_begin, clamped_end);
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    var out: VsOut;
    out.clip_pos = vec4<f32>(fullscreen_pos(vertex_index), 0.0, 1.0);
    return out;
}

@fragment
fn fs_reduce_depth(@builtin(position) position: vec4<f32>) -> @location(0) f32 {
    let dst_coord = vec2<u32>(position.xy);
    let bounds = source_range(depth_params, dst_coord);
    var min_depth = 1.0;

    var y = bounds.y;
    loop {
        if (y >= bounds.w) {
            break;
        }

        var x = bounds.x;
        loop {
            if (x >= bounds.z) {
                break;
            }

            min_depth = min(min_depth, textureLoad(source_depth, vec2<u32>(x, y), 0));
            x += 1u;
        }

        y += 1u;
    }

    return min_depth;
}

@fragment
fn fs_reduce_hiz(@builtin(position) position: vec4<f32>) -> @location(0) f32 {
    let dst_coord = vec2<u32>(position.xy);
    let bounds = source_range(hiz_params, dst_coord);
    var min_depth = 1.0;

    var y = bounds.y;
    loop {
        if (y >= bounds.w) {
            break;
        }

        var x = bounds.x;
        loop {
            if (x >= bounds.z) {
                break;
            }

            min_depth = min(min_depth, textureLoad(source_hiz, vec2<u32>(x, y), 0).x);
            x += 1u;
        }

        y += 1u;
    }

    return min_depth;
}
