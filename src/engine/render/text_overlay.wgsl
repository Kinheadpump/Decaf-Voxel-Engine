struct OverlayUniform {
    screen_size: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> overlay: OverlayUniform;

struct GlyphInstance {
    @location(0) origin_px: vec2<f32>,
    @location(1) size_px: vec2<f32>,
    @location(2) glyph_code: u32,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @interpolate(flat) @location(1) glyph_code: u32,
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
    glyph: GlyphInstance,
) -> VsOut {
    let uv = quad_uv(vertex_index);
    let pos_px = glyph.origin_px + uv * glyph.size_px;
    let clip_pos = vec2<f32>(
        pos_px.x / overlay.screen_size.x * 2.0 - 1.0,
        1.0 - pos_px.y / overlay.screen_size.y * 2.0,
    );

    var out: VsOut;
    out.clip_pos = vec4<f32>(clip_pos, 0.0, 1.0);
    out.uv = uv;
    out.glyph_code = glyph.glyph_code;
    return out;
}

fn glyph_bits(code: u32) -> u32 {
    switch code {
        case 32u { return 0x00000u; } // space
        case 45u { return 0x00F00u; } // -
        case 58u { return 0x04040u; } // :
        case 48u { return 0xF999Fu; } // 0
        case 49u { return 0x72262u; } // 1
        case 50u { return 0xF8F1Fu; } // 2
        case 51u { return 0xF171Fu; } // 3
        case 52u { return 0x11F99u; } // 4
        case 53u { return 0xF1F8Fu; } // 5
        case 54u { return 0xF9F8Fu; } // 6
        case 55u { return 0x1111Fu; } // 7
        case 56u { return 0xF9F9Fu; } // 8
        case 57u { return 0xF1F9Fu; } // 9
        case 65u { return 0x99F96u; } // A
        case 66u { return 0xE9E9Eu; } // B
        case 67u { return 0x78887u; } // C
        case 68u { return 0xE999Eu; } // D
        case 69u { return 0xF8E8Fu; } // E
        case 70u { return 0x88E8Fu; } // F
        case 71u { return 0x79B87u; } // G
        case 72u { return 0x99F99u; } // H
        case 73u { return 0x72227u; } // I
        case 74u { return 0x69111u; } // J
        case 75u { return 0x9ACA9u; } // K
        case 76u { return 0xF8888u; } // L
        case 77u { return 0x99FF9u; } // M
        case 78u { return 0x99BD9u; } // N
        case 79u { return 0x69996u; } // O
        case 80u { return 0x88E9Eu; } // P
        case 81u { return 0x7B996u; } // Q
        case 82u { return 0x9AE9Eu; } // R
        case 83u { return 0xF1F8Fu; } // S
        case 84u { return 0x2222Fu; } // T
        case 85u { return 0x69999u; } // U
        case 86u { return 0x66999u; } // V
        case 87u { return 0x9FF99u; } // W
        case 88u { return 0x99699u; } // X
        case 89u { return 0x22699u; } // Y
        case 90u { return 0xF421Fu; } // Z
        default { return 0x00000u; }
    }
}

fn glyph_alpha(code: u32, uv: vec2<f32>) -> f32 {
    if uv.x < 0.0 || uv.x >= 1.0 || uv.y < 0.0 || uv.y >= 1.0 {
        return 0.0;
    }

    let px = u32(floor(uv.x * 4.0));
    let py = u32(floor(uv.y * 5.0));
    let row_bits = (glyph_bits(code) >> (py * 4u)) & 0xFu;
    let mask = 1u << (3u - px);
    return select(0.0, 1.0, (row_bits & mask) != 0u);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let alpha = glyph_alpha(in.glyph_code, in.uv);
    return vec4<f32>(1.0, 1.0, 1.0, alpha);
}
