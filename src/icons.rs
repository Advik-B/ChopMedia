//! Minimal, hand-drawn vector icons in the spirit of [Lucide](https://lucide.dev)
//! (clean 24x24 stroke-based glyphs) - drawn directly with `egui::Painter`
//! primitives rather than rasterized/SVG assets, so there are no extra
//! dependencies and no emoji/text glyphs anywhere in the UI.

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Shape, Stroke};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    SkipBack,
    SkipForward,
    Repeat,
    RotateCcw,
    Crop,
    FolderOpen,
    Download,
    Volume2,
    VolumeX,
}

/// Maps a point in the nominal 24x24 icon grid into `rect`.
fn m(rect: Rect, x: f32, y: f32) -> Pos2 {
    egui::pos2(
        rect.min.x + (x / 24.0) * rect.width(),
        rect.min.y + (y / 24.0) * rect.height(),
    )
}

fn arc_points(
    rect: Rect,
    cx: f32,
    cy: f32,
    r: f32,
    a0_deg: f32,
    a1_deg: f32,
    segments: usize,
) -> Vec<Pos2> {
    (0..=segments)
        .map(|i| {
            let t = i as f32 / segments as f32;
            let a = (a0_deg + (a1_deg - a0_deg) * t).to_radians();
            m(rect, cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// A circular arrow (used for both the loop toggle and the crop reset
/// icon): an arc with a small chevron arrowhead at the leading end.
fn circular_arrow(
    painter: &egui::Painter,
    rect: Rect,
    start_deg: f32,
    end_deg: f32,
    stroke: Stroke,
    color: Color32,
) {
    let (cx, cy, r) = (12.0, 12.5, 7.5);
    painter.line(arc_points(rect, cx, cy, r, start_deg, end_deg, 24), stroke);

    let tip_a = end_deg.to_radians();
    let tip = (cx + r * tip_a.cos(), cy + r * tip_a.sin());
    let sweep_sign = if end_deg > start_deg { 1.0 } else { -1.0 };
    let tangent = (-tip_a.sin() * sweep_sign, tip_a.cos() * sweep_sign);
    let normal = (tip_a.cos(), tip_a.sin());
    let size = 3.4;
    let back = (tip.0 - tangent.0 * size, tip.1 - tangent.1 * size);
    let p1 = (
        back.0 + normal.0 * size * 0.75,
        back.1 + normal.1 * size * 0.75,
    );
    let p2 = (
        back.0 - normal.0 * size * 0.75,
        back.1 - normal.1 * size * 0.75,
    );
    let poly = vec![
        m(rect, tip.0, tip.1),
        m(rect, p1.0, p1.1),
        m(rect, p2.0, p2.1),
    ];
    painter.add(Shape::convex_polygon(poly, color, Stroke::NONE));
}

/// Paints `icon` centered within `rect`, tinted `color`.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let stroke = Stroke::new((rect.width() / 11.0).max(1.3), color);

    match icon {
        Icon::Play => {
            let pts = vec![m(rect, 8.0, 4.5), m(rect, 19.0, 12.0), m(rect, 8.0, 19.5)];
            painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        Icon::Pause => {
            let cr = CornerRadius::same((rect.width() * 0.06).round() as u8);
            painter.rect_filled(
                Rect::from_min_max(m(rect, 6.0, 5.0), m(rect, 10.0, 19.0)),
                cr,
                color,
            );
            painter.rect_filled(
                Rect::from_min_max(m(rect, 14.0, 5.0), m(rect, 18.0, 19.0)),
                cr,
                color,
            );
        }
        Icon::SkipBack => {
            painter.line_segment([m(rect, 6.0, 5.0), m(rect, 6.0, 19.0)], stroke);
            let pts = vec![m(rect, 18.0, 5.0), m(rect, 8.0, 12.0), m(rect, 18.0, 19.0)];
            painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        Icon::SkipForward => {
            painter.line_segment([m(rect, 18.0, 5.0), m(rect, 18.0, 19.0)], stroke);
            let pts = vec![m(rect, 6.0, 5.0), m(rect, 16.0, 12.0), m(rect, 6.0, 19.0)];
            painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        Icon::Repeat => circular_arrow(painter, rect, -205.0, 35.0, stroke, color),
        Icon::RotateCcw => circular_arrow(painter, rect, 205.0, -55.0, stroke, color),
        Icon::Crop => {
            painter.line_segment([m(rect, 6.0, 2.0), m(rect, 6.0, 17.0)], stroke);
            painter.line_segment([m(rect, 6.0, 17.0), m(rect, 21.0, 17.0)], stroke);
            painter.line_segment([m(rect, 18.0, 22.0), m(rect, 18.0, 7.0)], stroke);
            painter.line_segment([m(rect, 18.0, 7.0), m(rect, 3.0, 7.0)], stroke);
        }
        Icon::FolderOpen => {
            let pts = vec![
                m(rect, 3.0, 7.0),
                m(rect, 3.0, 19.0),
                m(rect, 21.0, 19.0),
                m(rect, 21.0, 9.0),
                m(rect, 12.0, 9.0),
                m(rect, 10.0, 6.0),
                m(rect, 5.0, 6.0),
                m(rect, 3.0, 7.0),
            ];
            painter.line(pts, stroke);
        }
        Icon::Download => {
            painter.line_segment([m(rect, 12.0, 3.0), m(rect, 12.0, 15.0)], stroke);
            painter.line_segment([m(rect, 7.0, 10.0), m(rect, 12.0, 15.5)], stroke);
            painter.line_segment([m(rect, 17.0, 10.0), m(rect, 12.0, 15.5)], stroke);
            painter.line_segment([m(rect, 4.0, 19.0), m(rect, 20.0, 19.0)], stroke);
        }
        Icon::Volume2 => {
            let pts = vec![
                m(rect, 4.0, 9.0),
                m(rect, 8.0, 9.0),
                m(rect, 12.0, 5.0),
                m(rect, 12.0, 19.0),
                m(rect, 8.0, 15.0),
                m(rect, 4.0, 15.0),
                m(rect, 4.0, 9.0),
            ];
            painter.line(pts, stroke);
            painter.line(arc_points(rect, 15.0, 12.0, 3.0, -50.0, 50.0, 10), stroke);
            painter.line(arc_points(rect, 15.0, 12.0, 6.0, -50.0, 50.0, 10), stroke);
        }
        Icon::VolumeX => {
            let pts = vec![
                m(rect, 4.0, 9.0),
                m(rect, 8.0, 9.0),
                m(rect, 12.0, 5.0),
                m(rect, 12.0, 19.0),
                m(rect, 8.0, 15.0),
                m(rect, 4.0, 15.0),
                m(rect, 4.0, 9.0),
            ];
            painter.line(pts, stroke);
            painter.line_segment([m(rect, 15.5, 9.5), m(rect, 21.0, 15.0)], stroke);
            painter.line_segment([m(rect, 21.0, 9.5), m(rect, 15.5, 15.0)], stroke);
        }
    }
}

/// A round-ish clickable icon button with hover/active styling. Returns the
/// interaction `Response` so callers can check `.clicked()` and attach
/// tooltips via `.on_hover_text(...)`.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, size: f32, active: bool) -> egui::Response {
    let desired = egui::vec2(size, size);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());

    let accent = crate::theme::colors::accent();
    let bg = if active {
        accent
    } else if response.hovered() {
        ui.visuals().widgets.hovered.weak_bg_fill
    } else {
        Color32::TRANSPARENT
    };

    let corner = CornerRadius::same((size * 0.28) as u8);
    ui.painter().rect_filled(rect, corner, bg);

    let icon_color = if active {
        Color32::WHITE
    } else if response.hovered() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };

    let icon_rect = rect.shrink(size * 0.24);
    paint(ui.painter(), icon_rect, icon, icon_color);

    response
}

/// A larger "primary" transport button (e.g. the big Play/Pause) with a
/// filled accent background.
pub fn primary_icon_button(ui: &mut egui::Ui, icon: Icon, size: f32) -> egui::Response {
    let desired = egui::vec2(size, size);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());

    let accent = crate::theme::colors::accent();
    let bg = if response.hovered() {
        crate::theme::colors::accent_hovered()
    } else {
        accent
    };

    ui.painter().circle_filled(rect.center(), size * 0.5, bg);

    let icon_rect = rect.shrink(size * 0.28);
    paint(ui.painter(), icon_rect, icon, Color32::WHITE);

    response
}

/// Like [`icon_text_button`], but filled with the accent color to make the
/// button stand out as the primary call-to-action.
pub fn primary_icon_text_button(
    ui: &mut egui::Ui,
    icon: Icon,
    text: &str,
    enabled: bool,
) -> egui::Response {
    let padding = ui.spacing().button_padding;
    let icon_size = 15.0;
    let gap = 7.0;

    let fg = if enabled {
        Color32::WHITE
    } else {
        ui.visuals().weak_text_color()
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), egui::FontId::proportional(14.0), fg);

    let desired = egui::vec2(
        icon_size + gap + galley.size().x + padding.x * 2.0,
        22.0f32.max(galley.size().y) + padding.y * 2.0,
    );
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(desired, sense);

    let bg = if !enabled {
        crate::theme::colors::card()
    } else if response.hovered() {
        crate::theme::colors::accent_hovered()
    } else if response.is_pointer_button_down_on() {
        crate::theme::colors::accent_active()
    } else {
        crate::theme::colors::accent()
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), bg);

    let icon_rect = Rect::from_min_size(
        egui::pos2(rect.min.x + padding.x, rect.center().y - icon_size * 0.5),
        egui::vec2(icon_size, icon_size),
    );
    paint(ui.painter(), icon_rect, icon, fg);

    let text_pos = egui::pos2(
        icon_rect.max.x + gap,
        rect.center().y - galley.size().y * 0.5,
    );
    ui.painter().galley(text_pos, galley, fg);

    response
}

