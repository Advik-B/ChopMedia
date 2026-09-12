//! Timeline strip: thumbnails, dimmed out-of-range regions, trim-range
//! highlight, a capped playhead, and chunky colored start/end grab handles
//! with drag tooltips.

use eframe::egui;

use super::VideoClipper;
use crate::theme::{self, colors};
use crate::timecode;

impl VideoClipper {
    pub(super) fn draw_timeline(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let (rect, _response) = ui.allocate_exact_size(available, egui::Sense::hover());

        let Some(info) = self.info.clone() else {
            return;
        };
        if info.frame_count <= 0 {
            return;
        }

        let w = rect.width();
        let h = rect.height();

        if !self.thumbnail_textures.is_empty() {
            let painter = ui.painter();
            let thumb_w = w / self.thumbnail_textures.len() as f32;
            for (i, tex) in self.thumbnail_textures.iter().enumerate() {
                if let Some(tex) = tex {
                    let x0 = rect.min.x + i as f32 * thumb_w;
                    let img_rect = egui::Rect::from_min_max(
                        egui::pos2(x0, rect.min.y),
                        egui::pos2(x0 + thumb_w, rect.min.y + h),
                    );
                    painter.image(
                        tex.id(),
                        img_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
            }
        }

        // Scrub interaction is registered first so the trim handles
        // (registered below) win the pointer when they overlap.
        let scrub = ui.interact(
            rect,
            ui.id().with("timeline_scrub"),
            egui::Sense::click_and_drag(),
        );

        let frame_count = info.frame_count as f32;
        let start_x = rect.min.x + (self.start_frame as f32 / frame_count) * w;
        let end_x = rect.min.x + (self.end_frame as f32 / frame_count) * w;
        let playhead_x = rect.min.x + (self.current_frame as f32 / frame_count) * w;

        let painter = ui.painter();

        // Dim everything outside the trim range, then tint the range itself.
        painter.rect_filled(
            egui::Rect::from_min_max(rect.min, egui::pos2(start_x, rect.max.y)),
            0.0,
            colors::timeline_dim(),
        );
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(end_x, rect.min.y), rect.max),
            0.0,
            colors::timeline_dim(),
        );
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(start_x, rect.min.y),
                egui::pos2(end_x, rect.max.y),
            ),
            0.0,
            colors::trim_range_fill(),
        );

        // Trim handles (drawn under the playhead).
        if let Some(new_start) = draw_handle(
            ui,
            rect,
            "start_handle",
            self.start_frame,
            0,
            self.end_frame - 1,
            frame_count,
            info.fps,
            colors::start_marker(),
        ) {
            self.start_frame = new_start;
            self.update_timestamps();
        }
        if let Some(new_end) = draw_handle(
            ui,
            rect,
            "end_handle",
            self.end_frame,
            self.start_frame + 1,
            info.frame_count - 1,
            frame_count,
            info.fps,
            colors::end_marker(),
        ) {
            self.end_frame = new_end;
            self.update_timestamps();
        }

        // Playhead: bright line with a small triangle cap at the top.
        painter.line_segment(
            [
                egui::pos2(playhead_x, rect.min.y),
                egui::pos2(playhead_x, rect.max.y),
            ],
            egui::Stroke::new(2.0_f32, colors::playhead()),
        );
        let cap_half = 4.5;
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(playhead_x - cap_half, rect.min.y),
                egui::pos2(playhead_x + cap_half, rect.min.y),
                egui::pos2(playhead_x, rect.min.y + 6.0),
            ],
            colors::playhead(),
            egui::Stroke::NONE,
        ));

        // Click/drag anywhere on the timeline to scrub.
        if scrub.is_pointer_button_down_on()
            && let Some(pos) = scrub.interact_pointer_pos()
        {
            let frac = ((pos.x - rect.min.x) / w).clamp(0.0, 1.0);
            let new_frame = ((frac * frame_count) as i64).clamp(0, info.frame_count - 1);
            if new_frame != self.current_frame {
                self.show_frame(new_frame);
            }
            theme::chip(
                ui.painter(),
                egui::pos2(pos.x + 12.0, rect.min.y + 6.0),
                &timecode::seconds_to_timecode(new_frame as f64 / info.fps),
            );
        }
    }
}

/// Draws one trim handle: a full-height colored bar with a chunky grab pill
/// at its center. Returns the new frame index while it is being dragged.
#[allow(clippy::too_many_arguments)]
fn draw_handle(
    ui: &egui::Ui,
    rect: egui::Rect,
    id: &str,
    frame: i64,
    min_frame: i64,
    max_frame: i64,
    frame_count: f32,
    fps: f64,
    color: egui::Color32,
) -> Option<i64> {
    let x = rect.min.x + (frame as f32 / frame_count) * rect.width();

    // Generous invisible hit area, much wider than the visible bar.
    let hit = egui::Rect::from_min_max(
        egui::pos2(x - 8.0, rect.min.y),
        egui::pos2(x + 8.0, rect.max.y),
    );
    let response = ui
        .interact(hit, ui.id().with(id), egui::Sense::drag())
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    let engaged = response.hovered() || response.dragged();

    let painter = ui.painter();

    // Full-height bar.
    painter.line_segment(
        [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
        egui::Stroke::new(if engaged { 3.5_f32 } else { 3.0_f32 }, color),
    );

    // Grab pill with two inner grip lines.
    let grip_w = if engaged { 11.0 } else { 9.0 };
    let grip =
        egui::Rect::from_center_size(egui::pos2(x, rect.center().y), egui::vec2(grip_w, 26.0));
    painter.rect_filled(grip, egui::CornerRadius::same(5), color);
    let inner = colors::background();
    for offset in [-2.0_f32, 2.0_f32] {
        painter.line_segment(
            [
                egui::pos2(x + offset, grip.min.y + 7.0),
                egui::pos2(x + offset, grip.max.y - 7.0),
            ],
            egui::Stroke::new(1.5_f32, inner),
        );
    }

    if response.dragged() {
        let new_x = x + response.drag_delta().x;
        let new_frame = (((new_x - rect.min.x) / rect.width()) * frame_count) as i64;
        let new_frame = new_frame.clamp(min_frame.min(max_frame), max_frame.max(min_frame));
        if let Some(pos) = response.interact_pointer_pos() {
            theme::chip(
                painter,
                egui::pos2(pos.x + 12.0, rect.min.y + 6.0),
                &timecode::seconds_to_timecode(new_frame as f64 / fps),
            );
        }
        return Some(new_frame);
    }
    None
}
