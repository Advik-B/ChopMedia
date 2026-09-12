//! Top header bar: brand, load/export actions, status.

use eframe::egui;

use super::VideoClipper;
use crate::icons::{self, Icon};
use crate::theme;

impl VideoClipper {
    pub(super) fn draw_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("ChopMedia")
                    .color(theme::colors::text())
                    .size(19.0)
                    .strong(),
            );
            ui.label(
                egui::RichText::new("video clipper")
                    .color(theme::colors::muted_text())
                    .size(12.5),
            );

            ui.add_space(18.0);
            ui.separator();
            ui.add_space(10.0);

            if icons::icon_text_button(ui, Icon::FolderOpen, "Load Video", true).clicked() {
                self.show_open_file_dialog();
            }

            ui.add_space(6.0);

            let has_video = self.info.is_some();
            let exporting = self.export_handle.is_some() && !self.export_done;
            let export_enabled = has_video && !exporting;
            if icons::primary_icon_text_button(ui, Icon::Download, "Export Clip", export_enabled)
                .clicked()
                && export_enabled
            {
                self.show_export_modal = true;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                if let Some(progress) = self.status_progress {
                    ui.add(
                        egui::ProgressBar::new(progress)
                            .desired_width(140.0)
                            .desired_height(8.0),
                    );
                    ui.add_space(8.0);
                }
                if !self.status_message.is_empty() {
                    ui.label(
                        egui::RichText::new(&self.status_message)
                            .color(theme::colors::muted_text())
                            .size(12.5),
                    );
                }
            });
        });
    }
}
