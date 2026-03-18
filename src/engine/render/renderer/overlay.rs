use crate::engine::render::gpu_types::{RenderStats, TextGlyphInstance};

use super::Renderer;

impl Renderer {
    pub(super) fn build_overlay_instances(&self, stats: RenderStats) -> Vec<TextGlyphInstance> {
        let Some(debug_overlay) = self.debug_overlay else {
            return Vec::new();
        };

        let mode_label = self.debug_view_mode.label().to_ascii_uppercase();
        let hiz_label = if stats.hiz_enabled { "ON" } else { "OFF" };
        let lines = [
            format!("FPS: {}", debug_overlay.fps),
            format!(
                "POS: {} {} {}",
                debug_overlay.player_voxel[0],
                debug_overlay.player_voxel[1],
                debug_overlay.player_voxel[2]
            ),
            format!(
                "CHK: {} {} {}",
                debug_overlay.player_chunk[0],
                debug_overlay.player_chunk[1],
                debug_overlay.player_chunk[2]
            ),
            format!("FACING: {}", debug_overlay.player_facing),
            format!("WORLD: {}", stats.loaded_chunks),
            format!("GPU: {}", stats.gpu_chunks),
            format!("DRAWN: {}", stats.drawn_chunks),
            format!("FRUSTUM: {}", stats.frustum_culled_chunks),
            format!("OCCLUDED: {}", stats.occlusion_culled_chunks),
            format!("DIR: {}", stats.directional_culled_draws),
            format!("OPAQUE: {}", stats.opaque_draws),
            format!("TRANS: {}", stats.transparent_draws),
            format!("MESH: {}", stats.meshing_pending_chunks),
            format!("HIZ: {hiz_label}"),
            format!("MODE: {mode_label}"),
        ];
        let max_overlay_glyphs = self.overlay_config.max_glyphs;
        let glyph_size = [self.overlay_config.glyph_width, self.overlay_config.glyph_height];
        let mut instances = Vec::with_capacity(max_overlay_glyphs.min(lines.len() * 16));

        'lines: for (line_index, line) in lines.iter().enumerate() {
            for (column_index, ch) in line.chars().enumerate() {
                if instances.len() >= max_overlay_glyphs {
                    break 'lines;
                }

                let ch = ch.to_ascii_uppercase();
                if !overlay_supports_char(ch) {
                    continue;
                }

                instances.push(TextGlyphInstance {
                    origin_px: [
                        self.overlay_config.padding_x
                            + column_index as f32 * self.overlay_config.glyph_advance_x,
                        self.overlay_config.padding_y
                            + line_index as f32 * self.overlay_config.line_advance_y,
                    ],
                    size_px: glyph_size,
                    glyph_code: ch as u32,
                    _pad: [0; 3],
                });
            }
        }

        instances
    }
}

fn overlay_supports_char(ch: char) -> bool {
    ch == ' ' || ch == ':' || ch == '-' || ch.is_ascii_digit() || ch.is_ascii_uppercase()
}
