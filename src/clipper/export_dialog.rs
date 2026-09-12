//! Export progress modal. Ported from the old `DrawExportPopup`.

use eframe::egui;

use super::VideoClipper;

impl VideoClipper {
    pub(super) fn draw_export_dialog(&mut self, ctx: &egui::Context) {
        if self.export_handle.is_none() {
            return;
        }

        egui::Window::new("Exporting...")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("Export in progress, please wait.");
                ui.add(egui::ProgressBar::new(self.export_progress).desired_width(400.0));
                ui.label(&self.export_message);

                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked()
                        && let Some(handle) = &self.export_handle {
                            handle.cancel();
                        }

                    if self.export_done && ui.button("Close").clicked() {
                        self.export_handle = None;
                        self.export_message.clear();
                    }
                });
            });
    }
}