/// A pill-shaped button with an icon followed by a text label, used for
/// primary actions (Load Video, Export...).
pub fn icon_text_button(
    ui: &mut egui::Ui,
    icon: Icon,
    text: &str,
    enabled: bool,
) -> egui::Response {
    let padding = ui.spacing().button_padding;
    let icon_size = 15.0;
    let gap = 7.0;

    let text_color = if enabled {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(14.0),
        text_color,
    );

    let desired = egui::vec2(
        icon_size + gap + galley.size().x + padding.x * 2.0,
        22.0f32.max(galley.size().y) + padding.y * 2.0,
    );
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(desired, sense);

    let corner = CornerRadius::same(8);
    let bg = if !enabled {
        Color32::TRANSPARENT
    } else if response.hovered() {
        ui.visuals().widgets.hovered.weak_bg_fill
    } else {
        ui.visuals().widgets.inactive.weak_bg_fill
    };
    ui.painter().rect_filled(rect, corner, bg);
    if enabled {
        let stroke_color = if response.hovered() {
            crate::theme::colors::accent()
        } else {
            crate::theme::colors::border()
        };
        ui.painter().rect_stroke(
            rect,
            corner,
            Stroke::new(1.0_f32, stroke_color),
            egui::StrokeKind::Inside,
        );
    }

    let icon_rect = Rect::from_min_size(
        egui::pos2(rect.min.x + padding.x, rect.center().y - icon_size * 0.5),
        egui::vec2(icon_size, icon_size),
    );
    paint(ui.painter(), icon_rect, icon, text_color);

    let text_pos = egui::pos2(
        icon_rect.max.x + gap,
        rect.center().y - galley.size().y * 0.5,
    );
    ui.painter().galley(text_pos, galley, text_color);

    response
}
