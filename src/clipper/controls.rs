//! Right-hand settings panel: source info, crop (with aspect lock), trim
//! timecodes, export options, and preview volume.

use eframe::egui;

use super::{AspectLock, CropRect, VideoClipper};
use crate::icons::{self, Icon};
use crate::theme::colors;
use crate::timecode;

impl VideoClipper {
    pub(super) fn draw_controls(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);

        let Some(info) = self.info.clone() else {
            ui.vertical_centered(|ui| {
                ui.add_space(32.0);
                ui.label(
                    egui::RichText::new("No video loaded")
                        .size(13.0)
                        .color(colors::muted_text()),
                );
                ui.label(
                    egui::RichText::new("Use Load Video in the header")
                        .size(11.5)
                        .color(colors::muted_text()),
                );
            });
            return;
        };

        // ---- Source ----
        section_heading(ui, None, "Source");
        if let Some(name) = self
            .video_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
        {
            ui.label(egui::RichText::new(name).size(12.5).color(colors::text()));
        }
        ui.label(
            egui::RichText::new(format!(
                "{} x {}  ·  {:.2} fps  ·  {}",
                info.width,
                info.height,
                info.fps,
                timecode::seconds_to_timecode(info.duration),
            ))
            .size(11.5)
            .color(colors::muted_text()),
        );

        ui.add_space(12.0);

        // ---- Crop ----
        section_heading(ui, Some(Icon::Crop), "Crop");
        ui.checkbox(&mut self.is_cropping, "Enable crop");
        if self.is_cropping {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Aspect")
                        .size(12.5)
                        .color(colors::muted_text()),
                );
                let current = AspectLock::PRESETS
                    .iter()
                    .find(|(_, a)| *a == self.crop_aspect)
                    .map(|(l, _)| *l)
                    .unwrap_or("Free");
                let mut changed = false;
                egui::ComboBox::from_id_salt("crop_aspect")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for (label, value) in AspectLock::PRESETS {
                            if ui
                                .selectable_label(self.crop_aspect == value, label)
                                .clicked()
                            {
                                self.crop_aspect = value;
                                changed = true;
                            }
                        }
                    });
                if changed {
                    self.snap_crop_to_aspect();
                }
            });
            if icons::icon_text_button(ui, Icon::RotateCcw, "Reset crop", true).clicked() {
                self.crop_rect = CropRect::default();
            }
            ui.label(
                egui::RichText::new("Cropping requires re-encoding on export.")
                    .size(11.0)
                    .color(colors::warning()),
            );
        }

        ui.add_space(12.0);

        // ---- Trim ----
        section_heading(ui, None, "Trim");
        let mut trim_changed = timecode_row(ui, "In", &mut self.start_time_str);
        trim_changed |= timecode_row(ui, "Out", &mut self.end_time_str);
        if trim_changed {
            self.update_from_text();
        }
        ui.label(
            egui::RichText::new("I / O set In / Out at the playhead")
                .size(11.0)
                .color(colors::muted_text()),
        );
    }
}

/// A small section heading with an optional leading icon.
fn section_heading(ui: &mut egui::Ui, icon: Option<Icon>, label: &str) {
    ui.horizontal(|ui| {
        if let Some(icon) = icon {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(15.0, 15.0), egui::Sense::hover());
            icons::paint(ui.painter(), rect, icon, colors::muted_text());
        }
        ui.label(
            egui::RichText::new(label)
                .size(13.0)
                .strong()
                .color(colors::text()),
        );
    });
    ui.add_space(2.0);
}

/// A labeled monospace timecode text field; returns true when edited.
fn timecode_row(ui: &mut egui::Ui, label: &str, value: &mut String) -> bool {
    ui.horizontal(|ui| {
        ui.add_sized(
            egui::vec2(26.0, 20.0),
            egui::Label::new(
                egui::RichText::new(label)
                    .size(12.5)
                    .color(colors::muted_text()),
            ),
        );
        ui.add(
            egui::TextEdit::singleline(value)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY),
        )
        .changed()
    })
    .inner
}
