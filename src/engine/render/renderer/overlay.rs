use crate::engine::render::gpu_types::{DebugViewMode, RenderStats, TextGlyphInstance};

use super::Renderer;

const RIGHT_COLUMN_OFFSET_CHARS: f32 = 25.0;

impl Renderer {
    pub(super) fn build_overlay_instances(&self, stats: RenderStats) -> Vec<TextGlyphInstance> {
        let Some(debug_overlay) = self.debug_overlay.as_ref() else {
            return Vec::new();
        };

        let left_lines = [
            "F3 DEBUG".to_string(),
            String::new(),
            "PLAYER".to_string(),
            format!("FPS      {}", debug_overlay.fps),
            format!(
                "POS      {} {} {}",
                debug_overlay.player_voxel[0],
                debug_overlay.player_voxel[1],
                debug_overlay.player_voxel[2]
            ),
            format!(
                "CHUNK    {} {} {}",
                debug_overlay.player_chunk[0],
                debug_overlay.player_chunk[1],
                debug_overlay.player_chunk[2]
            ),
            format!("FACING   {}", debug_overlay.player_facing),
            format!("BIOME    {}", debug_overlay.biome_name),
            format!("REGION   {}", debug_overlay.region_name),
            format!("GROUND   {}", debug_overlay.surface_y),
            format!("TEMP     {}", debug_overlay.temperature_percent),
            format!("HUMID    {}", debug_overlay.humidity_percent),
        ];
        let right_lines = [
            "WORLD".to_string(),
            format!("LOADED   {}", debug_overlay.loaded_chunks),
            format!("GPU      {}", stats.gpu_chunks),
            format!("DRAWN    {}", stats.drawn_chunks),
            format!("MESH     {}", stats.meshing_pending_chunks),
            String::new(),
            "RENDER".to_string(),
            format!("FRUSTUM  {}", stats.frustum_culled_chunks),
            format!("OCCLUDED {}", stats.occlusion_culled_chunks),
            format!("DIRCULL  {}", stats.directional_culled_draws),
            format!("OPAQUE   {}", stats.opaque_draws),
            format!("TRANS    {}", stats.transparent_draws),
            format!("HIZ      {}", overlay_hiz_label(stats.hiz_enabled)),
            format!("MODE     {}", overlay_mode_label(self.debug_view_mode)),
        ];

        let max_overlay_glyphs = self.overlay_config.max_glyphs;
        let mut instances = Vec::with_capacity(max_overlay_glyphs.min(320));
        let left_x = self.overlay_config.padding_x;
        let right_x = left_x + RIGHT_COLUMN_OFFSET_CHARS * self.overlay_config.glyph_advance_x;
        let top_y = self.overlay_config.padding_y;

        push_overlay_lines(
            &mut instances,
            &left_lines,
            left_x,
            top_y,
            self.overlay_config,
            max_overlay_glyphs,
        );
        push_overlay_lines(
            &mut instances,
            &right_lines,
            right_x,
            top_y,
            self.overlay_config,
            max_overlay_glyphs,
        );

        instances
    }
}

fn push_overlay_lines(
    instances: &mut Vec<TextGlyphInstance>,
    lines: &[String],
    origin_x: f32,
    origin_y: f32,
    overlay_config: crate::config::OverlayConfig,
    max_overlay_glyphs: usize,
) {
    let glyph_size = [overlay_config.glyph_width, overlay_config.glyph_height];

    'lines: for (line_index, line) in lines.iter().enumerate() {
        for (column_index, ch) in line.chars().filter_map(overlay_normalize_char).enumerate() {
            if instances.len() >= max_overlay_glyphs {
                break 'lines;
            }

            instances.push(TextGlyphInstance {
                origin_px: [
                    origin_x + column_index as f32 * overlay_config.glyph_advance_x,
                    origin_y + line_index as f32 * overlay_config.line_advance_y,
                ],
                size_px: glyph_size,
                glyph_code: ch as u32,
                _pad: [0; 3],
            });
        }
    }
}

fn overlay_normalize_char(ch: char) -> Option<char> {
    let normalized = match ch {
        '_' => ' ',
        ch if ch.is_ascii_lowercase() => ch.to_ascii_uppercase(),
        ch => ch,
    };

    overlay_supports_char(normalized).then_some(normalized)
}

fn overlay_mode_label(mode: DebugViewMode) -> &'static str {
    match mode {
        DebugViewMode::Shaded => "SHADED",
        DebugViewMode::FaceDir => "FACE DIR",
        DebugViewMode::ChunkCoord => "CHUNK COORD",
        DebugViewMode::DrawId => "DRAW ID",
        DebugViewMode::Wireframe => "WIREFRAME",
    }
}

fn overlay_hiz_label(enabled: bool) -> &'static str {
    if enabled { "ON" } else { "OFF" }
}

fn overlay_supports_char(ch: char) -> bool {
    ch == ' ' || ch == ':' || ch == '-' || ch.is_ascii_digit() || ch.is_ascii_uppercase()
}
