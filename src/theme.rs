//! A bold, modern dark theme (not aiming to match the old ImGui look).

use eframe::egui::{self, Color32, CornerRadius, Stroke};

fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
        (a * 255.0).round() as u8,
    )
}

/// Builds the app's visuals: a near-black canvas, soft elevated panels, and
/// a vivid indigo accent.
pub fn visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();

    let text = colors::text();
    let window_bg = colors::background();
    let panel_bg = colors::panel();
    let card_bg = colors::card();
    let border = colors::border();
    let accent = colors::accent();
    let accent_hovered = colors::accent_hovered();
    let accent_active = colors::accent_active();

    visuals.override_text_color = None;
    visuals.window_fill = window_bg;
    visuals.panel_fill = window_bg;
    visuals.faint_bg_color = card_bg;
    visuals.extreme_bg_color = colors::field_bg();
    visuals.code_bg_color = card_bg;
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.window_stroke = Stroke::new(1.0_f32, border);
    visuals.window_shadow.color = rgba(0.0, 0.0, 0.0, 0.55);
    visuals.popup_shadow.color = rgba(0.0, 0.0, 0.0, 0.55);
    visuals.resize_corner_size = 8.0;

    visuals.selection.bg_fill = accent;
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.hyperlink_color = accent;
    visuals.warn_fg_color = colors::warning();
    visuals.error_fg_color = colors::danger();

    visuals.widgets.noninteractive.bg_fill = panel_bg;
    visuals.widgets.noninteractive.weak_bg_fill = panel_bg;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, border);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, text);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);

    visuals.widgets.inactive.bg_fill = card_bg;
    visuals.widgets.inactive.weak_bg_fill = card_bg;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, border);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, text);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(8);

    visuals.widgets.hovered.bg_fill = colors::card_hovered();
    visuals.widgets.hovered.weak_bg_fill = colors::card_hovered();
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.2_f32, accent);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(8);
    visuals.widgets.hovered.expansion = 0.5;

    visuals.widgets.active.bg_fill = accent_active;
    visuals.widgets.active.weak_bg_fill = accent_active;
    visuals.widgets.active.bg_stroke = Stroke::new(1.2_f32, accent_hovered);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.active.corner_radius = CornerRadius::same(8);

    visuals.widgets.open.bg_fill = accent;
    visuals.widgets.open.weak_bg_fill = accent;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, accent_hovered);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.open.corner_radius = CornerRadius::same(8);

    visuals
}

/// Roomier default spacing, to match the bolder visual language.
pub fn spacing() -> egui::style::Spacing {
    egui::style::Spacing {
        item_spacing: egui::vec2(8.0, 8.0),
        button_padding: egui::vec2(10.0, 6.0),
        window_margin: egui::Margin::same(12),
        menu_margin: egui::Margin::same(8),
        indent: 14.0,
        interact_size: egui::vec2(40.0, 26.0),
        ..Default::default()
    }
}

/// Colors used directly by custom-painted widgets (timeline, waveform, crop
/// overlay, icons) that don't go through egui's `Visuals`/widget system.
pub mod colors {
    use super::rgba;
    use eframe::egui::Color32;

    pub fn background() -> Color32 {
        rgba(0.043, 0.047, 0.059, 1.0)
    }
    pub fn panel() -> Color32 {
        rgba(0.067, 0.075, 0.094, 1.0)
    }
    pub fn card() -> Color32 {
        rgba(0.11, 0.12, 0.15, 1.0)
    }
    pub fn card_hovered() -> Color32 {
        rgba(0.15, 0.16, 0.20, 1.0)
    }
    pub fn field_bg() -> Color32 {
        rgba(0.03, 0.035, 0.045, 1.0)
    }
    pub fn border() -> Color32 {
        rgba(0.22, 0.24, 0.29, 1.0)
    }
    pub fn text() -> Color32 {
        rgba(0.94, 0.95, 0.97, 1.0)
    }
    pub fn muted_text() -> Color32 {
        rgba(0.58, 0.61, 0.68, 1.0)
    }

    /// Primary accent: vivid indigo.
    pub fn accent() -> Color32 {
        rgba(0.46, 0.38, 0.98, 1.0)
    }
    pub fn accent_hovered() -> Color32 {
        rgba(0.55, 0.48, 1.0, 1.0)
    }
    pub fn accent_active() -> Color32 {
        rgba(0.38, 0.30, 0.90, 1.0)
    }

    /// Secondary accent: teal, used for "loop active" / success states.
    pub fn teal() -> Color32 {
        rgba(0.28, 0.87, 0.79, 1.0)
    }

    pub fn warning() -> Color32 {
        rgba(0.97, 0.65, 0.25, 1.0)
    }
    pub fn danger() -> Color32 {
        rgba(0.95, 0.35, 0.40, 1.0)
    }

    // ---- Timeline ----

    pub fn trim_range_fill() -> Color32 {
        rgba(0.46, 0.38, 0.98, 0.22)
    }
    pub fn playhead() -> Color32 {
        rgba(0.98, 0.98, 1.0, 1.0)
    }
    /// Trim-start handle color (distinct from end, for quick recognition).
    pub fn start_marker() -> Color32 {
        teal()
    }
    /// Trim-end handle color.
    pub fn end_marker() -> Color32 {
        rgba(0.98, 0.55, 0.42, 1.0)
    }

    // ---- Crop ----

    pub fn crop_mask() -> Color32 {
        rgba(0.0, 0.0, 0.0, 0.6)
    }
    pub fn crop_border() -> Color32 {
        Color32::WHITE
    }
    pub fn crop_guide() -> Color32 {
        rgba(1.0, 1.0, 1.0, 0.35)
    }
    pub fn crop_handle() -> Color32 {
        accent_hovered()
    }

    // ---- Waveform ----

    pub fn waveform_line() -> Color32 {
        accent_hovered()
    }
    pub fn waveform_fill() -> Color32 {
        rgba(0.46, 0.38, 0.98, 0.28)
    }

    /// Background chip used for floating labels (drag tooltips, dimension
    /// readouts) painted directly via `Painter`.
    pub fn chip_bg() -> Color32 {
        rgba(0.03, 0.035, 0.045, 0.92)
    }

    /// Darkening applied to timeline thumbnails outside the trim range.
    pub fn timeline_dim() -> Color32 {
        rgba(0.0, 0.0, 0.0, 0.45)
    }
}

/// Paints a small floating label (drag tooltip / dimension readout) with its
/// top-left corner at `anchor`.
pub fn chip(painter: &egui::Painter, anchor: egui::Pos2, text: &str) {
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        egui::FontId::monospace(12.0),
        colors::text(),
    );
    let padding = egui::vec2(8.0, 5.0);
    let rect = egui::Rect::from_min_size(anchor, galley.size() + padding * 2.0);
    painter.rect_filled(rect, CornerRadius::same(6), colors::chip_bg());
    painter.rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.0_f32, colors::border()),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + padding, galley, colors::text());
}
