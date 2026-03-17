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
    _pad0: u32,
    _pad1: u32,
};

struct DrawMeta {
    chunk_origin: vec4<i32>,
    face_dir: u32,
    face_offset: u32,
    face_count: u32,
    draw_id: u32,
};

struct PackedFace {
    value: u32,
};

@group(0) @binding(0)
var<uniform> camera: Camera;

@group(0) @binding(1)
var<uniform> render_settings: RenderSettings;

@group(1) @binding(0)
var<uniform> draw_meta: DrawMeta;

@group(1) @binding(1)
var<storage, read> faces: array<PackedFace>;

@group(2) @binding(0)
var tex_array: texture_2d_array<f32>;

@group(2) @binding(1)
var tex_sampler: sampler;

struct VsIn {
    @location(0) uv: vec2<f32>,
    @builtin(instance_index) instance_index: u32,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) tex_uv: vec2<f32>,
    @interpolate(flat)
    @location(1) tex_id: u32,
    @location(2) normal: vec3<f32>,
};

fn face_u_axis(dir: u32) -> vec3<f32> {
    switch dir {
        case 0u { return vec3<f32>( 0.0, 1.0, 0.0); } // +X
        case 1u { return vec3<f32>( 0.0, 0.0, 1.0); } // -X
        case 2u { return vec3<f32>( 0.0, 0.0, 1.0); } // +Y
        case 3u { return vec3<f32>( 1.0, 0.0, 0.0); } // -Y
        case 4u { return vec3<f32>( 1.0, 0.0, 0.0); } // +Z
        default { return vec3<f32>( 0.0, 1.0, 0.0); } // -Z
    }
}

fn face_v_axis(dir: u32) -> vec3<f32> {
    switch dir {
        case 0u { return vec3<f32>(0.0, 0.0, 1.0); } // +X
        case 1u { return vec3<f32>(0.0, 1.0, 0.0); } // -X
        case 2u { return vec3<f32>(1.0, 0.0, 0.0); } // +Y
        case 3u { return vec3<f32>(0.0, 0.0, 1.0); } // -Y
        case 4u { return vec3<f32>(0.0, 1.0, 0.0); } // +Z
        default { return vec3<f32>(1.0, 0.0, 0.0); } // -Z
    }
}

fn face_normal(dir: u32) -> vec3<f32> {
    switch dir {
        case 0u { return vec3<f32>( 1.0, 0.0, 0.0); }
        case 1u { return vec3<f32>(-1.0, 0.0, 0.0); }
        case 2u { return vec3<f32>( 0.0, 1.0, 0.0); }
        case 3u { return vec3<f32>( 0.0,-1.0, 0.0); }
        case 4u { return vec3<f32>( 0.0, 0.0, 1.0); }
        default { return vec3<f32>( 0.0, 0.0,-1.0); }
    }
}

fn face_dir_color(dir: u32) -> vec3<f32> {
    switch dir {
        case 0u { return vec3<f32>(0.95, 0.34, 0.27); }
        case 1u { return vec3<f32>(0.69, 0.17, 0.14); }
        case 2u { return vec3<f32>(0.30, 0.78, 0.39); }
        case 3u { return vec3<f32>(0.14, 0.47, 0.21); }
        case 4u { return vec3<f32>(0.24, 0.55, 0.96); }
        default { return vec3<f32>(0.14, 0.24, 0.63); }
    }
}

fn hash_u32(x: u32) -> u32 {
    var h = x;
    h ^= h >> 16u;
    h *= 0x7feb352du;
    h ^= h >> 15u;
    h *= 0x846ca68bu;
    h ^= h >> 16u;
    return h;
}

fn hash_color_u32(x: u32) -> vec3<f32> {
    let h = hash_u32(x);
    return vec3<f32>(
        f32(h & 0xffu) / 255.0,
        f32((h >> 8u) & 0xffu) / 255.0,
        f32((h >> 16u) & 0xffu) / 255.0,
    ) * 0.8 + vec3<f32>(0.2, 0.2, 0.2);
}

fn chunk_coord_color() -> vec3<f32> {
    let chunk_coord = draw_meta.chunk_origin.xyz / i32(render_settings.chunk_size);
    let seed =
        bitcast<u32>(chunk_coord.x * 73856093) ^
        bitcast<u32>(chunk_coord.y * 19349663) ^
        bitcast<u32>(chunk_coord.z * 83492791);
    return hash_color_u32(seed);
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let face = faces[draw_meta.face_offset + in.instance_index].value;

    let x = f32((face >> 0u) & 0x1Fu);
    let y = f32((face >> 5u) & 0x1Fu);
    let z = f32((face >> 10u) & 0x1Fu);
    let tex_id = (face >> 15u) & 0x7Fu;
    let w = f32(((face >> 22u) & 0x1Fu) + 1u);
    let h = f32(((face >> 27u) & 0x1Fu) + 1u);

    let u_axis = face_u_axis(draw_meta.face_dir);
    let v_axis = face_v_axis(draw_meta.face_dir);
    let normal = face_normal(draw_meta.face_dir);

    let local_anchor = vec3<f32>(x, y, z);
    let positive_dir = f32((draw_meta.face_dir & 1u) == 0u);

    let local_pos =
        local_anchor +
        u_axis * (in.uv.x * w) +
        v_axis * (in.uv.y * h) +
        normal * positive_dir;

    let world_pos = local_pos + vec3<f32>(
        f32(draw_meta.chunk_origin.x),
        f32(draw_meta.chunk_origin.y),
        f32(draw_meta.chunk_origin.z)
    );

    var out: VsOut;
    out.clip_pos = camera.view_proj * vec4<f32>(world_pos, 1.0);
    out.tex_uv = vec2<f32>(in.uv.x * w, in.uv.y * h);
    out.tex_id = tex_id;
    out.normal = normal;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    switch render_settings.debug_view_mode {
        case 1u { return vec4<f32>(face_dir_color(draw_meta.face_dir), 1.0); }
        case 2u { return vec4<f32>(chunk_coord_color(), 1.0); }
        case 3u { return vec4<f32>(hash_color_u32(draw_meta.draw_id), 1.0); }
        default {}
    }

    let light_dir = normalize(vec3<f32>(0.7, 1.0, 0.5));
    let diffuse = max(dot(in.normal, light_dir), 0.18);
    let color = textureSample(tex_array, tex_sampler, fract(in.tex_uv), i32(in.tex_id));
    return vec4<f32>(color.rgb * diffuse, color.a);
}
