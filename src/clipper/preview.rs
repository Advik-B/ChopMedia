//! Video preview + improved crop overlay: dimmed mask, rule-of-thirds
//! guides, eight resize handles (corners + edges) with an optional
//! aspect-ratio lock, and a live pixel-dimension readout while dragging.

use eframe::egui;

use super::{AspectLock, VideoClipper};
use crate::theme::{self, colors};
use crate::video::VideoInfo;

const CORNER_SIZE: f32 = 12.0;
const HANDLE_HIT: f32 = 20.0;
const MIN_SIZE: f32 = 0.05;

#[derive(Clone, Copy, PartialEq)]
enum Handle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Bottom,
    Left,
    Right,
}

impl Handle {
    const ALL: [Handle; 8] = [
        Handle::TopLeft,
        Handle::TopRight,
        Handle::BottomLeft,
        Handle::BottomRight,
        Handle::Top,
        Handle::Bottom,
        Handle::Left,
        Handle::Right,
    ];

    fn pos(self) -> (f32, f32) {
        match self {
            Handle::TopLeft => (0.0, 0.0),
            Handle::Top => (0.5, 0.0),
            Handle::TopRight => (1.0, 0.0),
            Handle::Left => (0.0, 0.5),
            Handle::Right => (1.0, 0.5),
            Handle::BottomLeft => (0.0, 1.0),
            Handle::Bottom => (0.5, 1.0),
            Handle::BottomRight => (1.0, 1.0),
        }
    }

    fn cursor(self) -> egui::CursorIcon {
        match self {
            Handle::TopLeft | Handle::BottomRight => egui::CursorIcon::ResizeNwSe,
            Handle::TopRight | Handle::BottomLeft => egui::CursorIcon::ResizeNeSw,
            Handle::Top | Handle::Bottom => egui::CursorIcon::ResizeVertical,
            Handle::Left | Handle::Right => egui::CursorIcon::ResizeHorizontal,
        }
    }

    fn is_corner(self) -> bool {
        matches!(
            self,
            Handle::TopLeft | Handle::TopRight | Handle::BottomLeft | Handle::BottomRight
        )
    }

    fn id(self) -> &'static str {
        match self {
            Handle::TopLeft => "crop_tl",
            Handle::TopRight => "crop_tr",
            Handle::BottomLeft => "crop_bl",
            Handle::BottomRight => "crop_br",
            Handle::Top => "crop_t",
            Handle::Bottom => "crop_b",
            Handle::Left => "crop_l",
            Handle::Right => "crop_r",
        }
    }
}

