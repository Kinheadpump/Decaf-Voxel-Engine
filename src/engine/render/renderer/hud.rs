use crate::engine::render::gpu_types::{HudSpriteInstance, TextGlyphInstance};

use super::{MAX_HUD_SPRITES, Renderer};

const HOTBAR_SLOT_COUNT: usize = 9;
const HOTBAR_CENTER_INDEX: f32 = 4.0;
const HUD_UV_MIN: [f32; 2] = [0.0, 0.0];
const HUD_UV_MAX: [f32; 2] = [1.0, 1.0];

#[derive(Clone, Copy)]
struct HudLayout {
    center_x: f32,
    hotbar_center_y: f32,
    slot_pitch: f32,
    slot_size: f32,
    selected_slot_size: f32,
    icon_size: f32,
    selected_icon_size: f32,
    selected_lift: f32,
    digit_size: [f32; 2],
    digit_inset: [f32; 2],
    crosshair_size: f32,
}

impl Renderer {
    pub(super) fn build_hud_sprite_instances(&self) -> Vec<HudSpriteInstance> {
        let Some(debug_overlay) = self.debug_overlay.as_ref() else {
            return Vec::new();
        };
        if !debug_overlay.show_game_hud {
            return Vec::new();
        }

        let layout =
            hud_layout(self.surface_config.width as f32, self.surface_config.height as f32);
        let mut instances = Vec::with_capacity(MAX_HUD_SPRITES.min(24));

        push_sprite(
            &mut instances,
            centered_rect(
                layout.center_x,
                self.surface_config.height as f32 * 0.5,
                layout.crosshair_size,
                layout.crosshair_size,
            ),
            u32::from(self.hud_crosshair_layer),
            [1.0, 1.0, 1.0, 0.96],
        );

        for (slot_index, &icon_layer) in debug_overlay.hotbar_icon_layers.iter().enumerate() {
            let selected = slot_index as u32 == debug_overlay.selected_hotbar_slot;
            let slot_rect = hotbar_slot_rect(layout, slot_index, selected);
            let icon_size = if selected { layout.selected_icon_size } else { layout.icon_size };
            let icon_rect = centered_rect(
                slot_rect.origin_px[0] + slot_rect.size_px[0] * 0.5,
                slot_rect.origin_px[1] + slot_rect.size_px[1] * 0.5,
                icon_size,
                icon_size,
            );
            let frame_layer =
                if selected { self.hud_selected_slot_layer } else { self.hud_slot_layer };

            push_sprite(&mut instances, slot_rect, u32::from(frame_layer), [1.0, 1.0, 1.0, 1.0]);
            push_sprite(&mut instances, icon_rect, u32::from(icon_layer), [1.0, 1.0, 1.0, 1.0]);
        }

        instances
    }

    pub(super) fn append_hud_digit_instances(
        &self,
        instances: &mut Vec<TextGlyphInstance>,
        max_overlay_glyphs: usize,
    ) {
        let Some(debug_overlay) = self.debug_overlay.as_ref() else {
            return;
        };
        if !debug_overlay.show_game_hud {
            return;
        }

        let layout =
            hud_layout(self.surface_config.width as f32, self.surface_config.height as f32);

        for slot_index in 0..HOTBAR_SLOT_COUNT {
            if instances.len() >= max_overlay_glyphs {
                break;
            }

            let selected = slot_index as u32 == debug_overlay.selected_hotbar_slot;
            let slot_rect = hotbar_slot_rect(layout, slot_index, selected);
            instances.push(TextGlyphInstance {
                origin_px: [
                    slot_rect.origin_px[0] + layout.digit_inset[0],
                    slot_rect.origin_px[1] + layout.digit_inset[1],
                ],
                size_px: layout.digit_size,
                glyph_code: (b'1' + slot_index as u8) as u32,
                _pad: [0; 3],
            });
        }
    }
}

fn hud_layout(surface_width: f32, surface_height: f32) -> HudLayout {
    let ui_scale = (surface_height / 720.0).clamp(0.9, 1.35);
    HudLayout {
        center_x: surface_width * 0.5,
        hotbar_center_y: surface_height - 58.0 * ui_scale,
        slot_pitch: 60.0 * ui_scale,
        slot_size: 56.0 * ui_scale,
        selected_slot_size: 64.0 * ui_scale,
        icon_size: 34.0 * ui_scale,
        selected_icon_size: 40.0 * ui_scale,
        selected_lift: 7.0 * ui_scale,
        digit_size: [11.0 * ui_scale, 13.0 * ui_scale],
        digit_inset: [7.0 * ui_scale, 6.0 * ui_scale],
        crosshair_size: 22.0 * ui_scale,
    }
}

fn hotbar_slot_rect(layout: HudLayout, slot_index: usize, selected: bool) -> HudSpriteRect {
    let slot_center_x =
        layout.center_x + (slot_index as f32 - HOTBAR_CENTER_INDEX) * layout.slot_pitch;
    let slot_center_y = if selected {
        layout.hotbar_center_y - layout.selected_lift
    } else {
        layout.hotbar_center_y
    };
    let slot_size = if selected { layout.selected_slot_size } else { layout.slot_size };

    centered_rect(slot_center_x, slot_center_y, slot_size, slot_size)
}

#[derive(Clone, Copy)]
struct HudSpriteRect {
    origin_px: [f32; 2],
    size_px: [f32; 2],
}

fn centered_rect(center_x: f32, center_y: f32, width: f32, height: f32) -> HudSpriteRect {
    HudSpriteRect {
        origin_px: [center_x - width * 0.5, center_y - height * 0.5],
        size_px: [width, height],
    }
}

fn push_sprite(
    instances: &mut Vec<HudSpriteInstance>,
    rect: HudSpriteRect,
    texture_layer: u32,
    tint: [f32; 4],
) {
    if instances.len() >= MAX_HUD_SPRITES {
        return;
    }

    instances.push(HudSpriteInstance {
        origin_px: rect.origin_px,
        size_px: rect.size_px,
        uv_min: HUD_UV_MIN,
        uv_max: HUD_UV_MAX,
        tint,
        texture_layer,
        _pad: [0; 3],
    });
}
