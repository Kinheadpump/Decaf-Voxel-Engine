use crate::engine::render::gpu_types::{DebugViewMode, RenderStats, TextGlyphInstance};

use super::Renderer;

const RIGHT_COLUMN_OFFSET_CHARS: f32 = 32.0;

impl Renderer {
    pub(super) fn build_overlay_instances(&self, stats: RenderStats) -> Vec<TextGlyphInstance> {
        let Some(debug_overlay) = self.debug_overlay.as_ref() else {
            return Vec::new();
        };

        let max_overlay_glyphs = self.overlay_config.max_glyphs;
        let mut instances = Vec::with_capacity(max_overlay_glyphs.min(320));
        let left_x = self.overlay_config.padding_x;
        let right_x = left_x + RIGHT_COLUMN_OFFSET_CHARS * self.overlay_config.glyph_advance_x;
        let top_y = self.overlay_config.padding_y;

        if debug_overlay.show_debug {
            let left_lines = vec![
                "F3 DEBUG".to_string(),
                String::new(),
                "PLAYER".to_string(),
                format!("FPS        {}", debug_overlay.fps),
                format!(
                    "POS        {} {} {}",
                    debug_overlay.player_voxel[0],
                    debug_overlay.player_voxel[1],
                    debug_overlay.player_voxel[2]
                ),
                format!(
                    "CHUNK      {} {} {}",
                    debug_overlay.player_chunk[0],
                    debug_overlay.player_chunk[1],
                    debug_overlay.player_chunk[2]
                ),
                format!("FACING     {}", debug_overlay.player_facing),
                String::new(),
                "TERRAIN".to_string(),
                format!("BIOME      {}", debug_overlay.biome_name),
                format!("REGION     {}", debug_overlay.region_name),
                format!("GROUND Y   {}", debug_overlay.ground_y),
                format!("BIOME Y    {}", debug_overlay.biome_altitude_y),
                format!("TEMP       {}", debug_overlay.temperature_percent),
                format!("HUMID      {}", debug_overlay.humidity_percent),
                format!("CONT       {}", debug_overlay.continentalness_percent),
            ];
            let right_lines = vec![
                "BIOME FIT".to_string(),
                format!("PRIORITY   {}", debug_overlay.biome_priority),
                format!(
                    "TEMP BAND  {}",
                    overlay_percent_band(
                        Some(debug_overlay.biome_temperature_min_percent),
                        Some(debug_overlay.biome_temperature_max_percent),
                    )
                ),
                format!(
                    "HUMID BAND {}",
                    overlay_percent_band(
                        Some(debug_overlay.biome_humidity_min_percent),
                        Some(debug_overlay.biome_humidity_max_percent),
                    )
                ),
                format!(
                    "ALT BAND   {}",
                    overlay_altitude_band(
                        debug_overlay.biome_altitude_min,
                        debug_overlay.biome_altitude_max,
                    )
                ),
                format!(
                    "CONT BAND  {}",
                    overlay_percent_band(
                        debug_overlay.biome_continentalness_min_percent,
                        debug_overlay.biome_continentalness_max_percent,
                    )
                ),
                String::new(),
                "WORLD".to_string(),
                format!("LOADED     {}", debug_overlay.loaded_chunks),
                format!("GPU        {}", stats.gpu_chunks),
                format!("DRAWN      {}", stats.drawn_chunks),
                format!("MESH       {}", stats.meshing_pending_chunks),
                format!("MESH FACE  {}", stats.meshing_faces_uploaded),
                format!("MESH GROW  {}", stats.meshing_slice_buffer_growths),
                String::new(),
                "RENDER".to_string(),
                format!("FRUSTUM    {}", stats.frustum_culled_chunks),
                format!("OCCLUDED   {}", stats.occlusion_culled_chunks),
                format!("DIR CULL   {}", stats.directional_culled_draws),
                format!("OPAQUE     {}", stats.opaque_draws),
                format!("TRANS      {}", stats.transparent_draws),
                format!("HIZ        {}", overlay_hiz_label(stats.hiz_enabled)),
                format!("MODE       {}", overlay_mode_label(self.debug_view_mode)),
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
                &right_lines,
                right_x,
                top_y,
                self.overlay_config,
                max_overlay_glyphs,
            );
        }

        if debug_overlay.show_game_hud {
            let center_x = self.surface_config.width as f32 * 0.5;
            let center_y = self.surface_config.height as f32 * 0.5;

            push_centered_overlay_line(
                &mut instances,
                "X",
                center_x,
                center_y - self.overlay_config.glyph_height * 0.5,
                self.overlay_config,
                max_overlay_glyphs,
            );
            let hotbar_y = self.surface_config.height as f32
                - self.overlay_config.padding_y
                - self.overlay_config.line_advance_y;
            push_centered_overlay_line(
                &mut instances,
                &debug_overlay.hotbar_line,
                center_x,
                hotbar_y.max(self.overlay_config.padding_y),
                self.overlay_config,
                max_overlay_glyphs,
            );
        }

        instances
    }
}

fn overlay_percent_band(min: Option<u8>, max: Option<u8>) -> String {
    match (min, max) {
        (Some(min), Some(max)) => format!("{min}-{max}"),
        (Some(min), None) => format!("{min}-100"),
        (None, Some(max)) => format!("0-{max}"),
        (None, None) => "ANY".to_string(),
    }
}

fn overlay_altitude_band(min: Option<i32>, max: Option<i32>) -> String {
    match (min, max) {
        (Some(min), Some(max)) => format!("{min} TO {max}"),
        (Some(min), None) => format!("{min} UP"),
        (None, Some(max)) => format!("UP TO {max}"),
        (None, None) => "ANY".to_string(),
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

fn push_centered_overlay_line(
    instances: &mut Vec<TextGlyphInstance>,
    line: &str,
    center_x: f32,
    origin_y: f32,
    overlay_config: crate::config::OverlayConfig,
    max_overlay_glyphs: usize,
) {
    let width = overlay_line_width(line, overlay_config);
    let origin_x = (center_x - width * 0.5).max(overlay_config.padding_x);
    let lines = [line.to_string()];
    push_overlay_lines(
        instances,
        &lines,
        origin_x,
        origin_y,
        overlay_config,
        max_overlay_glyphs,
    );
}

fn overlay_line_width(line: &str, overlay_config: crate::config::OverlayConfig) -> f32 {
    let char_count = line.chars().filter_map(overlay_normalize_char).count();
    if char_count == 0 {
        0.0
    } else {
        overlay_config.glyph_width
            + (char_count.saturating_sub(1) as f32 * overlay_config.glyph_advance_x)
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