impl VideoClipper {
    pub(super) fn draw_preview(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let (rect, _response) = ui.allocate_exact_size(available, egui::Sense::hover());

        let Some(info) = self.info.clone() else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Load a video to get started",
                egui::FontId::proportional(16.0),
                colors::muted_text(),
            );
            return;
        };

        let Some(tex) = &self.preview_texture else {
            return;
        };

        let aspect = info.width as f32 / info.height as f32;
        let mut w = rect.width();
        let mut h = w / aspect;
        if h > rect.height() {
            h = rect.height();
            w = h * aspect;
        }
        let image_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(w, h));

        ui.painter().image(
            tex.id(),
            image_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        if self.is_cropping {
            self.draw_crop_overlay(ui, image_rect, &info);
        }
    }

    /// Adjusts the crop rect (about its center) so it satisfies the current
    /// aspect lock. Called when the user picks a preset in the control panel.
    pub(crate) fn snap_crop_to_aspect(&mut self) {
        let AspectLock::Ratio(ratio) = self.crop_aspect else {
            return;
        };
        let Some(info) = &self.info else { return };
        let sx = info.width as f32;
        let sy = info.height as f32;

        let cx = self.crop_rect.x + self.crop_rect.w * 0.5;
        let cy = self.crop_rect.y + self.crop_rect.h * 0.5;

        let mut w = self.crop_rect.w.min(1.0);
        let mut h = w * sx / (ratio * sy);
        if h > 1.0 {
            h = 1.0;
            w = h * ratio * sy / sx;
        }

        self.crop_rect.x = (cx - w * 0.5).clamp(0.0, 1.0 - w);
        self.crop_rect.y = (cy - h * 0.5).clamp(0.0, 1.0 - h);
        self.crop_rect.w = w;
        self.crop_rect.h = h;
        self.crop_rect.clamp();
    }

    fn draw_crop_overlay(&mut self, ui: &mut egui::Ui, image_rect: egui::Rect, info: &VideoInfo) {
        let crop_x = image_rect.min.x + self.crop_rect.x * image_rect.width();
        let crop_y = image_rect.min.y + self.crop_rect.y * image_rect.height();
        let crop_w = self.crop_rect.w * image_rect.width();
        let crop_h = self.crop_rect.h * image_rect.height();
        let crop_min = egui::pos2(crop_x, crop_y);
        let crop_max = egui::pos2(crop_x + crop_w, crop_y + crop_h);

        let painter = ui.painter();

        // Dimmed mask around the crop region.
        let mask = colors::crop_mask();
        let regions = [
            egui::Rect::from_min_max(image_rect.min, egui::pos2(image_rect.max.x, crop_y)),
            egui::Rect::from_min_max(egui::pos2(image_rect.min.x, crop_max.y), image_rect.max),
            egui::Rect::from_min_max(
                egui::pos2(image_rect.min.x, crop_y),
                egui::pos2(crop_x, crop_max.y),
            ),
            egui::Rect::from_min_max(
                egui::pos2(crop_max.x, crop_y),
                egui::pos2(image_rect.max.x, crop_max.y),
            ),
        ];
        for r in regions {
            painter.rect_filled(r, 0.0, mask);
        }

        // Rule-of-thirds guides.
        for i in 1..3 {
            let f = i as f32 / 3.0;
            let gx = crop_x + crop_w * f;
            let gy = crop_y + crop_h * f;
            painter.line_segment(
                [egui::pos2(gx, crop_min.y), egui::pos2(gx, crop_max.y)],
                egui::Stroke::new(1.0_f32, colors::crop_guide()),
            );
            painter.line_segment(
                [egui::pos2(crop_min.x, gy), egui::pos2(crop_max.x, gy)],
                egui::Stroke::new(1.0_f32, colors::crop_guide()),
            );
        }

        // Border.
        painter.rect_stroke(
            egui::Rect::from_min_max(crop_min, crop_max),
            0.0,
            egui::Stroke::new(2.0_f32, colors::crop_border()),
            egui::StrokeKind::Middle,
        );

        // Move the whole rect (registered before the handles so they win
        // the pointer where they overlap).
        let move_resp = ui
            .interact(
                egui::Rect::from_min_max(crop_min, crop_max),
                ui.id().with("crop_move"),
                egui::Sense::drag(),
            )
            .on_hover_cursor(egui::CursorIcon::Move);
        if move_resp.dragged() {
            let d = move_resp.drag_delta();
            self.crop_rect.x += d.x / image_rect.width();
            self.crop_rect.y += d.y / image_rect.height();
            self.crop_rect.clamp();
        }

        // Resize handles.
        let mut drag: Option<(Handle, f32, f32, egui::Pos2)> = None;
        for handle in Handle::ALL {
            let (nx, ny) = handle.pos();
            let cx = crop_x + nx * crop_w;
            let cy = crop_y + ny * crop_h;

            let hit = egui::Rect::from_center_size(
                egui::pos2(cx, cy),
                egui::vec2(HANDLE_HIT, HANDLE_HIT),
            );
            let resp = ui
                .interact(hit, ui.id().with(handle.id()), egui::Sense::drag())
                .on_hover_cursor(handle.cursor());
            let engaged = resp.hovered() || resp.dragged();

            if handle.is_corner() {
                let size = if engaged {
                    CORNER_SIZE + 2.0
                } else {
                    CORNER_SIZE
                };
                let r = egui::Rect::from_center_size(egui::pos2(cx, cy), egui::vec2(size, size));
                painter.rect_filled(r, egui::CornerRadius::same(3), colors::crop_handle());
                painter.rect_stroke(
                    r,
                    egui::CornerRadius::same(3),
                    egui::Stroke::new(1.5_f32, colors::crop_border()),
                    egui::StrokeKind::Inside,
                );
            } else {
                let horizontal = matches!(handle, Handle::Top | Handle::Bottom);
                let size = if horizontal {
                    egui::vec2(22.0, 7.0)
                } else {
                    egui::vec2(7.0, 22.0)
                };
                let r = egui::Rect::from_center_size(egui::pos2(cx, cy), size);
                painter.rect_filled(r, egui::CornerRadius::same(3), colors::crop_handle());
            }

            if resp.dragged()
                && let Some(pointer) = resp.interact_pointer_pos()
            {
                let d = resp.drag_delta();
                drag = Some((
                    handle,
                    d.x / image_rect.width(),
                    d.y / image_rect.height(),
                    pointer,
                ));
            }
        }

        if let Some((handle, dx, dy, pointer)) = drag {
            self.resize_crop(handle, dx, dy, info);
            let wpx = (self.crop_rect.w * info.width as f32).round() as i64;
            let hpx = (self.crop_rect.h * info.height as f32).round() as i64;
            theme::chip(
                painter,
                egui::pos2(pointer.x + 14.0, pointer.y + 14.0),
                &format!("{wpx} x {hpx}"),
            );
        }
    }

    /// Applies a handle drag to the crop rect, honoring the aspect lock.
    /// `dx`/`dy` are normalized (0..1 over the video frame).
    fn resize_crop(&mut self, handle: Handle, dx: f32, dy: f32, info: &VideoInfo) {
        let mut x0 = self.crop_rect.x;
        let mut y0 = self.crop_rect.y;
        let mut x1 = x0 + self.crop_rect.w;
        let mut y1 = y0 + self.crop_rect.h;

        match handle {
            Handle::TopLeft => {
                x0 += dx;
                y0 += dy;
            }
            Handle::TopRight => {
                x1 += dx;
                y0 += dy;
            }
            Handle::BottomLeft => {
                x0 += dx;
                y1 += dy;
            }
            Handle::BottomRight => {
                x1 += dx;
                y1 += dy;
            }
            Handle::Top => y0 += dy,
            Handle::Bottom => y1 += dy,
            Handle::Left => x0 += dx,
            Handle::Right => x1 += dx,
        }

        // Clamp everything into bounds first (these ranges are always
        // valid), then enforce the minimum size by pushing the *moved* edge.
        // The invariants maintained by `CropRect::clamp` (0 <= min edge,
        // min edge + MIN_SIZE <= max edge <= 1) keep these pushes in bounds.
        x0 = x0.clamp(0.0, 1.0);
        y0 = y0.clamp(0.0, 1.0);
        x1 = x1.clamp(0.0, 1.0);
        y1 = y1.clamp(0.0, 1.0);
        match handle {
            Handle::TopLeft | Handle::BottomLeft | Handle::Left if x1 - x0 < MIN_SIZE => {
                x0 = x1 - MIN_SIZE;
            }
            Handle::TopRight | Handle::BottomRight | Handle::Right if x1 - x0 < MIN_SIZE => {
                x1 = x0 + MIN_SIZE;
            }
            _ => {}
        }
        match handle {
            Handle::TopLeft | Handle::TopRight | Handle::Top if y1 - y0 < MIN_SIZE => {
                y0 = y1 - MIN_SIZE;
            }
            Handle::BottomLeft | Handle::BottomRight | Handle::Bottom if y1 - y0 < MIN_SIZE => {
                y1 = y0 + MIN_SIZE;
            }
            _ => {}
        }

        if let AspectLock::Ratio(ratio) = self.crop_aspect {
            let sx = info.width as f32;
            let sy = info.height as f32;
            match handle {
                Handle::TopLeft | Handle::TopRight | Handle::BottomLeft | Handle::BottomRight => {
                    // Anchor on the opposite corner; derive height from width.
                    let (ax, dir_x) = match handle {
                        Handle::TopLeft | Handle::BottomLeft => (x1, -1.0_f32),
                        _ => (x0, 1.0_f32),
                    };
                    let (ay, dir_y) = match handle {
                        Handle::TopLeft | Handle::TopRight => (y1, -1.0_f32),
                        _ => (y0, 1.0_f32),
                    };
                    let mut w = (x1 - x0).max(MIN_SIZE);
                    let mut h = w * sx / (ratio * sy);
                    let max_h = if dir_y < 0.0 { ay } else { 1.0 - ay };
                    if h > max_h {
                        h = max_h.max(MIN_SIZE);
                        w = h * ratio * sy / sx;
                    }
                    let max_w = if dir_x < 0.0 { ax } else { 1.0 - ax };
                    if w > max_w {
                        w = max_w.max(MIN_SIZE);
                        h = w * sx / (ratio * sy);
                    }
                    if dir_x < 0.0 {
                        x0 = ax - w;
                    } else {
                        x1 = ax + w;
                    }
                    if dir_y < 0.0 {
                        y0 = ay - h;
                    } else {
                        y1 = ay + h;
                    }
                }
                Handle::Left | Handle::Right => {
                    // Width drives; height follows about the vertical center.
                    let cy = (y0 + y1) * 0.5;
                    let w = (x1 - x0).max(MIN_SIZE);
                    let mut h = w * sx / (ratio * sy);
                    h = h.min(2.0 * cy).min(2.0 * (1.0 - cy)).max(MIN_SIZE);
                    let w = h * ratio * sy / sx;
                    if handle == Handle::Left {
                        x0 = (x1 - w).max(0.0);
                    } else {
                        x1 = (x0 + w).min(1.0);
                    }
                    y0 = cy - h * 0.5;
                    y1 = cy + h * 0.5;
                }
                Handle::Top | Handle::Bottom => {
                    // Height drives; width follows about the horizontal center.
                    let cx = (x0 + x1) * 0.5;
                    let h = (y1 - y0).clamp(MIN_SIZE, 1.0);
                    let mut w = h * ratio * sy / sx;
                    w = w.min(2.0 * cx).min(2.0 * (1.0 - cx)).max(MIN_SIZE);
                    let h = w * sx / (ratio * sy);
                    if handle == Handle::Top {
                        y0 = (y1 - h).max(0.0);
                    } else {
                        y1 = (y0 + h).min(1.0);
                    }
                    x0 = cx - w * 0.5;
                    x1 = cx + w * 0.5;
                }
            }
        }

        self.crop_rect.x = x0;
        self.crop_rect.y = y0;
        self.crop_rect.w = (x1 - x0).max(MIN_SIZE);
        self.crop_rect.h = (y1 - y0).max(MIN_SIZE);
        self.crop_rect.clamp();
    }
}
