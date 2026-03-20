use crate::engine::render::gpu_types::{DebugViewMode, RenderStats, TextGlyphInstance};

use super::Renderer;

const MIDDLE_COLUMN_OFFSET_CHARS: f32 = 24.0;
const RIGHT_COLUMN_OFFSET_CHARS: f32 = 49.0;

impl Renderer {
    pub(super) fn build_overlay_instances(&self, stats: RenderStats) -> Vec<TextGlyphInstance> {
        let Some(debug_overlay) = self.debug_overlay.as_ref() else {
            return Vec::new();
        };

        let max_overlay_glyphs = self.overlay_config.max_glyphs;
        let mut instances = Vec::with_capacity(max_overlay_glyphs.min(192));
        let left_x = self.overlay_config.padding_x;
        let middle_x = left_x + MIDDLE_COLUMN_OFFSET_CHARS * self.overlay_config.glyph_advance_x;
        let right_x = left_x + RIGHT_COLUMN_OFFSET_CHARS * self.overlay_config.glyph_advance_x;
        let top_y = self.overlay_config.padding_y;

        if debug_overlay.show_debug {
            let left_lines = vec![
                "PLAYER".to_string(),
                String::new(),
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
            ];
            let middle_lines = vec![
                "WORLD".to_string(),
                String::new(),
                format!("BIOME    {}", debug_overlay.biome_name),
                format!("REGION   {}", debug_overlay.region_name),
                format!("GROUND   {}", debug_overlay.ground_y),
                format!(
                    "CLIMATE  T{} H{} C{}",
                    debug_overlay.temperature_percent,
                    debug_overlay.humidity_percent,
                    debug_overlay.continentalness_percent
                ),
            ];
            let right_lines = vec![
                "RENDER".to_string(),
                String::new(),
                format!("LOADED   {}", debug_overlay.loaded_chunks),
                format!("DRAWN    {}", stats.drawn_chunks),
                format!("DRAWS    O{} T{}", stats.opaque_draws, stats.transparent_draws),
                format!(
                    "CULL     F{} O{}",
                    stats.frustum_culled_chunks, stats.occlusion_culled_chunks
                ),
                format!(
                    "MESH     P{} U{}",
                    stats.meshing_pending_chunks, stats.meshing_faces_uploaded
                ),
                format!("MODE     {}", overlay_mode_label(self.debug_view_mode)),
                format!("HIZ      {}", overlay_hiz_label(stats.hiz_enabled)),
            ];

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
                &middle_lines,
                middle_x,
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
        }

        self.append_hud_digit_instances(&mut instances, max_overlay_glyphs);

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
    matches!(ch, ' ' | ':' | '-' | '[' | ']' | '<' | '>' | '+')
        || ch.is_ascii_digit()
        || ch.is_ascii_uppercase()
}
