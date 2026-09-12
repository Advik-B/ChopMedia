//! Waveform strip: mirrored amplitude bars over a center baseline.

use eframe::egui;

use super::VideoClipper;
use crate::theme::colors;

impl VideoClipper {
    pub(super) fn draw_waveform(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let (rect, _response) = ui.allocate_exact_size(available, egui::Sense::hover());

        let painter = ui.painter();
        let mid_y = rect.center().y;

        // Center baseline.
        painter.line_segment(
            [egui::pos2(rect.min.x, mid_y), egui::pos2(rect.max.x, mid_y)],
            egui::Stroke::new(1.0_f32, colors::border()),
        );

        let Some(samples) = &self.waveform_samples else {
            return;
        };
        if samples.len() < 2 {
            return;
        }

        let step_x = rect.width() / samples.len() as f32;
        let bar_w = (step_x * 0.72).max(1.0);
        let max_half = rect.height() * 0.5 - 3.0;
        let corner = egui::CornerRadius::same((bar_w * 0.5).clamp(1.0, 2.0) as u8);

        for (i, sample) in samples.iter().enumerate() {
            let amp = sample.clamp(0.0, 1.0);
            let half = (amp * max_half).max(1.0);
            let cx = rect.min.x + (i as f32 + 0.5) * step_x;

            let bar =
                egui::Rect::from_center_size(egui::pos2(cx, mid_y), egui::vec2(bar_w, half * 2.0));
            painter.rect_filled(bar, corner, colors::waveform_fill());

            // Bright caps at both ends of the bar.
            for y in [mid_y - half, mid_y + half] {
                painter.line_segment(
                    [
                        egui::pos2(cx - bar_w * 0.5, y),
                        egui::pos2(cx + bar_w * 0.5, y),
                    ],
                    egui::Stroke::new(1.0_f32, colors::waveform_line()),
                );
            }
        }
    }
}
